//! The writes a bump needs, which the poll makes with the runner's lock let go
//!
//! [`Runner::work_bump`](super::Runner) settles under the lock what a bump needs next and returns
//! it as an [`Act`], [`Act::run`] makes the call, and [`Runner::apply_act`](super::Runner) keeps
//! what came of it and returns the act after, if there is one. Each call waits its turn behind the
//! login's pace, so a poll that made them with the lock held stalled the loop behind it.

use super::{DEPENDENCIES_LABEL, Why};
use crate::board::TRIAGE;
use crate::runner::follow_up::TRIAGE_LABEL;
use crate::runner::voice::Writer;
use crate::settings::Dependencies;
use crate::state::Bump;

/// One write a bump needs
pub(super) enum Act {
    /// Asks the bump's bot to rebase it
    Rebase { bump: Bump, why: Why },
    /// Merges the bump, which `behind` names the rule that let it through if it was behind `main`
    Merge {
        bump: Bump,
        behind: Option<&'static str>,
    },
    /// Comments on a red bump that it is being closed
    Comment { bump: Bump, body: String },
    /// Closes a red bump
    Close { bump: Bump },
    /// Files the issue, or on the local store the ticket, for a closed red bump
    File {
        bump: Bump,
        title: String,
        body: String,
        local: bool,
    },
}

/// What a write came to when it went through
pub(super) enum Made {
    /// The id of the comment
    Comment(u64),
    /// Nothing to keep
    Done,
    /// What was filed, as the log says it
    Filed(String),
}

/// A write and what came of it
pub(super) struct Done {
    pub(super) act: Act,
    /// Why the forge refused it, if it did
    pub(super) result: Result<Made, String>,
}

impl Act {
    /// The pull request the write is for
    pub(super) fn number(&self) -> u64 {
        match self {
            Self::Rebase { bump, .. }
            | Self::Merge { bump, .. }
            | Self::Comment { bump, .. }
            | Self::Close { bump }
            | Self::File { bump, .. } => bump.pull_request,
        }
    }

    /// Makes the call
    pub(super) fn run(self, hands: &Writer) -> Done {
        let (forge, repo) = (hands.forge(), hands.remote());
        let number = self.number();
        let result = match &self {
            Self::Rebase { bump, .. } => {
                let mention = format!("@{} rebase", Dependencies::mention(&bump.author));
                (forge.comment(repo, number, &mention))
                    .map(Made::Comment)
                    .map_err(|e| e.to_string())
            }
            Self::Merge { bump, .. } => (forge.merge(repo, number, &bump.head))
                .map(|()| Made::Done)
                .map_err(|e| e.to_string()),
            Self::Comment { body, .. } => (forge.comment(repo, number, body))
                .map(Made::Comment)
                .map_err(|e| e.to_string()),
            Self::Close { .. } => (forge.close_pull_request(repo, number))
                .map(|()| Made::Done)
                .map_err(|e| e.to_string()),
            Self::File {
                title, body, local, ..
            } => file(hands, title, body, *local).map(Made::Filed),
        };
        Done { act: self, result }
    }
}

// Files the issue for a closed bump. A project on the local store files a ticket in the store
// instead, and makes no label on its repo.
fn file(hands: &Writer, title: &str, body: &str, local: bool) -> Result<String, String> {
    let labels = [TRIAGE, DEPENDENCIES_LABEL.name];
    match local {
        true => (hands.file_ticket(title, body, &labels))
            .map(|ticket| format!("ticket {ticket}"))
            .map_err(|e| e.to_string()),
        false => (hands.make_missing_labels([TRIAGE_LABEL, DEPENDENCIES_LABEL])).and_then(|()| {
            (hands.open_issue(title, body, &labels))
                .map(|issue| format!("issue #{issue}"))
                .map_err(|e| e.to_string())
        }),
    }
}
