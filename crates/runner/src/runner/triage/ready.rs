//! The maintainer's own issue the pass rewrites, made agent-ready when the
//! issue check passes it
//!
//! The rewrite runs for an issue whose author is [`Runner::maintainer`],
//! ignoring case, with no open question and a `body`. Where the check passes the
//! issue as the pass leaves it, the body goes over the issue's, then the labels,
//! `ready-for-agent` last; where it does not, it goes under the issue's text and
//! the issue ends `needs-triage`. The
//! comment is worded first, so it promises nothing for a body the local guard
//! would refuse or while a hand-put triage label stands.

use super::super::Runner;
use super::super::contributions::post::{forge_is_busy, refused_text};
use super::TRIAGE_LABELS;
use super::research::Found;
use crate::board::has_criteria;
use crate::board::{READY, TRIAGE};
use crate::local_paths::{Leak, Surface};
use crate::ports::{ForgeError, NewLabel};
use crate::state::{StateError, TriageStep};

/// Made on a repo that lacks it before the first issue that passes the issue
/// check is made ready
pub(super) const READY_LABEL: NewLabel<'static> = NewLabel {
    name: READY,
    color: "0e8a16",
    description: "The issue passes kelpie's issue check; its board may take it",
};

// Whether `body` is an agent-ready issue body: it names `Touches:` and has
// acceptance criteria, as the issue writer files them
pub(super) fn is_issue_body(body: &str) -> bool {
    has_criteria(body)
        && body
            .lines()
            .map(str::trim)
            .any(|line| line.starts_with("Touches:"))
}

/// The body kelpie writes over `issue`'s, or why it will not write one
pub(in crate::runner) enum Ready {
    /// The body to write over the issue's, which the check passes
    Body(String),
    /// The body to write under the issue's text, which the check still refuses
    Rewrite(String),
    /// Not this issue: an outside author, a reply with an open question or no
    /// body, or no maintainer to ask for
    Not,
    /// The maintainer's own issue, but the body names something of this
    /// machine, so kelpie will not write it
    Local(Leak),
}

/// Whether kelpie is going to rewrite `issue`, with `labels` the pass puts on,
/// and whether that makes it ready. Only an issue whose author is
/// `Runner::maintainer()`, ignoring case, is ever rewritten or made
/// ready. The body is read against the same local guard every App post goes
/// through: the comment is worded before the body is written, so text the
/// guard would refuse is dropped here rather than promised there.
pub(in crate::runner) fn readiness(
    runner: &Runner,
    issue: u64,
    author: &str,
    found: &Found,
    labels: &[String],
) -> Ready {
    let Some(body) = found.body.clone() else {
        return Ready::Not;
    };
    if !found.open.is_empty() {
        return Ready::Not;
    }
    let Ok(maintainer) = runner.maintainer() else {
        return Ready::Not;
    };
    if !author.eq_ignore_ascii_case(&maintainer) {
        return Ready::Not;
    }
    match runner.local.find(&body, Surface::Prose) {
        Some(leak) => Ready::Local(leak),
        None if runner.passes_with(issue, &body, labels) => Ready::Body(body),
        None => Ready::Rewrite(body),
    }
}

/// What writing the owed body came to
pub(super) enum ReadyWrite {
    /// The body went on and its step is saved
    Written,
    /// A publish refusal dropped the body, so the issue ends `needs-triage`
    /// like any other, and the issue is told the rewrite did not happen
    Refused,
    /// Nothing more this pass: the maintainer's own label stood, or the write
    /// failed and is tried at the next poll
    Held,
}

/// What putting the ready path's labels came to
pub(super) enum ReadyLabels {
    /// They all went on, `ready-for-agent` last
    On,
    /// A triage label the maintainer put on by hand stands, and the record is
    /// saved `done`
    Stood,
    /// A label failed, and the step is tried at the next poll
    Held,
}

impl Runner {
    // Writes the owed body over `issue`'s and saves its step, unless a triage
    // label the maintainer put on by hand since the pass began stands, which
    // leaves the record with no further label and no body write.
    pub(super) fn ready_write(&mut self, issue: u64, body: &str) -> Result<ReadyWrite, StateError> {
        match self.triage_labelled_by_hand(issue) {
            Ok(Some(label)) => {
                self.notes.push(format!(
                    "triage #{issue}: `{label}` went on while its triage was still running, so \
                     that stands: kelpie labels it no further and writes no new body"
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
                self.triage_body_written(issue, body)?;
                self.triage_stepped(issue, TriageStep::BodyWritten)?;
                Ok(ReadyWrite::Written)
            }
            Err(e) if refused_text(&e) => {
                self.notes.push(format!(
                    "triage #{issue}: its agent-ready body was refused: {e}; it is not \
                     written, and the issue ends `{TRIAGE}"
                ));
                self.triage_ready_dropped(issue)?;
                // The comment said the body would be rewritten. Say on the
                // issue that it was not, rather than leave that promise there
                // unkept.
                self.notice_on(
                    issue,
                    "Kelpie did not rewrite this issue's body: the rewrite it was given was \
                     refused. The body stays as its author wrote it, and the maintainer triages \
                     it next.",
                );
                Ok(ReadyWrite::Refused)
            }
            Err(e) => {
                let why = format!("cannot write its new body: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                Ok(ReadyWrite::Held)
            }
        }
    }

    // Every label `issue` carries now
    pub(super) fn labels_now(&self, issue: u64) -> Result<Vec<String>, ForgeError> {
        let found = self
            .ports
            .tickets
            .issue(&self.remote, super::ticket(issue)?)?;
        Ok(found.labels)
    }

    // The triage labels `issue` carries now
    pub(super) fn triage_labels_now(&self, issue: u64) -> Result<Vec<String>, ForgeError> {
        let carried = self.labels_now(issue)?;
        Ok(carried
            .into_iter()
            .filter(|l| TRIAGE_LABELS.contains(&l.as_str()))
            .collect())
    }

    // The labels `issue`'s pass began with: the reading taken when its call
    // started, or the one its record saved, for a pass that resumes after a
    // restart with no in-memory reading left.
    pub(super) fn triage_seen(&self, issue: u64) -> Vec<String> {
        (self.triage_tries.get(&issue))
            .and_then(|t| t.seen.clone())
            .or_else(|| self.triage_record(issue).map(|t| t.seen.clone()))
            .unwrap_or_default()
    }

    // The triage label `issue` carries now that it did not when the pass began,
    // or `None`. Kelpie's own labels go on after this check, so a label that came
    // by since is the maintainer's and stands. A failed reading is the caller's
    // to count as a failure: kelpie will not label over labels it cannot see.
    pub(super) fn triage_labelled_by_hand(&self, issue: u64) -> Result<Option<String>, ForgeError> {
        Ok(stood(
            &self.triage_labels_now(issue)?,
            &self.triage_seen(issue),
        ))
    }

    // The same, leaving `own` out of what the issue carries: the label the
    // caller is about to write may already be on from a call whose answer was
    // lost, and that one is kelpie's.
    pub(super) fn triage_labelled_by_hand_but(
        &self,
        issue: u64,
        own: &str,
    ) -> Result<Option<String>, ForgeError> {
        let carried: Vec<String> = (self.triage_labels_now(issue)?.into_iter())
            .filter(|l| l != own)
            .collect();
        Ok(stood(&carried, &self.triage_seen(issue)))
    }

    // Saves the record with the owed ready body dropped: a publish refusal
    // took it, so the issue ends `needs-triage` like any other
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

    // Puts on the labels the reply gave and then `ready-for-agent`, last, taking
    // off the `needs-triage` an earlier pass left. A triage label put on by hand
    // since the pass began is read for here too: kelpie labels nothing over it,
    // whether it came after the body went on or while the call ran.
    pub(super) fn label_ready(&mut self, issue: u64) -> Result<ReadyLabels, StateError> {
        let seen = self.triage_seen(issue);
        let carried = match self.triage_labels_now(issue) {
            Ok(carried) => carried,
            Err(e) => {
                let why = format!("cannot read its labels: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                return Ok(ReadyLabels::Held);
            }
        };
        // `ready-for-agent` is this step's own label, so on the issue here it is
        // kelpie's: a lost answer or a crash before the step saved leaves it there, and
        // the pass finishes the labelling rather than reading it as the maintainer's.
        // Any other triage label here came by hand and stands.
        let owed: Vec<String> = carried
            .iter()
            .filter(|l| l.as_str() != READY)
            .cloned()
            .collect();
        if let Some(label) = stood(&owed, &seen) {
            self.notes.push(format!(
                "triage #{issue}: `{label}` went on while its triage waited to be labelled, so \
                 that stands: kelpie labels it no further"
            ));
            self.triage_stepped(issue, TriageStep::Done)?;
            return Ok(ReadyLabels::Stood);
        }
        let labels = self
            .triage_record(issue)
            .map_or_else(Vec::new, |t| t.labels.clone());
        if !self.put_pass_labels(issue, &labels) {
            return Ok(ReadyLabels::Held);
        }
        // An issue is not both ready for the board and waiting for the
        // maintainer, so the `needs-triage` of an earlier pass comes off before
        // `ready-for-agent` goes on. The reading above already ruled out a label
        // put on by hand, so this one is kelpie's own.
        if carried.iter().any(|l| l == TRIAGE)
            && let Err(e) = self.label_issue(issue, TRIAGE, false)
        {
            let why = format!("cannot take `{TRIAGE}` off: {e}");
            self.triage_failed(issue, &why, !forge_is_busy(&e));
            return Ok(ReadyLabels::Held);
        }
        if let Err(why) = self.make_missing_labels([READY_LABEL]) {
            self.triage_failed(issue, &format!("cannot make `{READY}`: {why}"), true);
            return Ok(ReadyLabels::Held);
        }
        if let Err(e) = self.label_issue(issue, READY, true) {
            let why = format!("cannot label it `{READY}`: {e}");
            self.triage_failed(issue, &why, !forge_is_busy(&e));
            return Ok(ReadyLabels::Held);
        }
        Ok(ReadyLabels::On)
    }
}

// The triage label among `carried` that `seen` does not hold: one that came by
// after the pass began, which is the maintainer's and stands
fn stood(carried: &[String], seen: &[String]) -> Option<String> {
    (carried.iter())
        .find(|l| !seen.iter().any(|s| s.as_str() == l.as_str()))
        .cloned()
}
