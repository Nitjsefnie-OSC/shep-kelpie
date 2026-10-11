//! A project's own hooks, which kelpie runs at its own points

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::NonBlank;

/// A point in kelpie's work where a project's hook runs
// wire format: the point is named in the payload a hook reads and in the
// arguments of `kelpie guard`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
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
        }
    }
}

/// One of a project's own hooks, run by kelpie at `point`
///
/// Kelpie runs the command with a JSON payload on its stdin. Exit 0
/// passes, 2 refuses with its stderr as the reason, and any other exit
/// refuses as a hook that failed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectHook {
    /// Where kelpie runs it
    pub point: HookPoint,
    /// The shell command kelpie runs. Its program must resolve when the
    /// runner starts.
    pub command: NonBlank,
}
