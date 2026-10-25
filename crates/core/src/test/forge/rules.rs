//! The fake forge's read of the repo's up-to-date rule, and the knobs that set it

use std::sync::atomic::Ordering;

use super::FakeForge;
use crate::ports::ForgeError;
use crate::settings::ForgeSlug;

impl FakeForge {
    /// Makes the repo require, or not require, a pull request's branch to be up to date
    /// with its base before it may merge. It requires it until a test says otherwise.
    pub fn set_up_to_date_required(&self, required: bool) {
        self.up_to_date_required.store(required, Ordering::SeqCst);
    }

    /// Makes reading the repo's up-to-date rule fail, as it does for a login the
    /// repo will not answer, or lets it work again
    pub fn set_up_to_date_read_down(&self, down: bool) {
        self.up_to_date_read_down.store(down, Ordering::SeqCst);
    }

    /// How many times the repo's up-to-date rule was read
    pub fn up_to_date_reads(&self) -> usize {
        self.up_to_date_reads.load(Ordering::SeqCst)
    }

    pub(super) fn up_to_date_rule(
        &self,
        _repo: &ForgeSlug,
        _base: &str,
    ) -> Result<bool, ForgeError> {
        self.ask()?;
        self.up_to_date_reads.fetch_add(1, Ordering::SeqCst);
        if self.up_to_date_read_down.load(Ordering::SeqCst) {
            return Err(ForgeError::Failed(
                "gh: Resource not accessible by integration (HTTP 403)".into(),
            ));
        }
        Ok(self.up_to_date_required.load(Ordering::SeqCst))
    }
}
