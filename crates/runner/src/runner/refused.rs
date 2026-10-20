//! A settings change the runner refused, and the maintainer being told
//!
//! The settings in effect stay as they were while the stored table is bad.
//! `status` shows the refusal while it stands, and the webhook is told once
//! per distinct reason.

use serde::Serialize;

use super::Runner;
use crate::ports::{Alert, Timestamp};

/// The reason the stored settings were refused, and since when
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Refusal {
    /// What did not hold
    pub reason: String,
    /// When it was first refused, in seconds since the Unix epoch
    pub at: Timestamp,
    /// Whether the webhook was told, or there is none to tell
    #[serde(skip)]
    told: bool,
}

impl Runner {
    /// Records that the stored settings are refused for `reason`, and tells
    /// the webhook the first time it sees that reason
    ///
    /// Asked again each wake while the refusal stands: a reason already seen
    /// keeps its time, and is posted again only if the last post failed.
    pub fn refuse(&mut self, reason: &str) {
        let now = self.ports.clock.now();
        let standing = self.refusal.take().filter(|r| r.reason == reason);
        let mut refusal = standing.unwrap_or_else(|| Refusal {
            reason: reason.to_owned(),
            at: now,
            told: false,
        });
        if !refusal.told {
            refusal.told = self.tell_refused(reason);
        }
        self.refusal = Some(refusal);
    }

    /// Forgets a refusal: the stored settings are in effect, or the same as before
    pub fn accept(&mut self) {
        self.refusal = None;
    }

    // Posts the refusal to the webhook, and whether it landed or there is none to post to.
    fn tell_refused(&mut self, reason: &str) -> bool {
        let Some(webhook) = self.webhook.clone() else {
            return true;
        };
        let project = self.project.as_str();
        let alert = Alert {
            title: format!("kelpie: {project} settings change refused"),
            text: format!("{reason}. {project} keeps its old settings until its table is fixed."),
            reply: None,
        };
        match self.ports.alerts.post(&webhook, &alert) {
            Ok(()) => true,
            Err(e) => {
                self.notes.push(format!(
                    "cannot post that a settings change was refused: {e}"
                ));
                false
            }
        }
    }
}
