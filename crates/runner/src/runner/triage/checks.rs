//! The issue check the board runs on every open issue
//!
//! Each poll checks every open issue that is new to it or edited since
//! ([`check::key`]), whatever `triage.pick` says, and keeps the result in
//! `checks`. The maintainer's issue that passes is labelled `ready-for-agent`,
//! and anyone else's waits at `needs-triage` for the maintainer. One that fails
//! is owed the triager, where the pick rule reaches it, and the board holds a
//! ready one meanwhile. Once its pass ends the issue is checked again, and a
//! rewrite that still fails gets one comment naming what the check refused.

use std::collections::BTreeSet;

use super::super::Runner;
use super::TRIAGE_LABELS;
use super::comment::NEEDS_TRIAGE;
use super::ready::READY_LABEL;
use crate::board::{READY, ReadyIssue, Skip, TRIAGE, check};
use crate::ports::OpenIssue;
use crate::state::{CheckTriage, IssueCheck, TriageStep};

// What a check asks of the forge, once its result is new
enum Act {
    // It passed: the maintainer's is made ready, anyone else's waits
    Passed,
    // Its triage's rewrite still fails: one comment names the failures
    Refused,
    // Nothing to write
    Kept,
}

fn carries(labels: &[String], label: &str) -> bool {
    labels.iter().any(|l| l == label)
}

impl Runner {
    // Checks each issue in `open` that is new, edited, or whose triage ended since,
    // and does what the result asks. One in `busy` was under a pass when the poll
    // began, and waits for a listing made after its end. A write that fails leaves
    // the old result, so the next poll tries it again.
    pub(super) fn check_issues(&mut self, open: &[OpenIssue], busy: &BTreeSet<u64>) {
        let mut kept: Vec<IssueCheck> = (self.state.checks.iter())
            .filter(|c| open.iter().any(|i| i.number == c.issue))
            .cloned()
            .collect();
        for issue in open.iter().filter(|i| !i.author_is_bot) {
            // A pass under way writes the body and labels itself, and its end
            // is what the next check reads.
            if busy.contains(&issue.number) || self.triage_busy(issue.number) {
                continue;
            }
            let key = check::key(&issue.title, &issue.body, &issue.labels);
            let at = kept.iter().position(|c| c.issue == issue.number);
            let ran = at.is_some_and(|at| kept[at].triage == CheckTriage::Ran);
            let same = at.is_some_and(|at| kept[at].key == key);
            if same && !ran {
                continue;
            }
            let failures: Vec<String> = (check::check(&issue.title, &issue.body, &issue.labels))
                .iter()
                .map(ToString::to_string)
                .collect();
            // An issue triaged before is owed a new pass for an edit, and not for
            // the check's first read of it.
            let triaged = self.triage_record(issue.number).is_some();
            // A pass that rewrote nothing the check reads, as silence leaves
            // it, said all it had in its own comment, and a triage label put
            // on by hand is the maintainer's word on it.
            let decided = (issue.labels.iter())
                .any(|l| l != READY && l != TRIAGE && TRIAGE_LABELS.contains(&l.as_str()));
            let (triage, act) = match (failures.is_empty(), ran) {
                (true, _) => (CheckTriage::None, Act::Passed),
                (false, true) if same || decided => (CheckTriage::None, Act::Kept),
                (false, true) => (CheckTriage::None, Act::Refused),
                (false, false) if triaged && at.is_none() => (CheckTriage::None, Act::Kept),
                (false, false) if triaged => (CheckTriage::Edited, Act::Kept),
                (false, false) => (CheckTriage::Owed, Act::Kept),
            };
            let done = match act {
                Act::Passed => self.check_passed(issue),
                Act::Refused => self.check_refused(issue, &failures),
                Act::Kept => true,
            };
            if !done {
                continue;
            }
            let result = IssueCheck {
                issue: issue.number,
                key,
                failures,
                triage,
            };
            match at {
                Some(at) => kept[at] = result,
                None => kept.push(result),
            }
        }
        if kept == self.state.checks {
            return;
        }
        let mut next = self.state.clone();
        next.checks = kept.clone();
        // The results can be read again from the forge, so a save that fails
        // keeps them in memory and holds up nothing else.
        if let Err(e) = self.save(next) {
            self.notes
                .push(format!("cannot save the issue check's results: {e}"));
            self.state.checks = kept;
        }
    }

    // Whether a triage pass on `issue` is under way: its call, a comment not yet
    // posted, or a record short of its end
    pub(super) fn triage_busy(&self, issue: u64) -> bool {
        let unfinished = self.triage_record(issue).is_some_and(|t| {
            !matches!(
                t.step,
                TriageStep::Done | TriageStep::Ready | TriageStep::Closed
            )
        });
        let unposted = (self.triage_tries.get(&issue))
            .is_some_and(|t| t.waiting.is_some() || t.body.is_some());
        unfinished || unposted || self.flights.triaging() == Some(issue)
    }

    // Whether `issue` failed its last check and the triager has not run on it since
    pub(super) fn check_owed(&self, issue: u64) -> bool {
        (self.state.checks.iter()).any(|c| {
            c.issue == issue && matches!(c.triage, CheckTriage::Owed | CheckTriage::Edited)
        })
    }

    // Whether `issue` failed on an edit made after its triage ended
    pub(super) fn check_edited(&self, issue: u64) -> bool {
        (self.state.checks.iter()).any(|c| c.issue == issue && c.triage == CheckTriage::Edited)
    }

    // The ready issues the check refused, as the board's skips: one whose
    // result is for the text it holds now, or whose triage ran and is not yet
    // checked again
    pub(in crate::runner) fn check_holds(&self, ready: &[ReadyIssue]) -> Vec<Skip> {
        (ready.iter())
            .filter_map(|issue| {
                let number = issue.ticket.github()?;
                let ended = (self.state.finished.iter())
                    .chain(&self.state.dropped)
                    .any(|t| *t == issue.ticket);
                if ended {
                    return None;
                }
                let kept = self.state.checks.iter().find(|c| c.issue == number)?;
                let key = check::key(&issue.title, &issue.body, &issue.labels);
                let current = kept.key == key || kept.triage == CheckTriage::Ran;
                (!kept.passed() && current).then(|| Skip::Check {
                    issue: issue.ticket,
                    failures: kept.failures.clone(),
                })
            })
            .collect()
    }

    // Makes the maintainer's passing issue ready, taking `needs-triage` off, and
    // puts `needs-triage` on anyone else's. A triage label other than those
    // two stands, and an issue the board has had a work item for is left as it
    // is. Whether nothing is left to write.
    fn check_passed(&mut self, issue: &OpenIssue) -> bool {
        let number = issue.number;
        let decided = (issue.labels.iter())
            .any(|l| l != READY && l != TRIAGE && TRIAGE_LABELS.contains(&l.as_str()));
        if decided || self.has_work_item(number) {
            return true;
        }
        let Ok(maintainer) = self.maintainer() else {
            return false;
        };
        let wanted: &[(&str, bool)] = match issue.author.eq_ignore_ascii_case(&maintainer) {
            true => &[(READY, true), (TRIAGE, false)],
            false if carries(&issue.labels, READY) => &[],
            false => &[(TRIAGE, true)],
        };
        for &(label, on) in wanted {
            if carries(&issue.labels, label) == on {
                continue;
            }
            let made = match on {
                true if label == READY => self.make_missing_labels([READY_LABEL]),
                true => self.make_missing_labels([NEEDS_TRIAGE]),
                false => Ok(()),
            };
            if let Err(why) = made {
                self.notes
                    .push(format!("check #{number}: cannot make `{label}`: {why}"));
                return false;
            }
            if let Err(e) = self.label_issue(number, label, on) {
                let how = if on { "label it" } else { "take off" };
                self.notes
                    .push(format!("check #{number}: cannot {how} `{label}`: {e}"));
                return false;
            }
            if label == READY {
                self.notes.push(format!(
                    "check #{number}: the maintainer's issue passes the issue check, so it is \
                     labelled `{READY}`"
                ));
            }
        }
        true
    }

    // Says on the maintainer's `issue` once what the check still refuses after
    // its triage. Anyone else's is told in plain words, which the check's are
    // not, and its triage comment already handed it to the maintainer.
    fn check_refused(&mut self, issue: &OpenIssue, failures: &[String]) -> bool {
        let number = issue.number;
        let Ok(maintainer) = self.maintainer() else {
            return false;
        };
        if !issue.author.eq_ignore_ascii_case(&maintainer) {
            return true;
        }
        let listed: Vec<String> = failures.iter().map(|f| format!("- {f}")).collect();
        let text = format!(
            "Kelpie checked this issue again after its triage, and the issue check still \
             refuses it:\n\n{}\n\nIt waits at `{TRIAGE}`. Kelpie checks it again, and triages \
             it again if it still fails, once it is edited.",
            listed.join("\n")
        );
        match self.comment_on_issue(number, &text) {
            Ok(()) => {
                self.notes.push(format!(
                    "check #{number}: its triage's rewrite still fails the issue check, said on \
                     the issue"
                ));
                true
            }
            Err(e) => {
                self.notes.push(format!(
                    "check #{number}: cannot say what the check refused: {e}"
                ));
                false
            }
        }
    }
}
