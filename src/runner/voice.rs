//! What kelpie writes on GitHub for the maintainer, through its App
//!
//! Where an App covers the project's repo, a ruling, a notice and each
//! review round of kelpie's own reviewers are comments on the work item's
//! pull request, or on its issue while it has none. A ruling mentions the
//! maintainer, so GitHub notifies them, and ends with how to answer it. A
//! ruling the project manager, the terminal or a withdrawal settles has its
//! comment edited to say so (see [`settled_comment`]). A
//! failed post is told once in the runner's log and never fails the step it
//! belongs to. Where no App covers the repo nothing here posts, and the
//! runner writes as it did before.

use self::card::ruling_comment;
use super::Runner;
use super::gate::short;
use super::ruling::stuck_comment;
use crate::issues::make_missing_with;
use crate::ports::{Finding, ForgeError, NewLabel, Severity};
use crate::state::{Heard, Notice, ProjectState, Ruling, RulingKind};

/// Where a ruling's posted comment is kept in the state
pub(super) enum Placed {
    /// It mentions a login, whose reply in the thread answers it
    Heard(Heard),
    /// It mentions no one: the id of the comment
    Unmentioned(u64),
}

impl Placed {
    /// Keeps it in `state` for ruling `id`
    pub(super) fn keep(self, state: &mut ProjectState, id: u64) {
        match self {
            Self::Heard(heard) => state.heard.insert(id, heard).map(drop),
            Self::Unmentioned(comment) => state.unmentioned.insert(id, comment).map(drop),
        };
    }
}

impl Runner {
    /// Whether kelpie's App covers the project's repo
    pub(super) fn app_covers(&self) -> bool {
        self.ports.voice.covers(&self.remote)
    }

    /// The login a ruling mentions: `git.maintainer`, else the login `gh`
    /// acts as, asked once a run and kept
    ///
    /// # Errors
    ///
    /// [`ForgeError`] when the setting is absent and `gh` could not say who
    /// it acts as: who to mention is not known.
    pub(super) fn maintainer(&self) -> Result<String, ForgeError> {
        if let Some(login) = &self.settings.git.maintainer {
            return Ok(login.as_str().to_owned());
        }
        // An answer that failed is asked again at the next ruling.
        if let Some(login) = self.gh_login.get() {
            return Ok(login.clone());
        }
        let login = self.ports.forge.viewer()?;
        let _ = self.gh_login.set(login.clone());
        Ok(login)
    }

    /// Posts ruling `id`'s comment on `thread` as the App, and returns why it
    /// failed, or went up without a mention of the maintainer, if it did,
    /// and where its comment is kept
    pub(super) fn post_app_ruling(
        &self,
        thread: u64,
        ruling: &Ruling,
    ) -> (Option<String>, Option<Placed>) {
        let id = ruling.id;
        // A ruling still goes up when no one can be named to mention, as the
        // record on the thread, but it notifies no one and only the terminal
        // answers it, so that is told.
        let (mention, unasked) = match self.maintainer() {
            Ok(login) => (Some(login), None),
            Err(e) => (None, Some(e)),
        };
        let body = ruling_comment(mention.as_deref(), ruling);
        let posted = match self.ports.voice.post(&self.remote, thread, &body) {
            Err(e) => return (Some(e.to_string()), None),
            Ok(posted) => posted,
        };
        // Its answer is read from the comment's own time and id, the forge's.
        let placed = Some(match mention {
            Some(mentions) => Placed::Heard(Heard {
                mentions,
                time: posted.created,
                comment: posted.id,
                ruling_comment: posted.id,
            }),
            // No one is read for an answer, but the comment is still edited
            // when the ruling is settled.
            None => Placed::Unmentioned(posted.id),
        });
        let told = unasked.map(|why| {
            // Said in the log once a run, and only for a ruling that went up.
            if !self.unmentioned_told.replace(true) {
                self.notes.push(format!(
                    "ruling {id} is posted on #{thread} without a mention, so it notifies \
                     no one and only `shep kelpie rule` answers it: {why}; set \
                     `git.maintainer` to your GitHub login"
                ));
            }
            format!(
                "posted without mentioning the maintainer, since the login `gh` acts as \
                 could not be read: {why}. Only `shep kelpie rule` answers it"
            )
        });
        (told, placed)
    }

    /// Posts `body` on issue or pull request `number` as the login `gh` acts
    /// as, and keeps the comment's id in the state file at once: a comment
    /// kelpie wrote is never an answer to a ruling, though it is the
    /// maintainer's own login's, and a crash must not forget it. The window is
    /// the forge having the comment and the file not yet having its id, a
    /// crash between the two or a write that fails, which is told in the log.
    pub(super) fn comment_as_gh(&self, number: u64, body: &str) -> Result<(), ForgeError> {
        let id = self.ports.forge.comment(&self.remote, number, body)?;
        self.posted.borrow_mut().push(id);
        let mut kept = self.state.clone();
        kept.note_posted(&self.posted.borrow());
        if let Err(e) = self.store.save(&kept) {
            self.notes.push(format!(
                "cannot save the id of comment {id} on #{number}: {e}"
            ));
        }
        Ok(())
    }

    /// Makes each of `wanted` the repo lacks, as the App where one covers it
    pub(super) fn make_missing_labels<'a>(
        &self,
        wanted: impl IntoIterator<Item = NewLabel<'a>>,
    ) -> Result<(), String> {
        let (forge, repo) = (self.ports.forge.as_ref(), &self.remote);
        let app = self.app_covers();
        make_missing_with(forge, repo, wanted, |label| match app {
            true => self.ports.voice.create_label(repo, label),
            false => forge.create_label(repo, label),
        })
    }

    /// Opens an issue, as the App where one covers the repo
    pub(super) fn open_issue(
        &self,
        title: &str,
        body: &str,
        labels: &[&str],
    ) -> Result<u64, ForgeError> {
        match self.app_covers() {
            true => (self.ports.voice).create_issue(&self.remote, title, body, labels),
            false => (self.ports.forge).create_issue(&self.remote, title, body, labels),
        }
    }

    /// Comments on issue `number`, as the App where one covers the repo
    pub(super) fn comment_on_issue(&self, number: u64, body: &str) -> Result<(), ForgeError> {
        match self.app_covers() {
            true => self.ports.voice.comment(&self.remote, number, body),
            false => self.comment_as_gh(number, body),
        }
    }

    /// Adds `label` to issue `number`, or takes it off, as the App where one
    /// covers the repo
    pub(super) fn label_issue(&self, number: u64, label: &str, on: bool) -> Result<(), ForgeError> {
        match self.app_covers() {
            true => (self.ports.voice).set_issue_label(&self.remote, number, label, on),
            false => (self.ports.forge).set_issue_label(&self.remote, number, label, on),
        }
    }

    /// Posts `text` on `thread` as the App, where one covers the repo, and
    /// notes why it could not, once
    pub(super) fn notice_on(&mut self, thread: u64, text: &str) {
        if !self.app_covers() {
            return;
        }
        if let Err(e) = self.ports.voice.comment(&self.remote, thread, text) {
            self.notes
                .push(format!("cannot post a notice on #{thread}: {e}"));
        }
    }

    /// Posts a review round that ended on the pull request, where an App covers the repo
    pub(super) fn post_round(&mut self, round: &Round<'_>) {
        if !self.app_covers() {
            return;
        }
        let (number, text) = (round.number, round_comment(round));
        if let Err(e) = self.ports.voice.comment(&self.remote, number, &text) {
            self.notes.push(format!(
                "cannot post {}'s round {} on #{number}: {e}",
                round.reviewer, round.round
            ));
        }
    }
}

/// A review round of kelpie's own reviewer that ended, to post
pub(super) struct Round<'a> {
    /// The pull request it read
    pub(super) number: u64,
    /// Who read it
    pub(super) reviewer: &'a str,
    /// The round
    pub(super) round: u32,
    /// The head it read, when kelpie knows
    pub(super) head: Option<&'a str>,
    /// What it found
    pub(super) findings: &'a [Finding],
}

fn round_comment(round: &Round<'_>) -> String {
    let Round {
        reviewer,
        round: number,
        head,
        findings,
        ..
    } = round;
    let at = head.map_or_else(String::new, |head| format!(" at {}", short(head)));
    let read = format!("{reviewer} read this pull request{at} in review round {number}");
    if findings.is_empty() {
        return format!("{read} and found nothing.");
    }
    let count = match findings.len() {
        1 => "1 thing".to_owned(),
        n => format!("{n} things"),
    };
    let lines: Vec<String> = findings.iter().map(finding_line).collect();
    format!("{read} and found {count}:\n\n{}", lines.join("\n"))
}

fn finding_line(finding: &Finding) -> String {
    let severity = match finding.severity {
        Severity::High => "HIGH",
        Severity::Medium => "MEDIUM",
        Severity::Low => "LOW",
    };
    let place = match finding.line {
        0 => finding.file.clone(),
        line => format!("{}:{line}", finding.file),
    };
    let (what, why) = (finding.what.trim(), finding.why.trim());
    let what = what.strip_suffix('.').unwrap_or(what);
    format!("- {severity} `{place}`: {what}. {why}")
}

/// The text of an automatic merge's notice
pub(super) fn merged_notice(notice: &Notice) -> String {
    let Notice {
        issue,
        pull_request,
        head,
    } = notice;
    format!(
        "Pull request #{pull_request} for issue #{issue} merged into main at {}, every gate \
         passed. Nothing to answer.",
        short(head)
    )
}

/// What a merge ruling warns of, in words fit for any reader of its thread
///
/// Nothing for any other kind of ruling.
pub(super) fn merge_notes(kind: &RulingKind) -> Vec<&'static str> {
    let RulingKind::Merge {
        unreviewed,
        open_threads,
        unread_head,
        note_fix,
        late_fix,
        nits,
        ..
    } = kind
    else {
        return Vec::new();
    };
    let flags = [
        (
            unreviewed.is_some(),
            "No reviewer read it in its last review.",
        ),
        (
            open_threads.is_some(),
            "Review bot threads are still open on it.",
        ),
        (
            *unread_head,
            "No review round is on record as reading this head.",
        ),
        (
            *note_fix,
            "This head is the worker's fix for your note, and no reviewer read it.",
        ),
        (
            *late_fix,
            "This head is the worker's fix for a review bot's late review, and no \
             reviewer read it.",
        ),
        (
            nits.is_some(),
            "Review bots left nits on it, which never hold a merge.",
        ),
    ];
    (flags.into_iter())
        .filter_map(|(set, line)| set.then_some(line))
        .collect()
}

/// What a ruling says to a reader of its thread
///
/// Nothing the forge or a reviewer said, which is not written for strangers.
/// Under `gh` the merge and follow-up rulings post no comment; through the
/// App every kind does, since the thread is where the maintainer answers.
pub(super) fn said(kind: &RulingKind) -> String {
    match kind {
        RulingKind::Merge { head, .. } => {
            let mut said = format!("Merge this pull request at {} into main?", short(head));
            for line in merge_notes(kind) {
                said.push(' ');
                said.push_str(line);
            }
            said
        }
        RulingKind::Stuck(reason) => stuck_comment(reason),
        RulingKind::Question { asked, .. } => asked.clone(),
        RulingKind::AgentFiles { files, .. } => format!(
            "This pull request changes agents' own files: {}.",
            files.join(", ")
        ),
        RulingKind::ForeignChange { description, .. } => {
            format!("This pull request was changed: {description}.")
        }
        RulingKind::FollowUp { findings, refused } => {
            let held = match findings.len() {
                1 => "1 confirmed finding".to_owned(),
                n => format!("{n} confirmed findings"),
            };
            match refused {
                Some(_) => format!(
                    "This pull request merged with {held} left unfixed, and the forge would \
                     not take them as issues. Try again?"
                ),
                None => {
                    format!(
                        "This pull request merged with {held} left unfixed. File them as issues?"
                    )
                }
            }
        }
    }
}

mod card;

pub(super) use self::card::{settled_comment, thumbs, ticked};

#[cfg(test)]
mod tests;
