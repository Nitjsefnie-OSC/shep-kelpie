//! The fake forge approving the CI runs a fork's pull request holds

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use super::FakeForge;
use crate::ports::ForgeError;

/// The heads whose held runs were approved, and whether approving is refused
#[derive(Debug, Default)]
pub struct Approvals {
    approved: Mutex<Vec<String>>,
    refused: AtomicBool,
}

impl FakeForge {
    /// Each head whose held runs kelpie approved, in order, once per call
    pub fn approvals(&self) -> Vec<String> {
        self.approvals.approved.lock().unwrap().clone()
    }

    /// Makes approving held runs fail, or work again
    pub fn set_approvals_refused(&self, refused: bool) {
        self.approvals.refused.store(refused, Ordering::SeqCst);
    }

    /// Approves the pull request at `head`
    pub fn approve(&self, head: &str) -> Result<(), ForgeError> {
        self.ask()?;
        if self.approvals.refused.load(Ordering::SeqCst) {
            return Err(ForgeError::Failed("approval refused".to_owned()));
        }
        self.approvals
            .approved
            .lock()
            .unwrap()
            .push(head.to_owned());
        Ok(())
    }
}
