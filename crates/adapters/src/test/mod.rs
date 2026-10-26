//! The stand-ins that name an adapter, and the ones the adapters' own tests use
//!
//! The rest of the stand-ins are in `shep-kelpie-core`, which the root's rig
//! gathers with these.

mod github;
mod leases;
mod reviewer;

pub use github::{Asked, FakeGithub, FakeSigner, HeldToken, PEM, REVIEW_ID, SentReview};
pub use leases::{DogStandIn, FakeLeases, Told};
pub use reviewer::{FakeReviewer, ScriptedRound, SeenRound};

use shep_kelpie_core::test::{AppComments, FakeCodeRabbit, Hold};

pub(crate) use shep_kelpie_core::test::FakeClock;

// A program no machine holds, so a test's policy names one and lets no real
// harness's files through.
#[cfg(test)]
pub(crate) static NO_PROGRAM: std::sync::LazyLock<std::path::PathBuf> =
    std::sync::LazyLock::new(|| std::path::Path::new("/nowhere/program").to_owned());

// What the adapters' own tests use of core's stand-ins and of `test_support`
#[cfg(test)]
pub(crate) use shep_kelpie_core::test::{
    Answer, OpenSandbox, StandInEndpoint, Take, ticket, unreachable_url,
};
#[cfg(test)]
pub(crate) use shep_kelpie_test_support::{
    CALL_PATIENCE, Elsewhere, Fake, cut_from_main, exited, git, linked_worktree, main_commits,
    make_fifo, short_tempdir, waits_for, write_fake, write_script,
};
