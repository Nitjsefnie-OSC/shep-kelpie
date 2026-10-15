//! The comment that says what the research found, and the labels after it
//!
//! The representative words the facts, kelpie's code appends [`MARKER`] and
//! posts the comment as the App, and then puts the labels on. The record in
//! `triage.issues` says how far an issue got, and it is saved before each
//! next write to the forge, so a restart goes on from its step and posts
//! nothing twice. A post the forge took whose record did not save is caught by
//! the marker, which only the App's own comment counts for.
//!
//! A step that fails is told once in the log and tried at the next poll, and
//! five failures give the issue up until kelpie restarts.

use super::super::Runner;
use super::super::contributions::post::{forge_is_busy, refused_text};
use super::super::representative::IssuePost;
use super::super::thread_replies::plain_login;
use super::research::Found;
use crate::board::TRIAGE;
use crate::github::{Apps, Owner};
use crate::ports::{ForgeError, NewLabel};
use crate::state::{StateError, TriageStep, Triaged};

/// What marks a comment as kelpie's triage comment
pub(in crate::runner) const MARKER: &str = "<!-- kelpie:triage -->";

/// How many times a step may fail for an issue before kelpie stops trying it
pub(super) const TRIES: u32 = 5;

// Made on a repo that lacks it before the first issue is labelled with it
const NEEDS_TRIAGE: NewLabel<'static> = NewLabel {
    name: TRIAGE,
    color: "d876e3",
    description: "Kelpie researched this issue; it waits on the maintainer",
};

/// What kind of post the representative is asked for
const KIND: &str = "Write the first triage comment on this issue, from what kelpie found in the \
    repo. Greet the author, say what was found, and ask only the questions listed. It is a \
    comment, not a review: no verdict, and nothing promised.";

/// A research that read, whose comment is not yet on the issue
#[derive(Debug)]
pub(super) struct Waiting {
    pub(super) author: String,
    pub(super) said: String,
    pub(super) found: Found,
}

/// What the runner keeps of one issue's triage that has not gone through, in
/// memory only
#[derive(Debug, Default)]
pub(in crate::runner) struct Tries {
    failures: u32,
    told: bool,
    // Whether its comments were read for the App's marker; a post that may have
    // gone up unrecorded clears it
    pub(super) read: bool,
    // What the research found, kept until the comment is recorded so that a
    // failed post is tried again without another call
    pub(super) waiting: Option<Waiting>,
    // The comment as worded, kept until the forge takes it
    pub(super) body: Option<String>,
    // The labels the research asked for that the repo has, put on after the
    // comment; the record keeps them from there
    labels: Vec<String>,
}

impl Tries {
    // What an issue asked for by hand starts with: its comments need no read
    // before the first call
    pub(super) fn asked() -> Self {
        Self {
            read: true,
            ..Self::default()
        }
    }
}

impl Runner {
    // The issue's record, if the state holds one
    pub(super) fn triage_record(&self, issue: u64) -> Option<&Triaged> {
        let triage = self.state.triage.as_ref()?;
        triage.issues.iter().find(|t| t.issue == issue)
    }

    // Whether `issue` was given up on
    pub(super) fn triage_given_up(&self, issue: u64) -> bool {
        self.triage_tries
            .get(&issue)
            .is_some_and(|t| t.failures >= TRIES)
    }

    // Says in the log that `why` went wrong for `issue`, once, and again when
    // it is given up. A failure the forge being busy explains does not count,
    // since asking again later may work.
    pub(in crate::runner) fn triage_failed(&mut self, issue: u64, why: &str, counts: bool) {
        let tries = self.triage_tries.entry(issue).or_default();
        if counts {
            tries.failures += 1;
        }
        let line = if tries.failures >= TRIES {
            format!(
                "triage #{issue}: {why}; it failed {TRIES} times, so it is not tried again \
                 until kelpie restarts"
            )
        } else if !tries.told {
            tries.told = true;
            format!("triage #{issue}: {why}; it is tried at the next poll")
        } else {
            return;
        };
        self.notes.push(line);
    }

    // The login of the App that covers the repo, as its comments name it
    fn app_login(&self) -> Option<String> {
        let owner = Owner::of(&self.remote).ok()?;
        let app = Apps::under(&self.paths.kelpie_home).get(&owner).ok()??;
        Some(app.slug)
    }

    // The id of the comment on `issue` the App wrote with the marker in it,
    // past `after` when a comment is given: a triage asked again is not found
    // by the comment of the one before. A marker in anyone else's comment
    // counts for nothing.
    pub(super) fn marked_by_app(
        &self,
        issue: u64,
        after: Option<u64>,
    ) -> Result<Option<u64>, ForgeError> {
        let comments = self.ports.forge.issue_comments(&self.remote, issue)?;
        let Some(app) = self.app_login().map(|app| plain_login(&app)) else {
            return Ok(None);
        };
        let ours = |c: &&crate::ports::ThreadComment| {
            c.body.contains(MARKER) && plain_login(&c.author) == app && Some(c.id) > after
        };
        Ok(comments.iter().filter(ours).map(|c| c.id).min())
    }

    // Posts what the research found: the labels the repo has are read first,
    // so the comment names only those, then the representative words the
    // facts, and the comment goes up with the marker under it.
    pub(in crate::runner) fn post_triage(
        &mut self,
        issue: u64,
        author: &str,
        said: &str,
        found: &Found,
    ) -> Result<(), StateError> {
        let tries = self.triage_tries.entry(issue).or_default();
        tries.waiting = Some(Waiting {
            author: author.to_owned(),
            said: said.to_owned(),
            found: found.clone(),
        });
        self.say_triage(issue)
    }

    // Retries the comment of an issue whose research read: the wording kept is
    // posted, or else it is worded again from what the research found
    pub(super) fn retry_triage(&mut self, issue: u64) -> Result<(), StateError> {
        let Some(tries) = self.triage_tries.get(&issue) else {
            return Ok(());
        };
        if tries.body.is_some() {
            return self.send_triage(issue);
        }
        self.say_triage(issue)
    }

    // Words the comment from the waiting research and posts it
    fn say_triage(&mut self, issue: u64) -> Result<(), StateError> {
        let Some(waiting) = self
            .triage_tries
            .get(&issue)
            .and_then(|t| t.waiting.as_ref())
        else {
            return Ok(());
        };
        let (author, said, found) = (
            waiting.author.clone(),
            waiting.said.clone(),
            waiting.found.clone(),
        );
        let (author, said, found) = (author.as_str(), said.as_str(), &found);
        let have = match self.ports.forge.repo_labels(&self.remote) {
            Ok(have) => have,
            Err(e) => {
                let why = format!("cannot read the repo's labels: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                return Ok(());
            }
        };
        let wanted = found.kind.into_iter().chain(found.priority);
        let labels: Vec<String> = wanted
            .filter_map(|want| have.iter().find(|l| l.eq_ignore_ascii_case(want)))
            .cloned()
            .collect();
        let facts = facts(found, &labels);
        let earlier: Vec<u64> = self
            .triage_record(issue)
            .map(|t| t.comment)
            .into_iter()
            .collect();
        let post = IssuePost {
            kind: KIND,
            facts: &facts,
            earlier: &earlier,
            author,
            said,
        };
        let words = match self.represent_issue(issue, &post) {
            Ok(words) => words,
            Err(e) => {
                self.triage_failed(issue, &e.to_string(), true);
                return Ok(());
            }
        };
        let tries = self.triage_tries.entry(issue).or_default();
        tries.body = Some(format!("{words}\n\n{MARKER}"));
        tries.labels = labels;
        self.send_triage(issue)
    }

    // Posts the comment kept for `issue`, records it, and goes on to its labels
    pub(super) fn send_triage(&mut self, issue: u64) -> Result<(), StateError> {
        let Some(body) = self.triage_tries.get(&issue).and_then(|t| t.body.clone()) else {
            return Ok(());
        };
        // The forge may take a post whose answer is lost, so the marker is read again.
        if let Some(tries) = self.triage_tries.get_mut(&issue) {
            tries.read = false;
        }
        match self.ports.voice.post(&self.remote, issue, &body) {
            Ok(posted) => {
                self.record_triage(issue, posted.id)?;
                self.finish_triage(issue)
            }
            Err(e) => {
                let why = format!("cannot post the comment: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                // The guard may have refused the wording, so it is worded again.
                if refused_text(&e)
                    && let Some(tries) = self.triage_tries.get_mut(&issue)
                {
                    tries.body = None;
                }
                Ok(())
            }
        }
    }

    // Records `comment` as `issue`'s triage comment, in place of any record it
    // had, at the step that says the forge has it
    pub(super) fn record_triage(&mut self, issue: u64, comment: u64) -> Result<(), StateError> {
        let mut next = self.state.clone();
        let Some(triage) = next.triage.as_mut() else {
            return Ok(());
        };
        triage.issues.retain(|t| t.issue != issue);
        let labels = (self.triage_tries.get(&issue)).map_or_else(Vec::new, |t| t.labels.clone());
        triage.issues.push(Triaged {
            issue,
            comment,
            step: TriageStep::Commented,
            labels,
        });
        self.save(next)?;
        self.triage_due.retain(|d| d.issue != issue);
        self.triage_queue.retain(|&n| n != issue);
        if let Some(tries) = self.triage_tries.get_mut(&issue) {
            tries.body = None;
            tries.waiting = None;
        }
        Ok(())
    }

    fn triage_stepped(&mut self, issue: u64, step: TriageStep) -> Result<(), StateError> {
        let mut next = self.state.clone();
        let held = next
            .triage
            .as_mut()
            .and_then(|t| t.issues.iter_mut().find(|t| t.issue == issue));
        let Some(held) = held else {
            return Ok(());
        };
        held.step = step;
        held.labels.clear();
        if step == TriageStep::Done
            && let Some(triage) = next.triage.as_mut()
        {
            triage.prune();
        }
        self.save(next)
    }

    // Does what `issue`'s record says is left: the labels the research asked
    // for, then `needs-triage`, each step saved before the next write
    pub(super) fn finish_triage(&mut self, issue: u64) -> Result<(), StateError> {
        loop {
            let Some(step) = self.triage_record(issue).map(|t| t.step) else {
                return Ok(());
            };
            match step {
                TriageStep::Done => return Ok(()),
                TriageStep::Commented => {
                    let labels = self
                        .triage_record(issue)
                        .map_or_else(Vec::new, |t| t.labels.clone());
                    for label in labels {
                        if let Err(e) = self.label_issue(issue, &label, true) {
                            let why = format!("cannot label it `{label}`: {e}");
                            self.triage_failed(issue, &why, !forge_is_busy(&e));
                            return Ok(());
                        }
                    }
                    self.triage_stepped(issue, TriageStep::Labelled)?;
                }
                TriageStep::Labelled => {
                    if let Err(why) = self.make_missing_labels([NEEDS_TRIAGE]) {
                        self.triage_failed(issue, &format!("cannot make `{TRIAGE}`: {why}"), true);
                        return Ok(());
                    }
                    if let Err(e) = self.label_issue(issue, TRIAGE, true) {
                        let why = format!("cannot label it `{TRIAGE}`: {e}");
                        self.triage_failed(issue, &why, !forge_is_busy(&e));
                        return Ok(());
                    }
                    self.triage_stepped(issue, TriageStep::Done)?;
                    self.triage_tries.remove(&issue);
                    return Ok(());
                }
            }
        }
    }
}

// What the representative words the comment from: the labels that go on, the
// related issues as `#n`, the files, tests and draft criteria, and the questions
fn facts(found: &Found, labels: &[String]) -> String {
    let mut facts = String::from("Kelpie read the issue against the repo's main branch.\n");
    if !labels.is_empty() {
        facts.push_str(&format!("\nKelpie labels it: {}.\n", labels.join(", ")));
    }
    if !found.related.is_empty() {
        let related: Vec<String> = found.related.iter().map(|(n, _)| format!("#{n}")).collect();
        facts.push_str(&format!("\nRelated open issues: {}.\n", related.join(", ")));
    }
    let lists = [
        ("Files the work would touch", &found.files, "`"),
        ("Tests that would pin it", &found.tests, "`"),
        ("Draft acceptance criteria", &found.criteria, ""),
    ];
    for (heading, items, quote) in lists {
        if items.is_empty() {
            continue;
        }
        facts.push_str(&format!("\n{heading}:\n"));
        for item in items {
            facts.push_str(&format!("- {quote}{item}{quote}\n"));
        }
    }
    match found.open.is_empty() {
        true => facts.push_str("\nThere are no open questions.\n"),
        false => {
            facts.push_str("\nQuestions only the author can answer, each with the answer the research recommends:\n");
            for (i, q) in found.open.iter().enumerate() {
                let n = i + 1;
                let (question, recommended) = (q.question.trim(), q.recommended.trim());
                match recommended.is_empty() {
                    true => facts.push_str(&format!("{n}. {question}\n")),
                    false => {
                        facts.push_str(&format!("{n}. {question} (recommended: {recommended})\n"))
                    }
                }
            }
        }
    }
    facts.push_str("\nThe maintainer triages it next.\n");
    facts
}
