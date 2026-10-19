//! Closing an issue once every one of its sub-issues is closed
//!
//! The board never dispatches an issue with sub-issues: they are worked in
//! its place. Once the forge shows them all closed, a step closes the issue
//! too, whether or not it is on the ready list. One off the list needs at
//! least one sub-issue closed as completed, as a merge's close of a parent
//! does, and the forge is asked for those in one listing. A close the forge
//! keeps refusing is passed over, so the board goes on.

use std::collections::BTreeMap;

use super::Runner;
use super::report::{Begin, StepReport};
use crate::board::ReadyIssue;
use crate::ports::{SubIssueState, Timestamp};

/// Refusals in a row before this runner stops closing an issue, until it
/// restarts, so one the forge will never close cannot hold up the board
const TRIES: u32 = 3;

/// What an issue kelpie closes, once its sub-issues are all closed, says
pub(super) const PARENT_CLOSED: &str =
    "Every sub-issue of this issue is closed, so kelpie closes it too.";

/// The forge's refusals in a row to close each issue, kept in memory only
pub(super) type Refused = BTreeMap<u64, u32>;

/// The parents off the ready list whose sub-issues were all closed as not
/// planned, with how many they had and when they were read, kept in memory
/// only so a look does not read them again
pub(super) type Unbuilt = BTreeMap<u64, (u64, Timestamp)>;

/// How long a parent read as all not planned goes unread, since the forge can
/// change how a closed sub-issue closed without changing their count
const UNBUILT_RECHECK: u64 = 3600;

impl Runner {
    /// Closes an issue whose sub-issues are all closed, if the board lists a
    /// ready one, or the forge an open one with a completed sub-issue, that it
    /// has not refused too often
    pub(super) fn close_done_parent(&mut self, ready: &[ReadyIssue]) -> Option<Begin> {
        let done = ready
            .iter()
            .find(|i| i.sub_issues.all_closed() && self.may_close(i.number));
        let issue = match done {
            Some(done) => done.number,
            None => self.unlisted_done_parent(ready)?,
        };
        Some(Begin::Report(self.close_parent(issue)))
    }

    fn may_close(&self, issue: u64) -> bool {
        self.close_refused.get(&issue).is_none_or(|n| *n < TRIES)
    }

    /// An open issue off the ready list whose sub-issues are all closed with
    /// one completed
    ///
    /// One listing names the candidates. Each is read once more for how its
    /// sub-issues closed, and one with none completed is read again when its
    /// count of sub-issues changes, or an hour on. A read that fails is told
    /// in the log.
    fn unlisted_done_parent(&mut self, ready: &[ReadyIssue]) -> Option<u64> {
        let listed = match self.ports.tickets.finished_parents(&self.remote) {
            Ok(listed) => listed,
            Err(e) => {
                self.notes.push(format!(
                    "cannot list the parents with every sub-issue closed: {e}"
                ));
                return None;
            }
        };
        let now = self.ports.clock.now();
        self.unbuilt.retain(|number, (total, at)| {
            let fresh = now.0 < at.0.saturating_add(UNBUILT_RECHECK);
            fresh && (listed.iter()).any(|p| p.number == *number && p.sub_issues == *total)
        });
        for parent in listed {
            let number = parent.number;
            let held = ready.iter().any(|i| i.number == number)
                || self.state.item(number).is_some()
                || self.unbuilt.contains_key(&number);
            if held || !self.may_close(number) {
                continue;
            }
            match self.ports.tickets.sub_issues(&self.remote, number) {
                // The listing can be stale, so every sub-issue is checked closed here.
                Ok(subs) if subs.iter().any(|s| s.state == SubIssueState::Open) => {}
                Ok(subs) if subs.iter().any(|s| s.state == SubIssueState::Completed) => {
                    return Some(number);
                }
                Ok(subs) if !subs.is_empty() => {
                    self.unbuilt.insert(number, (parent.sub_issues, now));
                }
                Ok(_) => {}
                Err(e) => self.notes.push(format!(
                    "cannot read the sub-issues of #{number} to close it: {e}"
                )),
            }
        }
        None
    }

    fn close_parent(&mut self, issue: u64) -> StepReport {
        // The comment goes first, on the first try only, as the App where one
        // covers the repo, else as a comment kelpie records: closing with
        // `--comment` would post it as the login that answers rulings, with no
        // id to skip. A comment that fails is told in the log, and the close
        // goes on without it.
        if !self.close_refused.contains_key(&issue)
            && let Err(e) = self.comment_on_issue(issue, PARENT_CLOSED)
        {
            self.notes.push(format!(
                "cannot comment on issue #{issue} before closing it: {e}"
            ));
        }
        let closed = (self.ports.forge).close_issue(&self.remote, issue);
        match closed {
            Ok(()) => {
                self.close_refused.remove(&issue);
                StepReport::ParentClosed { issue }
            }
            Err(e) => {
                let failures = self.close_refused.entry(issue).or_default();
                *failures += 1;
                StepReport::ParentCloseFailed {
                    issue,
                    reason: e.to_string(),
                    given_up: *failures >= TRIES,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;
    use crate::runner::step;
    use crate::test::Rig;

    fn running(project: &str) -> (Rig, Mutex<Runner>) {
        let rig = Rig::new(project);
        let runner = rig.open().unwrap();
        (rig, runner)
    }

    #[test]
    fn the_parent_closes_when_its_last_sub_issue_does() {
        let (rig, runner) = running("xilriws");
        rig.forge.list_ready(5, false);
        rig.forge.list_ready(6, false);
        rig.forge.link_sub_issue(5, 6);
        rig.forge.link_sub_issue(5, 7);
        rig.forge.close_issue(7);
        assert!(matches!(
            step(&runner).unwrap(),
            Some(StepReport::Dispatched { issue: 6, .. })
        ));
        assert!(rig.forge.closings().is_empty());

        rig.forge.close_issue(6);
        rig.ask(&runner, "drop", Some("6"));
        assert_eq!(
            step(&runner).unwrap(),
            Some(StepReport::ParentClosed { issue: 5 })
        );
        assert_eq!(rig.forge.closings(), [5]);
        assert_eq!(rig.forge.comments(), [(5, PARENT_CLOSED.to_owned())]);
        assert_eq!(step(&runner).unwrap(), None);
    }

    #[test]
    fn a_parent_with_sub_issues_is_never_dispatched() {
        let (rig, runner) = running("chelone");
        rig.forge.list_ready(5, false);
        rig.forge.list_ready(8, false);
        rig.forge.link_sub_issue(5, 8);
        rig.forge.block(8, 40);
        assert_eq!(step(&runner).unwrap(), None);
        assert_eq!(
            rig.ask(&runner, "status", None)["skipped"],
            json!([
                { "reason": "split", "issue": 5, "open": 1 },
                { "reason": "blocked", "issue": 8, "by": [40] },
            ])
        );
    }

    #[test]
    fn a_parent_the_forge_will_not_close_is_passed_over_and_the_board_goes_on() {
        let (rig, runner) = running("koji");
        rig.forge.list_ready(5, false);
        rig.forge.list_ready(6, false);
        rig.forge.link_sub_issue(5, 8);
        rig.forge.close_issue(8);
        rig.forge.set_closes_down(true);
        let refused = |given_up| StepReport::ParentCloseFailed {
            issue: 5,
            reason: "gh failed: closing is refused".into(),
            given_up,
        };
        assert_eq!(step(&runner).unwrap(), Some(refused(false)));
        rig.next_look();
        assert_eq!(step(&runner).unwrap(), Some(refused(false)));
        rig.next_look();
        assert_eq!(step(&runner).unwrap(), Some(refused(true)));
        rig.next_look();
        assert!(matches!(
            step(&runner).unwrap(),
            Some(StepReport::Dispatched { issue: 6, .. })
        ));
        assert!(rig.forge.closings().is_empty());
        // Told once, on the first try, however often the forge refused.
        assert_eq!(rig.forge.comments(), [(5, PARENT_CLOSED.to_owned())]);

        // A restarted runner tries it again.
        rig.forge.set_closes_down(false);
        rig.ask(&runner, "drop", Some("6"));
        drop(runner);
        let runner = rig.open().unwrap();
        assert_eq!(
            step(&runner).unwrap(),
            Some(StepReport::ParentClosed { issue: 5 })
        );
    }

    #[test]
    fn a_finished_parent_off_the_ready_list_is_closed_with_the_comment() {
        let (rig, runner) = running("pelagia");
        rig.forge.open_issue(5, "A parent", "Split in two.\n");
        rig.forge.link_sub_issue(5, 6);
        rig.forge.link_sub_issue(5, 7);
        rig.forge.close_issue(6);
        rig.forge.close_issue(7);
        assert_eq!(
            step(&runner).unwrap(),
            Some(StepReport::ParentClosed { issue: 5 })
        );
        assert_eq!(rig.forge.closings(), [5]);
        assert_eq!(rig.forge.comments(), [(5, PARENT_CLOSED.to_owned())]);
        rig.next_look();
        assert_eq!(step(&runner).unwrap(), None);
    }

    #[test]
    fn a_parent_with_a_sub_issue_still_open_stays_open() {
        let (rig, runner) = running("pelagia");
        rig.forge.open_issue(5, "A parent", "Split in two.\n");
        rig.forge.link_sub_issue(5, 6);
        rig.forge.link_sub_issue(5, 7);
        rig.forge.close_issue(6);
        assert_eq!(step(&runner).unwrap(), None);
        assert!(rig.forge.closings().is_empty());
    }

    #[test]
    fn a_parent_whose_sub_issues_were_all_not_planned_stays_open_without_a_read_each_look() {
        let (rig, runner) = running("pelagia");
        rig.forge.open_issue(5, "A parent", "Split in two.\n");
        rig.forge.link_sub_issue(5, 6);
        rig.forge.link_sub_issue(5, 7);
        rig.forge.close_issue_as_not_planned(6);
        rig.forge.close_issue_as_not_planned(7);
        assert_eq!(step(&runner).unwrap(), None);
        rig.next_look();
        let before = rig.forge.asked();
        assert_eq!(step(&runner).unwrap(), None);
        let look = rig.forge.asked() - before;
        assert!(rig.forge.closings().is_empty());
        assert!(rig.forge.comments().is_empty());

        // A look with no parent on the forge asks as much.
        let (bare, runner) = running("pelagia");
        assert_eq!(step(&runner).unwrap(), None);
        bare.next_look();
        let before = bare.forge.asked();
        assert_eq!(step(&runner).unwrap(), None);
        assert_eq!(bare.forge.asked() - before, look);
    }

    #[test]
    fn a_parent_with_a_new_sub_issue_is_read_again_and_closes_once_one_is_built() {
        let (rig, runner) = running("pelagia");
        rig.forge.open_issue(5, "A parent", "Split in two.\n");
        rig.forge.link_sub_issue(5, 6);
        rig.forge.close_issue_as_not_planned(6);
        assert_eq!(step(&runner).unwrap(), None);
        rig.forge.link_sub_issue(5, 7);
        rig.forge.close_issue(7);
        rig.next_look();
        assert_eq!(
            step(&runner).unwrap(),
            Some(StepReport::ParentClosed { issue: 5 })
        );
    }

    #[test]
    fn a_finished_parent_the_forge_will_not_close_is_given_up_on_after_three_tries() {
        let (rig, runner) = running("pelagia");
        rig.forge.open_issue(5, "A parent", "Split in two.\n");
        rig.forge.link_sub_issue(5, 6);
        rig.forge.close_issue(6);
        rig.forge.set_closes_down(true);
        let refused = |given_up| StepReport::ParentCloseFailed {
            issue: 5,
            reason: "gh failed: closing is refused".into(),
            given_up,
        };
        assert_eq!(step(&runner).unwrap(), Some(refused(false)));
        rig.next_look();
        assert_eq!(step(&runner).unwrap(), Some(refused(false)));
        rig.next_look();
        assert_eq!(step(&runner).unwrap(), Some(refused(true)));
        rig.next_look();
        assert_eq!(step(&runner).unwrap(), None);
        assert_eq!(rig.forge.comments(), [(5, PARENT_CLOSED.to_owned())]);
    }

    #[test]
    fn a_stale_listing_never_closes_a_parent_with_a_sub_issue_open() {
        let (rig, runner) = running("pelagia");
        rig.forge.open_issue(5, "A parent", "Split in two.\n");
        rig.forge.link_sub_issue(5, 6);
        rig.forge.link_sub_issue(5, 7);
        rig.forge.close_issue(6);
        rig.forge.list_finished_whatever_its_sub_issues(5);
        assert_eq!(step(&runner).unwrap(), None);
        assert!(rig.forge.closings().is_empty());
    }

    #[test]
    fn a_parent_read_as_not_planned_is_read_again_an_hour_on() {
        let (rig, runner) = running("pelagia");
        rig.forge.open_issue(5, "A parent", "Split in two.\n");
        rig.forge.link_sub_issue(5, 6);
        rig.forge.close_issue_as_not_planned(6);
        assert_eq!(step(&runner).unwrap(), None);
        rig.forge.reclose_as_completed(6);
        rig.next_look();
        assert_eq!(step(&runner).unwrap(), None);
        rig.clock.advance(UNBUILT_RECHECK);
        assert_eq!(
            step(&runner).unwrap(),
            Some(StepReport::ParentClosed { issue: 5 })
        );
    }
}
