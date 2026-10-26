//! The local round's port: the trait a round runs through, and why one fails

use std::fmt;

use super::Finding;
use crate::settings::LocalRound;
use crate::worktree::Linked;

/// Whether a local round waits for the GPU or runs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundStage {
    /// The round queues for the GPU
    Queued,
    /// The round runs, or no longer queues
    Running,
}

/// Runs one local round, of the kind the project's settings choose
pub trait Reviewer: Send + Sync {
    /// Checks, as the runner starts, that `local` can run: its command is
    /// there, or its endpoint answers
    ///
    /// # Errors
    ///
    /// Why it cannot, naming the command or the endpoint.
    fn check(&self, local: &LocalRound) -> Result<(), String>;

    /// Runs `local` for round `round` against `worktree`'s diff from `base`, writing findings under `out`
    ///
    /// `criteria` is what the issue asks for, which the round checks the
    /// diff against, as its prompt or a file the command is pointed at.
    /// The worker writes `worktree`, so git is run on it only as checked
    /// against its repo.
    ///
    /// # Errors
    ///
    /// [`ReviewerError`] when the round cannot be run or did not finish,
    /// or when `worktree`'s git dirs are not its repo's.
    fn round(
        &self,
        local: &LocalRound,
        worktree: Linked<'_>,
        base: &str,
        out: &std::path::Path,
        round: u32,
        criteria: &str,
    ) -> Result<Vec<Finding>, ReviewerError>;

    /// Runs `local` as [`Reviewer::round`] does, telling `watch` when the
    /// round starts or stops queueing for the GPU
    ///
    /// A round that never queues never calls `watch`, which is what this
    /// default does.
    ///
    /// # Errors
    ///
    /// As [`Reviewer::round`].
    #[expect(clippy::too_many_arguments, reason = "`round`'s own, and the watch")]
    fn round_watched(
        &self,
        local: &LocalRound,
        worktree: Linked<'_>,
        base: &str,
        out: &std::path::Path,
        round: u32,
        criteria: &str,
        _watch: &(dyn Fn(RoundStage) + Sync),
    ) -> Result<Vec<Finding>, ReviewerError> {
        self.round(local, worktree, base, out, round, criteria)
    }
}

/// Why a local round did not produce findings
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewerError {
    /// The command could not be started, with the OS's reason
    Spawn(String),
    /// The command ran and exited unsuccessfully, with this on stderr
    Failed(String),
    /// The command exited successfully but left no completion marker
    Incomplete,
    /// The round was ended because the runner is stopping
    Stopped,
    /// The endpoint answered with something other than a chat completion
    Unreadable(String),
    /// The model stopped at its token limit before it finished, or inside its
    /// thinking, with the reason
    CutOff(String),
    /// The server or its gateway turned the round away as busy, with the reason
    Busy(String),
    /// The server did not answer, with the reason
    Unreachable(String),
}

/// How a local round ended, as a kept round records it
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RoundEnd {
    /// The round came back with its answer
    Answered,
    /// The model stopped at its token limit
    CutOff,
    /// The server answered with something that is not a chat completion
    Unreadable,
    /// The server turned the round away as busy
    Busy,
    /// The server did not answer
    Unreachable,
    /// Anything else that stopped the round
    Failed,
}

impl ReviewerError {
    /// How a round that ended with this error is recorded
    pub fn ended(&self) -> RoundEnd {
        match self {
            Self::CutOff(_) => RoundEnd::CutOff,
            Self::Unreadable(_) => RoundEnd::Unreadable,
            Self::Busy(_) => RoundEnd::Busy,
            Self::Unreachable(_) => RoundEnd::Unreachable,
            Self::Spawn(_) | Self::Failed(_) | Self::Incomplete | Self::Stopped => RoundEnd::Failed,
        }
    }
}

impl fmt::Display for ReviewerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(f, "cannot run the local round: {error}"),
            Self::Failed(stderr)
            | Self::CutOff(stderr)
            | Self::Busy(stderr)
            | Self::Unreachable(stderr) => {
                write!(f, "the local round failed: {}", stderr.trim())
            }
            Self::Incomplete => f.write_str("the local round left no completion marker"),
            Self::Stopped => f.write_str("the local round was stopped with the runner"),
            Self::Unreadable(reply) => write!(f, "the local round's reply is unreadable: {reply}"),
        }
    }
}

impl core::error::Error for ReviewerError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_way_a_round_fails_is_recorded_as_one_ending() {
        let why = || "why".to_owned();
        for (error, ended) in [
            (ReviewerError::CutOff(why()), RoundEnd::CutOff),
            (ReviewerError::Unreadable(why()), RoundEnd::Unreadable),
            (ReviewerError::Busy(why()), RoundEnd::Busy),
            (ReviewerError::Unreachable(why()), RoundEnd::Unreachable),
            (ReviewerError::Failed(why()), RoundEnd::Failed),
            (ReviewerError::Spawn(why()), RoundEnd::Failed),
            (ReviewerError::Incomplete, RoundEnd::Failed),
        ] {
            assert_eq!(error.ended(), ended, "{error}");
        }
        assert_eq!(
            serde_json::to_value(RoundEnd::CutOff).unwrap(),
            serde_json::json!("cut_off")
        );
    }
}
