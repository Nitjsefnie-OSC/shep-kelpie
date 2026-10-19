//! The log lines for a ready issue the board passes over
//!
//! A skip is said once when it begins, with its reason, and once when it
//! clears. A pass that finds the same skip says nothing. A skip whose reason
//! changes is said again with the new one. The runner's `skipped` is also in
//! `status`, and on a dispatch line, which says nothing of a skip while
//! nothing dispatches.

use super::Runner;
use super::briefing::passed_over;
use crate::board::Skip;
use crate::ticket::TicketId;

impl Runner {
    // Takes `skipped` as the issues the board passes over now, and notes
    // each skip that began, changed its reason or cleared since the last pass
    pub(super) fn set_skipped(&mut self, skipped: Vec<Skip>) {
        let reason = |skip: &Skip| passed_over(skip, &|text| text).0;
        // An adopted pull request has no issue, and its refusal is a report.
        let named = |skips: &[Skip]| -> Vec<(TicketId, String)> {
            (skips.iter())
                .filter_map(|skip| skip.issue().map(|ticket| (ticket, reason(skip))))
                .collect()
        };
        let (before, after) = (named(&self.skipped), named(&skipped));
        for (issue, why) in &after {
            if !before.contains(&(*issue, why.clone())) {
                self.notes.push(format!("#{issue} skipped reason={why}"));
            }
        }
        for (issue, _) in &before {
            if !after.iter().any(|(now, _)| now == issue) {
                self.notes.push(format!("#{issue} skip-cleared"));
            }
        }
        self.skipped = skipped;
    }

    // Takes `failed` as the skips of a pass that began a call before the board
    // was read: the board's skips stand as they were, so the pass neither
    // clears nor repeats them
    pub(super) fn set_failed_skips(&mut self, failed: Vec<Skip>) {
        let mut skipped: Vec<Skip> = (self.skipped.iter())
            .filter(|skip| {
                !matches!(
                    skip,
                    Skip::Failed { .. } | Skip::Rework { .. } | Skip::Adopt { .. }
                )
            })
            .cloned()
            .collect();
        skipped.extend(failed);
        skipped.sort_by_key(Skip::issue);
        self.set_skipped(skipped);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use crate::runner::{Runner, StepReport, step};
    use crate::test::Rig;

    fn notes(runner: &Mutex<Runner>) -> Vec<String> {
        runner.lock().unwrap().take_notes()
    }

    #[test]
    fn a_skip_is_said_once_when_it_begins_and_once_when_it_clears_and_not_on_every_pass() {
        let rig = Rig::new("ditto");
        let runner = rig.open().unwrap();
        rig.forge.list_ready(8, false);
        rig.forge.block(8, 32);

        for _ in 0..3 {
            assert_eq!(step(&runner).unwrap(), None);
            rig.next_look();
        }
        assert_eq!(notes(&runner), ["#8 skipped reason=blocked by #32"]);

        rig.forge.close_issue(32);
        assert!(matches!(
            step(&runner).unwrap(),
            Some(StepReport::Dispatched { issue: 8, .. })
        ));
        assert_eq!(notes(&runner), ["#8 skip-cleared"]);
    }

    #[test]
    fn a_skip_whose_reason_changes_is_said_again_with_the_new_reason() {
        let rig = Rig::new("ditto");
        let runner = rig.open().unwrap();
        rig.forge.list_ready(8, false);
        rig.forge.block(8, 32);
        step(&runner).unwrap();
        rig.forge.block(8, 33);
        rig.next_look();
        step(&runner).unwrap();

        assert_eq!(
            notes(&runner),
            [
                "#8 skipped reason=blocked by #32",
                "#8 skipped reason=blocked by #32, #33"
            ]
        );
    }

    #[test]
    fn a_pass_that_begins_a_call_before_the_board_is_read_leaves_the_skips_standing() {
        let rig = Rig::new("shep");
        rig.ci_on();
        let runner = rig.open().unwrap();
        rig.forge.list_ready(8, false);
        rig.forge.block(8, 32);
        step(&runner).unwrap();
        assert_eq!(notes(&runner), ["#8 skipped reason=blocked by #32"]);

        rig.push_by_hand("fix/timeline", "work.txt");
        rig.forge.open_pull_request(80, "fix/timeline", &[5]);
        rig.ask(&runner, "adopt", Some("80"));
        rig.next_look();
        assert!(matches!(
            step(&runner).unwrap(),
            Some(StepReport::Adopted { .. })
        ));
        rig.ask(&runner, "drop", None);
        rig.next_look();
        step(&runner).unwrap();

        assert_eq!(notes(&runner), Vec::<String>::new());
    }
}
