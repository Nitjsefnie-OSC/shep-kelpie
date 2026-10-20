//! The fake forge going down, or its rate limit used up, and the count of
//! calls made of it

use std::sync::atomic::Ordering;

use super::FakeForge;
use super::held;
use crate::ports::{Cooldown, ForgeError, Timestamp};

/// What every call fails with while the forge is down, and when its rate
/// limit resets where it can say
pub type Down = (String, Option<Timestamp>);

impl FakeForge {
    /// How many calls the runner made, each counted as it was asked
    pub fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }

    /// How many times the board's ready issues were read
    pub fn board_reads(&self) -> usize {
        self.board_reads.load(Ordering::SeqCst)
    }

    /// Makes every call fail as gh does once the rate limit is used up,
    /// with `reset` as when it resets where the forge can say
    pub fn set_used_up(&self, reset: Option<Timestamp>) {
        let said = "GraphQL: API rate limit already exceeded for user ID 1.";
        *self.down.lock().unwrap() = Some((said.to_owned(), reset));
    }

    /// Makes every call fail as gh does when GitHub's secondary limit is
    /// passed, with `cooldown` as the headers of the answer
    pub fn set_limited(&self, cooldown: Cooldown) {
        *self.limited.lock().unwrap() = Some(cooldown);
    }

    /// Lets every call through again after [`Self::set_limited`]
    pub fn set_unlimited(&self) {
        *self.limited.lock().unwrap() = None;
    }

    /// Makes every call fail with `said`, as gh prints it
    pub fn set_failing(&self, said: &str) {
        *self.down.lock().unwrap() = Some((said.to_owned(), None));
    }

    /// Makes every call fail as gh does with no network, or lets every call through again
    pub fn set_down(&self, down: bool) {
        let said = "error connecting to api.github.com";
        *self.down.lock().unwrap() = down.then(|| (said.to_owned(), None));
    }

    /// Counts a call, and fails it while the forge is down
    ///
    /// A call the test holds open waits here, which is where a pass holds the
    /// runner's lock.
    pub fn ask(&self) -> Result<(), ForgeError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.wait_if_held(held::At::Any);
        if let Some(cooldown) = *self.limited.lock().unwrap() {
            let said = "gh: You have exceeded a secondary rate limit. (HTTP 403)";
            return Err(ForgeError::Limited {
                said: format!("gh failed: {said}"),
                cooldown,
            });
        }
        match &*self.down.lock().unwrap() {
            Some((said, _)) => Err(ForgeError::Failed(said.clone())),
            None => Ok(()),
        }
    }

    /// When the used-up rate limit resets, where the forge can say
    pub fn reset(&self) -> Option<Timestamp> {
        let down = self.down.lock().unwrap();
        down.as_ref().and_then(|(_, reset)| *reset)
    }
}
