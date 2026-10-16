//! The author's own session: the prompt kelpie posts, and the body it writes back
//!
//! An author who replies `local` settles the open questions with an agent in
//! their own terminal. Kelpie posts one comment: the representative's lead-in
//! and a code-built block holding the prompt to paste, the files the triage
//! comment named and the questions it asked. Their session writes what it
//! settles back into the issue body, and the body kelpie last saw is what the
//! wait is measured against: the record keeps `local { at, body }`, `body` a
//! hash of that text.
//!
//! Each poll reads each issue waiting on `local` through [`Forge::issue`]
//! (`crate::ports::Forge`). A body whose hash differs is triaged again on the
//! new text, by a call of [`Purpose::LocalRead`] taken the way the answers
//! call is, and its comment ends the round: `ready-for-agent` for a maintainer
//! issue the pass is sure of, `needs-triage` for any other, with no further ask
//! whatever is still open. The wait stays open until that comment lands, so a
//! restart between the edit and its call reads the body again and starts the
//! call again. No edit `triage.wait_days` after the prompt ends as `none` does.
//! Kelpie's own body edits set the hash, so a write of kelpie's own is never
//! read as the author's.
//!
//! The prompt comment's last line carries the hash of the body it went up
//! against, so a prompt that posted before a crash leaves the round measured
//! against that body rather than whatever the forge holds when kelpie comes
//! back. A prompt kelpie finds on the issue is taken up before a pass that
//! finds no question left is ended as `none`. The hash is taken from a read
//! made just before the post, since the wording call before it can run for
//! minutes. Everything the block names, the title, the files and each question
//! with what it recommends, is put on one line with its backticks gone, so
//! none of it can close the fence the prompt is posted in.

use super::super::Runner;
use super::super::contributions::post::forge_is_busy;
use super::super::report::Begin;
use super::super::representative::IssuePost;
use super::ask::Purpose;
use super::research::Question;
use crate::state::{StateError, TriageLocal};

/// What begins the last line of kelpie's local prompt comment, which also
/// carries the hash of the body the prompt was posted against
pub(in crate::runner) const LOCAL_MARKER: &str = "<!-- kelpie:triage local";

/// The skill the prompt starts with
const SKILL: &str = "grill-with-docs";

/// Where an author without the skill can get it, said beside the prompt
const SKILL_WHERE: &str = "The `grill-with-docs` skill is from mattpocock/skills. Kelpie's own \
    copy of it is in `shep-pm/shep-kelpie` at `skills/skills/engineering/grill-with-docs`.";

/// What kind of post the representative is asked for beside the prompt an
/// author runs in their own terminal. The prompt itself is code's and goes
/// under the lead-in.
const LOCAL_KIND: &str = "Write the short lead-in to kelpie's prompt for the author of this \
    issue, who chose to settle its open questions in their own terminal. Greet them, say kelpie \
    gives them one prompt to paste into an agent session of their own, and say kelpie reads the \
    issue again once their session has rewritten its body and leaves what is still open to the \
    maintainer. Do not restate or rewrite the prompt: kelpie posts it under your words. It is a \
    comment, not a review: no verdict, and nothing promised.";

/// What kind of post the representative is asked for on the pass that reads the
/// issue again after its author's own session rewrote its body
pub(in crate::runner) const LOCAL_READ_KIND: &str = "Write the triage comment on this issue, which kelpie read again \
    after its author's own session rewrote its body from kelpie's prompt. Greet the author, say \
    what kelpie now found, and ask only the questions still open. This is the last round: what \
    is still open goes to the maintainer. It is a comment, not a review: no verdict, and nothing \
    promised.";

impl Runner {
    // Posts the one prompt kelpie gives an author who settles the questions in
    // their own terminal: the numbered files and questions under a lead-in the
    // representative words. The body it posts the hash of is the one the forge
    // holds now, which is what the wait is measured against, and the comment
    // itself carries that hash.
    pub(in crate::runner) fn post_local_prompt(
        &mut self,
        issue: u64,
        said: &str,
    ) -> Result<(), StateError> {
        let Some(record) = self.triage_record(issue).cloned() else {
            return Ok(());
        };
        let Some(asked) = record.asked else {
            return Ok(());
        };
        let questions = self
            .triage_tries
            .get(&issue)
            .map(|t| t.open.clone())
            .unwrap_or_default();
        let files = self
            .triage_tries
            .get(&issue)
            .map(|t| t.files.clone())
            .unwrap_or_default();
        let author = self
            .triage_tries
            .get(&issue)
            .and_then(|t| t.author.clone())
            .unwrap_or_default();
        let found = match self.ports.forge.issue(&self.remote, issue) {
            Ok(found) => found,
            Err(e) => {
                let why = format!("cannot read its body: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                return Ok(());
            }
        };
        // The forge may take a post whose answer is lost, and a crash may leave
        // the wait unsaved, so the prompt kelpie already posted is taken up
        // first: what its author did after it is read from the body, and a pass
        // that now finds no question left is the round's answer rather than a
        // reason to end it as `none`. The comment it finds carries the body the
        // prompt went up against, so an edit made between that post and this
        // read is still the round's answer.
        match self.app_comment_after(issue, LOCAL_MARKER, asked.at) {
            Ok(Some(prompt)) => {
                let seen = prompt_hash(&prompt.body).unwrap_or_else(|| body_hash(&found.body));
                return self.set_local(issue, seen);
            }
            Ok(None) => {}
            Err(e) => {
                let why = format!("cannot read its comments: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                return Ok(());
            }
        }
        if questions.is_empty() {
            self.notes.push(format!(
                "triage #{issue}: its author replied `local`, but the pass left no question to \
                 settle, so the maintainer triages it as it stands"
            ));
            return self.triage_none(issue);
        }
        let facts = local_facts(questions.len());
        let kind = LOCAL_KIND;
        let earlier = [record.comment];
        let post = IssuePost {
            kind,
            facts: &facts,
            earlier: &earlier,
            author: &author,
            said,
        };
        let words = match self.represent_issue(issue, &post) {
            Ok(words) => words,
            Err(e) => {
                self.triage_failed(issue, &e.to_string(), true);
                return Ok(());
            }
        };
        // The wording call can take minutes, and the author's session can
        // rewrite the body while it runs. The wait is measured against the body
        // the prompt goes up against, so it is read again here rather than
        // hashed from the read made before the call.
        let found = match self.ports.forge.issue(&self.remote, issue) {
            Ok(found) => found,
            Err(e) => {
                let why = format!("cannot read its body: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                return Ok(());
            }
        };
        let seen = body_hash(&found.body);
        let block = local_block(
            self.remote.as_str(),
            issue,
            &found.title,
            &files,
            &questions,
        );
        let body = format!(
            "{words}\n\n{SKILL_WHERE}\n\n```text\n{block}\n```\n\n{LOCAL_MARKER} body:{seen:016x} -->"
        );
        match self.ports.voice.post(&self.remote, issue, &body) {
            Ok(_) => {
                if let Some(tries) = self.triage_tries.get_mut(&issue) {
                    tries.local_call = false;
                }
                self.set_local(issue, seen)
            }
            Err(e) => {
                let why = format!("cannot post the prompt: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                Ok(())
            }
        }
    }

    // Saves the record as waiting on the author's own session, with the body
    // kelpie last saw. The ask comes off: the prompt replaces it, and the
    // prompt's own `wait_days` is what silence is measured from.
    pub(in crate::runner) fn set_local(&mut self, issue: u64, body: u64) -> Result<(), StateError> {
        let mut next = self.state.clone();
        if let Some(record) = next
            .triage
            .as_mut()
            .and_then(|t| t.issues.iter_mut().find(|t| t.issue == issue))
        {
            record.asked = None;
            record.local = Some(TriageLocal {
                at: self.ports.clock.now(),
                body,
            });
        }
        self.save(next)
    }

    // Reads each issue whose triage waits on a local session. Its body is what
    // the author's session was told to rewrite, so a body whose hash differs is
    // the round's answer: the triager reads the issue again. With no edit
    // `triage.wait_days` after the prompt, the wait ends as `none` does.
    pub(super) fn watch_local_waits(&mut self) -> Result<(), StateError> {
        for record in self.triage_waiting() {
            let (Some(local), issue) = (record.local, record.issue) else {
                continue;
            };
            // No read under a call in flight: its comment is what ends the wait,
            // and a body read between the call and its comment is the body that
            // call was made on.
            if self.flights.triaging() == Some(issue) || self.triage_given_up(issue) {
                continue;
            }
            let body = match self.ports.forge.issue(&self.remote, issue) {
                Ok(found) => found.body,
                Err(e) => {
                    let why = format!("cannot read its body: {e}");
                    self.triage_failed(issue, &why, !forge_is_busy(&e));
                    continue;
                }
            };
            if body_hash(&body) != local.body {
                let told = self.triage_tries.get(&issue).is_some_and(|t| t.local_edit);
                self.triage_tries.entry(issue).or_default().local_edit = true;
                if !told {
                    self.notes.push(format!(
                        "triage #{issue}: its body changed since kelpie's prompt went up, so the \
                         triager reads it again"
                    ));
                }
                continue;
            }
            if self.ports.clock.now().0 < local.at.0.saturating_add(self.wait_seconds()) {
                continue;
            }
            self.notes.push(format!(
                "triage #{issue}: its author wrote no new body within `triage.wait_days` of the \
                 prompt, which counts as `none`: the maintainer triages it as it stands"
            ));
            self.triage_none(issue)?;
        }
        Ok(())
    }

    // Keeps the hash a `local` wait is measured against at the body kelpie just
    // wrote, so a write of kelpie's own is never read as the author's session's
    // edit. A record with no such wait needs nothing: its next wait takes the
    // body the forge holds then.
    pub(in crate::runner) fn triage_body_written(
        &mut self,
        issue: u64,
        body: &str,
    ) -> Result<(), StateError> {
        if self.triage_record(issue).and_then(|t| t.local).is_none() {
            return Ok(());
        }
        let mut next = self.state.clone();
        if let Some(record) = next
            .triage
            .as_mut()
            .and_then(|t| t.issues.iter_mut().find(|t| t.issue == issue))
            && let Some(local) = record.local.as_mut()
        {
            local.body = body_hash(body);
        }
        self.save(next)
    }

    // The issues whose questions or prompt a restart lost, and whose body an
    // author's session rewrote, each owed a call that takes a slot as the first
    // one does.
    pub(super) fn triage_round_due(&mut self) -> Result<Option<Begin>, StateError> {
        for record in self.triage_waiting() {
            let issue = record.issue;
            if self.flights.triage_tried(issue) || self.triage_given_up(issue) {
                continue;
            }
            let tries = self.triage_tries.get(&issue);
            let answers = tries.is_some_and(|t| t.answers.is_some());
            let questions = record.questions.is_none() && tries.is_some_and(|t| t.questions_call);
            let prompt = record.asked.is_some() && tries.is_some_and(|t| t.local_call);
            let read = record.local.is_some() && tries.is_some_and(|t| t.local_edit);
            let purpose = match (answers, questions, prompt, read) {
                (true, ..) => Purpose::Answers,
                (false, true, ..) => Purpose::Questions,
                (false, false, true, _) => Purpose::LocalPrompt,
                (false, false, false, true) => Purpose::LocalRead,
                _ => continue,
            };
            self.flights.triage_try(issue);
            if let Some(job) = self.triage_job(issue, purpose) {
                return Ok(Some(Begin::Triage(job)));
            }
        }
        Ok(None)
    }
}

// The prompt an author pastes into their own session: the issue, how to read it
// and the files kelpie named, the open questions with what the research
// recommends, and the write-back that is how kelpie learns what they settled.
// Every word of it that came from an agent's reply is put on one line with its
// backticks gone, so none of it can close the fence the block is posted in or
// become a line of the prompt itself.
fn local_block(
    repo: &str,
    issue: u64,
    title: &str,
    files: &[String],
    questions: &[Question],
) -> String {
    let mut block = format!("/{SKILL} Issue {repo}#{issue}: {}.\n", one_line(title));
    block.push_str(&format!(
        "\nRead it with `gh issue view {issue} --comments`.\n"
    ));
    if !files.is_empty() {
        block.push_str("\nThen read the files kelpie's triage comment named:\n");
        for file in files {
            block.push_str(&format!("- `{}`\n", one_line(file)));
        }
    }
    block.push_str(
        "\nSettle these open questions, each with the answer kelpie's research recommends:\n",
    );
    for (i, q) in questions.iter().enumerate() {
        let asked = format!("{}. {}", i + 1, one_line(&q.question));
        match one_line(&q.recommended).as_str() {
            "" => block.push_str(&format!("{asked}\n")),
            recommended => block.push_str(&format!("{asked} (recommended: {recommended})\n")),
        }
    }
    block.push_str(
        "\nWhen we agree, rewrite this issue's body with acceptance criteria and a `Touches:` \
         list of the files the work touches, and comment on the issue with the decisions we \
         made.",
    );
    block
}

// The title, file, question or answer as one line that cannot close the fence
// the prompt is posted in, or stand as a line of the prompt itself
fn one_line(text: &str) -> String {
    (text.chars())
        .map(|c| match c {
            '`' => ' ',
            c if c.is_whitespace() => ' ',
            c => c,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// What the representative words the lead-in from: that the author chose their
// own terminal for this, and how many questions the prompt names. The prompt is
// not a fact to restate.
fn local_facts(most: usize) -> String {
    let many = match most == 1 {
        true => "",
        false => "s",
    };
    let mut facts = String::from(
        "Kelpie asked the author of this issue to settle its open questions, and they replied \
         `local`.\n",
    );
    facts.push_str(&format!(
        "\nKelpie posts one prompt below your words, for the author to paste into an agent \
         session of their own. It names this issue, the {most} open question{many} and the files \
         its triage comment named, and it ends by writing what they settle back into the issue.\n"
    ));
    facts.push_str(
        "\nKelpie reads the issue again when its body changes, and what is still open after \
         that goes to the maintainer.\n",
    );
    facts
}

// The hash of the body a prompt comment was posted against, read off its last
// line. A comment with no hash there is kelpie's marker written by someone
// else, and the caller takes the body the forge holds for it.
fn prompt_hash(comment: &str) -> Option<u64> {
    let hex = (comment.trim_end().lines().last()?)
        .strip_prefix(LOCAL_MARKER)?
        .trim()
        .strip_prefix("body:")?
        .strip_suffix(" -->")?;
    u64::from_str_radix(hex, 16).ok()
}

// FNV-1a over the body's bytes, trimmed. A saved hash is read again after a
// restart and on another machine, so it is a fixed function and not the
// standard library's, which is free to change between releases.
fn body_hash(body: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in body.trim().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
