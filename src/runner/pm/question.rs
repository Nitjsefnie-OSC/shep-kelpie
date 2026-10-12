//! Settling a worker's question or a follow-up ruling on the project manager's answer
//!
//! The item must be parked on a question ruling now. Its text then answers
//! the ruling as the maintainer's would, led by `PM answered:`, and its
//! escalation puts the ruling to the maintainer with the recommendation
//! added. Anything else is dropped, and the ruling goes out as it stands.
//!
//! An item parked on a follow-up ruling takes an escalation alone through
//! `answer`: its text is dropped, and so is an escalation whose choice is
//! neither `yes` nor `no`. Its own `follow_up` key decides the ruling: a yes
//! files the findings as the maintainer's yes does, a no drops them.

use std::io::Write as _;

use super::act::{WORDS, plain};
use super::answer::{Call, Decide, How, Settle};
use super::maintainer::Settled;
use crate::board::{READY, TRIAGE};
use crate::ports::{Finding, PM_NOTES};
use crate::runner::ruling::question;
use crate::runner::thread_replies::Settler;
use crate::runner::{Answer as Ruled, RuleError, Runner};
use crate::state::{RulingKind, StateError};
use crate::work_item::Phase;

/// How many settled questions `status` keeps
const KEPT: usize = 5;

/// What leads the text of an answer the project manager gave
const ANSWERED: &str = "PM answered:";

// What a work item is parked on
enum Asked {
    /// A question ruling not yet posted to the maintainer
    On(u64),
    /// A question ruling the maintainer was alerted of
    Alerted(u64),
    /// Nothing that asks a question
    Not,
}

impl Runner {
    /// Settles the question `settle` names, and says what it did, or why
    /// it was dropped
    ///
    /// # Errors
    ///
    /// [`StateError`] when what it did cannot be saved.
    pub(super) fn settle_question(
        &mut self,
        settle: &Settle,
    ) -> Result<Result<String, String>, StateError> {
        let n = settle.item;
        let id = match self.asked_on(n) {
            Asked::On(id) => id,
            Asked::Alerted(id) => {
                return Ok(Err(format!(
                    "answer #{n}: ruling {id} already went to the maintainer"
                )));
            }
            Asked::Not => {
                return Ok(Err(format!(
                    "answer #{n}: not a work item parked on a question or follow-up ruling"
                )));
            }
        };
        let why = plain(&self.shown(settle.why.clone()), WORDS);
        let (done, outcome, recommends) = match &settle.how {
            How::Text(_) if self.follow_up_ruling(id) => {
                return Ok(Err(format!(
                    "answer #{n}: ruling {id} is a follow-up, which the project manager \
                     decides with `follow_up` or passes on with a recommendation, not with text"
                )));
            }
            How::Text(text) => {
                let by = Settler::Pm {
                    answer: text.clone(),
                    why: why.clone(),
                    unread: false,
                };
                match self.rule_by(id, Ruled::Text(format!("{ANSWERED} {text}")), by) {
                    Ok(()) => {}
                    Err(RuleError::State(e)) => return Err(e),
                    Err(e) => return Ok(Err(format!("answer #{n}: {e}"))),
                }
                (format!("answered ruling {id} on #{n}"), "answered", None)
            }
            How::Escalate(choice)
                if self.follow_up_ruling(id) && !Self::yes_or_no(choice.as_deref()) =>
            {
                return Ok(Err(format!(
                    "answer #{n}: ruling {id} is a follow-up, whose recommendation is `yes` or \
                     `no`"
                )));
            }
            How::Escalate(choice) => {
                let choice = (choice.as_deref()).map(|c| plain(&self.shown(c.to_owned()), WORDS));
                self.recommend(id, choice.as_deref(), &why)?;
                (
                    format!("put ruling {id} on #{n} to the maintainer"),
                    "passed on",
                    choice,
                )
            }
        };
        let said = recommends
            .as_ref()
            .map_or_else(String::new, |c| format!(", recommending {c}"));
        let line = format!("ruling {id} on #{n}: the project manager {outcome} it{said}: {why}");
        let settled = Settled {
            ruling: id,
            issue: n,
            how: outcome,
            recommends,
            why,
        };
        self.keep_settled(settled, line)?;
        Ok(Ok(done))
    }

    /// Settles the follow-up ruling `decide` names: its findings are filed
    /// as the maintainer's yes would, or dropped as their no would. Says what
    /// it did, or why it was dropped.
    ///
    /// # Errors
    ///
    /// [`StateError`] when what it did cannot be saved.
    pub(super) fn settle_follow_up(
        &mut self,
        decide: &Decide,
    ) -> Result<Result<String, String>, StateError> {
        let n = decide.item;
        let id = match self.asked_on(n) {
            Asked::On(id) if self.follow_up_ruling(id) => id,
            Asked::On(id) => {
                return Ok(Err(format!(
                    "follow_up #{n}: ruling {id} is a worker's question, not a follow-up"
                )));
            }
            Asked::Alerted(id) => {
                return Ok(Err(format!(
                    "follow_up #{n}: ruling {id} already went to the maintainer"
                )));
            }
            Asked::Not => {
                return Ok(Err(format!(
                    "follow_up #{n}: not a work item parked on a follow-up ruling"
                )));
            }
        };
        if self.refused_ruling(id) {
            return Ok(Err(format!(
                "follow_up #{n}: ruling {id} asks whether to try the forge again, which is \
                 the maintainer's to decide"
            )));
        }
        let why = plain(&self.shown(decide.why.clone()), WORDS);
        let found = self.findings_of(id);
        let count = match found.len() {
            1 => "1 finding".to_owned(),
            many => format!("{many} findings"),
        };
        let at = plain(&self.shown(found.join(", ")), WORDS);
        let (answer, verb, label) = match decide.call {
            Call::File { unread: false } => (Ruled::Yes, "filing", Some(READY)),
            Call::File { unread: true } => (Ruled::Yes, "filing", Some(TRIAGE)),
            Call::Drop => (Ruled::No(why.clone()), "dropping", None),
        };
        let as_label = label.map_or_else(String::new, |l| format!(" as {l}"));
        let by = Settler::Pm {
            answer: format!(
                "{}, {verb} {count}{as_label}: {at}",
                if label.is_some() { "yes" } else { "no" }
            ),
            why: why.clone(),
            unread: decide.call == Call::File { unread: true },
        };
        match self.rule_by(id, answer, by) {
            Ok(()) => {}
            Err(RuleError::State(e)) => return Err(e),
            Err(e) => return Ok(Err(format!("follow_up #{n}: {e}"))),
        }
        let how = if label.is_some() { "filed" } else { "dropped" };
        let line = format!(
            "ruling {id} on #{n}: the project manager {how} {count}{as_label}: {at}: {why}"
        );
        let settled = Settled {
            ruling: id,
            issue: n,
            how,
            recommends: None,
            why,
        };
        self.keep_settled(settled, line)?;
        Ok(Ok(format!(
            "{verb} {count} of ruling {id} on #{n}{as_label}: {at}"
        )))
    }

    // Whether follow-up ruling `id` was raised after the forge refused the
    // findings, and so asks about trying again, not about the findings.
    fn refused_ruling(&self, id: u64) -> bool {
        let ruling = self.state.rulings.iter().find(|r| r.id == id);
        ruling.is_some_and(|r| {
            matches!(
                r.kind,
                RulingKind::FollowUp {
                    refused: Some(_),
                    ..
                }
            )
        })
    }

    // The place of each finding of follow-up ruling `id`.
    fn findings_of(&self, id: u64) -> Vec<String> {
        let ruling = self.state.rulings.iter().find(|r| r.id == id);
        let Some(RulingKind::FollowUp { findings, .. }) = ruling.map(|r| &r.kind) else {
            return Vec::new();
        };
        let place = |f: &Finding| match f.line {
            0 => f.file.clone(),
            line => format!("{}:{line}", f.file),
        };
        findings.iter().map(place).collect()
    }

    // The question or follow-up ruling the work item for `issue` is parked
    // on, if any. One the maintainer was alerted of is theirs to answer,
    // however late the project manager's word comes.
    fn asked_on(&self, issue: u64) -> Asked {
        let Some(Phase::Ruling { id }) = self.state.item(issue).map(|i| i.phase.clone()) else {
            return Asked::Not;
        };
        let held = |kind: &RulingKind| {
            matches!(
                kind,
                RulingKind::Question { .. } | RulingKind::FollowUp { .. }
            )
        };
        match self.state.rulings.iter().find(|r| r.id == id) {
            Some(r) if held(&r.kind) && r.alerted => Asked::Alerted(id),
            Some(r) if held(&r.kind) => Asked::On(id),
            _ => Asked::Not,
        }
    }

    // Whether ruling `id` is a follow-up, which has no worker waiting on text.
    fn follow_up_ruling(&self, id: u64) -> bool {
        let ruling = self.state.rulings.iter().find(|r| r.id == id);
        ruling.is_some_and(|r| matches!(r.kind, RulingKind::FollowUp { .. }))
    }

    // Whether `choice` is the yes or no a follow-up ruling asks for.
    fn yes_or_no(choice: Option<&str>) -> bool {
        choice.is_some_and(|c| {
            let c = c.trim();
            c.eq_ignore_ascii_case("yes") || c.eq_ignore_ascii_case("no")
        })
    }

    // Adds the recommendation to ruling `id`, to be posted as it is.
    fn recommend(&mut self, id: u64, choice: Option<&str>, why: &str) -> Result<(), StateError> {
        let mut next = self.state.clone();
        let ruling = (next.rulings.iter_mut().find(|r| r.id == id)).expect("found by the caller");
        let issue = ruling.issue.expect("a question parks a work item");
        match &mut ruling.kind {
            RulingKind::Question { asked, .. } => {
                *asked = recommended(asked, choice, why);
                ruling.question = question(id, issue, ruling.pull_request, &ruling.kind);
            }
            // Its kind holds no words of the project manager's, so only the
            // question the maintainer reads does.
            RulingKind::FollowUp { .. } => {
                ruling.question = recommended(&ruling.question, choice, why);
            }
            _ => {}
        }
        ruling.alerted = false;
        self.save(next)
    }

    // Keeps `settled` for `status`, and `line` in the notes and the log.
    fn keep_settled(&mut self, settled: Settled, line: String) -> Result<(), StateError> {
        self.pm.settled.push(settled);
        let over = self.pm.settled.len().saturating_sub(KEPT);
        self.pm.settled.drain(..over);
        self.note_settled(&line);
        self.record(vec![line])
    }

    // Adds `line` to the end of the project manager's notes, since what it
    // settled is what a later wake must remember.
    fn note_settled(&mut self, line: &str) {
        let path = self.paths.pm.join(PM_NOTES);
        let added = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut file| writeln!(file, "- {line}"));
        if let Err(e) = added {
            self.notes
                .push(format!("cannot add to the project manager's notes: {e}"));
        }
    }
}

// `asked` with the project manager's `choice` on a line of its own, and
// first in the `- ` list that ends it if it has one: a choice it lists moves
// up, and one it does not is added. The list stays last.
fn recommended(asked: &str, choice: Option<&str>, why: &str) -> String {
    let lines: Vec<&str> = asked.lines().collect();
    let listed = |line: &str| line.trim_start().starts_with("- ");
    let start = lines.iter().rposition(|l| !listed(l)).map_or(0, |i| i + 1);
    let (head, list) = lines.split_at(start);
    let list: Vec<String> = {
        let named = choice.map(plain_choice);
        let at = list
            .iter()
            .position(|line| named.is_some() && Some(plain_choice(line)) == named);
        let mut list: Vec<String> = list.iter().map(|l| (*l).to_owned()).collect();
        match (choice, at) {
            (Some(_), Some(at)) => {
                let line = list.remove(at);
                list.insert(0, line);
            }
            (Some(choice), None) if !list.is_empty() => list.insert(0, format!("- {choice}")),
            _ => {}
        }
        list
    };
    let shown = match (choice, list.first()) {
        (Some(_), Some(first)) => Some(first.trim_start().trim_start_matches("- ").to_owned()),
        (Some(choice), None) => Some(choice.to_owned()),
        (None, _) => None,
    };
    let mut out: Vec<String> = head.iter().map(|l| (*l).to_owned()).collect();
    out.extend(shown.map(|c| format!("PM recommends: {c}")));
    if !why.is_empty() {
        out.push(format!("PM says: {why}"));
    }
    out.extend(list);
    out.join("\n")
}

// A choice as it is compared: without its `- `, its backticks or its case.
fn plain_choice(text: &str) -> String {
    let text = text.trim();
    let text = text.strip_prefix("- ").unwrap_or(text);
    text.trim().trim_matches('`').to_lowercase()
}
