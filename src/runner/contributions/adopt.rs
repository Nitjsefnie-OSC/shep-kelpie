//! A contribution kelpie cannot finish on its contributor's branch
//!
//! Kelpie finishes a released contribution by pushing to the contributor's
//! own branch (ADR 0010), which a fork lets it do only where its owner allows
//! edits by maintainers. A branch on the repo itself needs no permission.
//! When the contributor has been quiet for `quiet_days` and the fork cannot
//! be pushed to, nothing is asked or released: the maintainer gets a ruling,
//! to leave it open or close it with thanks. Nothing is posted on the pull
//! request until it is answered, and leaving it open is the end of it: the
//! pull request is followed no further.

use super::super::Runner;
use super::ruling::raise_no_edits;
use crate::state::StateError;

impl Runner {
    // Whether the release stage stops for the pull request this poll: the
    // contribution is on a fork kelpie cannot push to, and its ruling is
    // raised now, or the forge could not say, so nothing is concluded. The
    // next poll looks again.
    pub(super) fn awaits_edits(&mut self, number: u64) -> Result<bool, StateError> {
        let Ok(pr) = self.ports.forge.reviewed(&self.remote, number) else {
            return Ok(true);
        };
        if pr.pushable() {
            return Ok(false);
        }
        let now = self.ports.clock.now();
        let mut next = self.state.clone();
        raise_no_edits(&mut next, number, now);
        self.save(next)?;
        Ok(true)
    }
}
