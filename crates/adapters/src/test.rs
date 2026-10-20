//! The stand-ins that name an adapter, and the ones the adapters' own tests use
//!
//! The rest of the stand-ins are in `shep-kelpie-core`, which the root's rig
//! gathers with these.

mod github;
mod leases;
mod reviewer;

pub use github::{Asked, FakeGithub, FakeSigner, PEM, REVIEW_ID, SentReview};
pub use leases::{DogStandIn, FakeLeases, Told};
pub use reviewer::{FakeReviewer, ScriptedRound, SeenRound};

use shep_kelpie_core::test::{AppComments, FakeCodeRabbit, Hold};

pub(crate) use shep_kelpie_core::test::FakeClock;

// What the adapters' own tests use of core's stand-ins and of `test_support`
#[cfg(test)]
pub(crate) use shep_kelpie_core::test::{
    Answer, OpenSandbox, StandInEndpoint, Take, ticket, unreachable_url,
};
#[cfg(test)]
pub(crate) use shep_kelpie_test_support::{
    CALL_PATIENCE, Elsewhere, cut_from_main, git, linked_worktree, main_commits, make_fifo,
    short_tempdir, write_script,
};
