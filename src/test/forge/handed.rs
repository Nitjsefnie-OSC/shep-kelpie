//! The fake forge's pull requests kelpie opens for a worker, and the CI logs
//! it reads for one

use super::FakeForge;

impl FakeForge {
    /// Makes the failed CI runs' logs say `log`
    pub(crate) fn set_ci_log(&self, log: &str) {
        *self.ci_log.lock().unwrap() = log.to_owned();
    }

    /// Makes reading the failed CI runs' logs fail with `error`, verbatim
    pub(crate) fn set_ci_log_error(&self, error: &str) {
        *self.ci_log_error.lock().unwrap() = Some(error.to_owned());
    }

    /// The title and body kelpie gave pull request `number` when it opened or
    /// edited it, or none when it did neither
    pub(crate) fn text_of(&self, number: u64) -> Option<(String, String)> {
        self.texts.lock().unwrap().get(&number).cloned()
    }
}
