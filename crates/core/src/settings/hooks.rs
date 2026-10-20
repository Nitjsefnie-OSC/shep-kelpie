//! A project's own hooks, which kelpie runs at its own points

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::NonBlank;

/// A point in kelpie's work where a project's hook runs
// wire format: the point is named in the payload a hook reads and in the
// arguments of `kelpie guard`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HookPoint {
    /// Before any text goes to the forge, from any role or from kelpie
    /// itself: a pull request's title and body, a comment, a reply on a
    /// review thread, an issue, a ruling
    Publish,
    /// Before a worker's commit lands, with its message and staged diff
    Commit,
    /// Before kelpie or a worker pushes, with the commits being pushed
    Push,
    /// When a worker ends its turn with the branch ready, before review
    Handover,
    /// Before a shell command an agent runs
    Command,
    /// Before an agent writes or edits a file
    Write,
    /// After a shell command has run, failed or not
    AfterCommand,
    /// Before kelpie sends a worker, reviewer or the project manager a prompt
    TurnStart,
    /// When a worker's turn ends
    TurnEnd,
    /// Before kelpie resets a worker or the project manager
    Reset,
    /// When a ruling is raised
    Ruling,
    /// When CI settles on a head
    Ci,
    /// When a work item ends, merged or not
    Done,
}

/// Whether a point's hook can refuse, or only tells
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointKind {
    /// A refusal stops what the point guards
    Gate,
    /// A non-zero exit is logged and kelpie goes on
    Notice,
}

impl HookPoint {
    /// The point as the payload and the project's settings spell it
    #[inline]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Publish => "publish",
            Self::Commit => "commit",
            Self::Push => "push",
            Self::Handover => "handover",
            Self::Command => "command",
            Self::Write => "write",
            Self::AfterCommand => "after_command",
            Self::TurnStart => "turn_start",
            Self::TurnEnd => "turn_end",
            Self::Reset => "reset",
            Self::Ruling => "ruling",
            Self::Ci => "ci",
            Self::Done => "done",
        }
    }

    /// Whether the point's hook refuses or only tells
    #[inline]
    pub fn kind(self) -> PointKind {
        match self {
            Self::Reset | Self::Ruling | Self::Ci | Self::Done => PointKind::Notice,
            _ => PointKind::Gate,
        }
    }
}

/// One of a project's own hooks, run by kelpie at `point`
///
/// Kelpie runs the command with a JSON payload on its stdin. At a gate,
/// exit 0 passes, 2 refuses with its stderr as the reason, and any other
/// exit refuses as a hook that failed. At a notice (`reset`, `ruling`, `ci`
/// and `done`) a non-zero exit is logged and kelpie goes on.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectHook {
    /// Where kelpie runs it
    pub point: HookPoint,
    /// The shell command kelpie runs. Its program must resolve when the
    /// runner starts.
    pub command: NonBlank,
}
