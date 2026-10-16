//! The maintainer's own issue the pass is sure of becomes agent-ready
//!
//! Only the maintainer makes an outside issue ready, so the ready path runs
//! for an issue whose author is [`Runner::maintainer`], ignoring case. Sure
//! means the reply has a type and a priority, asks no open question, and
//! carries a `body` [`crate::issues::has_criteria`] accepts that names
//! `Touches:`. The comment says the issue is ready, the body goes over the
//! issue's through [`Voice::edit_issue_body`](crate::ports::Voice), and the
//! type and priority labels come before `ready-for-agent`, which goes on
//! last. A triage label the maintainer put on by hand while the call ran
//! stands: kelpie labels no further and writes no new body.

use super::super::Runner;
use super::super::contributions::post::{forge_is_busy, refused_text};
use super::TRIAGE_LABELS;
use super::research::Found;
use crate::board::{READY, TRIAGE};
use crate::issues::has_criteria;
use crate::ports::{ForgeError, NewLabel};
use crate::state::{StateError, TriageStep};

/// Made on a repo that lacks it before the first issue kelpie is sure of is
/// made ready
const READY_LABEL: NewLabel<'static> = NewLabel {
    name: READY,
    color: "0e8a16",
    description: "Kelpie researched this issue and is sure of it; its board may take it",
};

// Whether `body` is an agent-ready issue body: it names `Touches:` and has
// acceptance criteria, as the issue writer files them
fn is_issue_body(body: &str) -> bool {
    has_criteria(body)
        && body
            .lines()
            .map(str::trim)
            .any(|line| line.starts_with("Touches:"))
}

// Whether the pass is sure of everything: a type, a priority, no open
// question, and an agent-ready body
fn sure(found: &Found) -> bool {
    found.kind.is_some()
        && found.priority.is_some()
        && found.open.is_empty()
        && found.body.as_deref().is_some_and(is_issue_body)
}

/// The agent-ready body kelpie writes over `issue`'s, or `None`: when the
/// reply is sure of everything and `author` is the maintainer, ignoring case,
/// whose issues only the maintainer may make ready. A maintainer that cannot
/// be named makes no issue ready.
pub(in crate::runner) fn body_for(runner: &Runner, author: &str, found: &Found) -> Option<String> {
    let body = found.body.clone()?;
    if !sure(found) {
        return None;
    }
    let maintainer = runner.maintainer().ok()?;
    author.eq_ignore_ascii_case(&maintainer).then_some(body)
}

/// What writing the owed body came to
pub(super) enum ReadyWrite {
    /// The body went on and its step is saved
    Written,
    /// The guard refused the text, so the body is dropped and the issue ends
    /// `needs-triage` like any other
    Refused,
    /// Nothing more this pass: the maintainer's own label stood, or the write
    /// failed and is tried at the next poll
    Held,
}

impl Runner {
    // Writes the owed body over `issue`'s and saves its step, unless the
    // maintainer labelled the issue by hand while the call ran, which stands
    // with no further label and no body write.
    pub(super) fn ready_write(&mut self, issue: u64, body: &str) -> Result<ReadyWrite, StateError> {
        match self.triage_labelled_by_hand(issue) {
            Ok(Some(label)) => {
                self.notes.push(format!(
                    "triage #{issue}: `{label}` went on while the triager ran, so that stands: \
                     kelpie labels it no further and writes no new body"
                ));
                self.triage_stepped(issue, TriageStep::Done)?;
                self.triage_tries.remove(&issue);
                return Ok(ReadyWrite::Held);
            }
            Ok(None) => {}
            Err(e) => {
                let why = format!("cannot read its labels: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                return Ok(ReadyWrite::Held);
            }
        }
        match self.ports.voice.edit_issue_body(&self.remote, issue, body) {
            Ok(()) => {
                self.triage_stepped(issue, TriageStep::BodyWritten)?;
                Ok(ReadyWrite::Written)
            }
            Err(e) if refused_text(&e) => {
                self.notes.push(format!(
                    "triage #{issue}: its agent-ready body was refused: {e}; it is not \
                     written, and the issue ends `{TRIAGE}"
                ));
                self.triage_ready_dropped(issue)?;
                Ok(ReadyWrite::Refused)
            }
            Err(e) => {
                let why = format!("cannot write its new body: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                Ok(ReadyWrite::Held)
            }
        }
    }

    // The triage label `issue` carries now that it did not when the pass
    // began, or `None` when it carries none. Kelpie's own labels for the
    // issue go on after this check, so a label that came by since is the
    // maintainer's, and it stands. A reading that fails is the caller's to
    // count as a failure: kelpie will not label over labels it cannot see.
    pub(super) fn triage_labelled_by_hand(&self, issue: u64) -> Result<Option<String>, ForgeError> {
        let seen = self
            .triage_tries
            .get(&issue)
            .and_then(|t| t.seen.clone())
            .unwrap_or_default();
        let found = self.ports.forge.issue(&self.remote, issue)?;
        Ok((found.labels.iter())
            .find(|l| TRIAGE_LABELS.contains(&l.as_str()) && !seen.iter().any(|s| s == l.as_str()))
            .cloned())
    }

    // Saves the record with the owed ready body dropped: the guard refused
    // the text, so the issue ends `needs-triage` like any other
    pub(super) fn triage_ready_dropped(&mut self, issue: u64) -> Result<(), StateError> {
        let mut next = self.state.clone();
        if let Some(held) = next
            .triage
            .as_mut()
            .and_then(|t| t.issues.iter_mut().find(|t| t.issue == issue))
        {
            held.ready_body = None;
        }
        self.save(next)
    }

    // Puts on the labels the reply gave and then `ready-for-agent`, the last
    // label the ready path writes. Whether they all went on.
    pub(super) fn label_ready(&mut self, issue: u64) -> bool {
        let labels = self
            .triage_record(issue)
            .map_or_else(Vec::new, |t| t.labels.clone());
        for label in labels {
            if let Err(e) = self.label_issue(issue, &label, true) {
                let why = format!("cannot label it `{label}`: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                return false;
            }
        }
        if let Err(why) = self.make_missing_labels([READY_LABEL]) {
            self.triage_failed(issue, &format!("cannot make `{READY}`: {why}"), true);
            return false;
        }
        if let Err(e) = self.label_issue(issue, READY, true) {
            let why = format!("cannot label it `{READY}`: {e}");
            self.triage_failed(issue, &why, !forge_is_busy(&e));
            return false;
        }
        true
    }
}
