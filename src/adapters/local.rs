//! The local round, over a command or an OpenAI-compatible endpoint
//!
//! A reviewer's definition chooses the kind. A command keeps the README's
//! contract, as the maintainer's qwen-review script does, and queues its
//! model itself. An endpoint gets kelpie's own reviewer. An endpoint behind
//! a gateway holds the gateway's lease on its model for each round, and the
//! gateway queues the rounds. Kelpie holds no lock of its own around either.

mod command;
mod endpoint;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use super::paddock;
#[cfg(test)]
use super::paddock::Timing;
use super::process::Processes;
use crate::forwarder::Upstream;
use crate::ports::{Finding, LocalLeases, PaddockHolder, Reviewer, ReviewerError, RoundStage};
use crate::settings::{Gateways, LocalRound};
use crate::worktree::Linked;

/// Runs each local round, of whichever kind the settings choose
///
/// Clones share their rounds in flight, so one clone can stop them all.
#[derive(Debug, Clone, Default)]
pub struct LocalReviewer {
    pub(super) processes: Processes,
    // Where an endpoint behind a gateway sends its rounds, with the gateway's key
    pub(super) gateways: Gateways,
}

impl LocalReviewer {
    /// Sends an endpoint's rounds behind a gateway through `gateways`
    pub fn with_gateways(mut self, gateways: Gateways) -> Self {
        self.gateways = gateways;
        self
    }

    /// Takes and renews a gateway lease on `paddock`'s schedule
    #[cfg(test)]
    pub(crate) fn with_paddock_timing(mut self, paddock: Timing) -> Self {
        self.gateways = self.gateways.with_paddock_timing(paddock);
        self
    }

    /// Refuses new rounds as the runner stops, sends the rounds in flight
    /// SIGTERM, and leaves the rest to shep's stop
    pub fn stop(&self) {
        self.processes.stop();
    }
}

impl LocalLeases for LocalReviewer {
    fn paddock_holder(&self, gateway: &Upstream, model: &str) -> Option<PaddockHolder> {
        paddock::holder(gateway, model).ok().flatten()
    }
}

impl Reviewer for LocalReviewer {
    fn check(&self, local: &LocalRound) -> Result<(), String> {
        match local {
            LocalRound::Command(local) => check_command(&local.command),
            LocalRound::Endpoint(endpoint) => self.check_endpoint(endpoint),
        }
    }

    fn round(
        &self,
        local: &LocalRound,
        worktree: Linked<'_>,
        base: &str,
        out: &Path,
        round: u32,
        criteria: &str,
    ) -> Result<Vec<Finding>, ReviewerError> {
        self.round_watched(local, worktree, base, out, round, criteria, &|_| {})
    }

    fn round_watched(
        &self,
        local: &LocalRound,
        worktree: Linked<'_>,
        base: &str,
        out: &Path,
        round: u32,
        criteria: &str,
        watch: &(dyn Fn(RoundStage) + Sync),
    ) -> Result<Vec<Finding>, ReviewerError> {
        match local {
            LocalRound::Command(command) => {
                self.command_round(command, worktree, base, out, round, criteria)
            }
            LocalRound::Endpoint(endpoint) => {
                self.endpoint_round(endpoint, worktree, base, (out, round), criteria, watch)
            }
        }
    }
}

fn check_command(command: &Path) -> Result<(), String> {
    let cannot = |why: &str| format!("cannot run {}: {why}", command.display());
    let meta = std::fs::metadata(command).map_err(|e| cannot(&e.kind().to_string()))?;
    if !meta.is_file() {
        return Err(cannot("not a file"));
    }
    if meta.permissions().mode() & 0o111 == 0 {
        return Err(cannot("not executable"));
    }
    Ok(())
}

/// Removes round `round`'s findings file and marker from `out`, so a rework
/// or a retried round never reads the last run's
fn clear_round(out: &Path, round: u32) -> Result<(), ReviewerError> {
    for name in [
        format!("round-{round}.txt"),
        format!("round-{round}.txt.done"),
    ] {
        let path = out.join(name);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(ReviewerError::Failed(format!(
                    "cannot remove {}: {e}",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

/// The worktree's head commit
fn head(at: Linked<'_>) -> Result<String, ReviewerError> {
    crate::worktree::head(at.repo, at.worktree).map_err(|e| {
        ReviewerError::Failed(format!(
            "cannot read the head of {}: {e}",
            at.worktree.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::LocalCommand;
    use crate::test::write_script;

    fn command(path: &Path) -> LocalRound {
        LocalRound::Command(LocalCommand {
            command: path.to_owned(),
        })
    }

    #[test]
    fn a_command_is_checked_for_being_there_and_runnable() {
        let dir = tempfile::tempdir().unwrap();
        let reviewer = LocalReviewer::default();
        let gone = dir.path().join("gone");
        let err = reviewer.check(&command(&gone)).unwrap_err();
        assert_eq!(
            err,
            format!("cannot run {}: entity not found", gone.display())
        );
        let plain = dir.path().join("plain.txt");
        std::fs::write(&plain, "text\n").unwrap();
        let err = reviewer.check(&command(&plain)).unwrap_err();
        assert!(err.ends_with("plain.txt: not executable"), "{err}");
        let err = reviewer.check(&command(dir.path())).unwrap_err();
        assert!(err.ends_with(": not a file"), "{err}");
        let script = dir.path().join("review");
        write_script(&script, "#!/bin/sh\nexit 0\n");
        assert_eq!(reviewer.check(&command(&script)), Ok(()));
    }
}
