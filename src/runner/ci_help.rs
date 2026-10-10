//! How a worker is told to read why CI failed
//!
//! A worker on a repo the App covers holds a token that pushes and does
//! nothing else, so `gh pr checks` and `gh run view` are refused it. Kelpie
//! reads the end of the failed runs' logs as the login `gh` acts as, and the
//! prompt carries them. Any other worker is pointed at the commands.

use super::Runner;

impl Runner {
    /// What a red run's prompt says about reading the failure
    pub(super) fn red_help(&self, number: u64, head: &str) -> String {
        match self.app_covers() {
            true => self.logs_for(head, false),
            false => format!(
                "`gh pr checks {number}` lists the checks, and `gh run view <run id> \
                 --log-failed` shows what failed."
            ),
        }
    }

    /// What a merge queue removal's prompt says about reading the failure
    pub(super) fn queue_help(&self, head: &str) -> String {
        match self.app_covers() {
            true => self.logs_for(head, true),
            false => "`gh run list --event merge_group` lists the queue's runs, and `gh run \
                      view <run id> --log-failed` shows what failed."
                .to_owned(),
        }
    }

    fn logs_for(&self, head: &str, merge_group: bool) -> String {
        let read = self
            .ports
            .forge
            .failed_run_logs(&self.remote, head, merge_group);
        match read {
            Ok(logs) if logs.trim().is_empty() => "Your credential cannot read CI, and \
                kelpie found no failed run to read for you: reproduce the failing checks \
                with the repo's own commands."
                .to_owned(),
            Ok(logs) => format!(
                "Your credential cannot read CI, so kelpie read the end of each failed run's \
                 log for you:\n\n{logs}\n"
            ),
            Err(e) => format!(
                "Your credential cannot read CI, and kelpie could not read it for you ({e}): \
                 reproduce the failing checks with the repo's own commands."
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ports::Checks;
    use crate::test::Rig;

    // The prompt a red run sends the worker back with
    fn red_prompt(
        rig: &Rig,
        runner: &std::sync::Mutex<crate::runner::Runner>,
        head: &str,
    ) -> String {
        rig.forge
            .set_checks(head, Checks::Failed(vec!["test".to_owned()]));
        rig.verdict(runner);
        rig.claude.script([crate::test::Scripted::Say("fixed")]);
        crate::runner::step(runner).unwrap();
        rig.claude.calls().last().unwrap().prompt.clone()
    }

    #[test]
    fn on_a_covered_repo_the_prompt_carries_the_failed_logs_instead_of_commands_that_fail() {
        let (rig, runner, head) = Rig::with_pull_request_set("koji", Rig::with_app);
        rig.forge
            .set_ci_log("--- run 9\nerror[E0308]: mismatched types");

        let prompt = red_prompt(&rig, &runner, &head);

        assert!(
            prompt.contains("error[E0308]: mismatched types"),
            "{prompt}"
        );
        assert!(!prompt.contains("gh run view"), "{prompt}");
    }

    #[test]
    fn with_no_app_the_prompt_points_at_the_commands() {
        let (rig, runner, head) = Rig::with_pull_request("koji");

        let prompt = red_prompt(&rig, &runner, &head);

        assert!(
            prompt.contains("`gh pr checks 71` lists the checks"),
            "{prompt}"
        );
    }
}
