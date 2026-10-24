//! The reads a step asks for with the runner's lock let go
//!
//! A step that finds it needs a read returns `Begin::Reads` with a [`Job`], built from cloned
//! handles and owned ids. The rotation runs the job with the lock let go, takes the lock again,
//! keeps the answer in the pass's [`Reads`] and steps the work item again, which finds the
//! answer where it would have asked the forge. A read the store lacks is made in place.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::Runner;
use crate::board::OpenPullRequest;
use crate::ports::{Forge, ForgeError, PullRequest};
use crate::runner::off_lock::ItemKey;
use crate::runner::snapshot::view_of;
use crate::settings::ForgeSlug;

/// What the pass has read for its steps, which the next step of the same work item takes
#[derive(Debug, Default)]
pub(in crate::runner) struct Reads {
    views: BTreeMap<u64, Result<PullRequest, ForgeError>>,
}

impl Reads {
    /// Forgets what was read, as a pass begins or ends
    pub(in crate::runner) fn clear(&mut self) {
        self.views.clear();
    }
}

/// A read for a work item's step, which needs no lock
pub(in crate::runner) struct Job {
    key: ItemKey,
    read: Read,
}

enum Read {
    View(View),
}

// The read `pull_request_view` makes, from the pass's listing or the pull request itself
struct View {
    forge: Arc<dyn Forge>,
    remote: ForgeSlug,
    number: u64,
    // Whether the pass has listed nothing yet, so the read lists first and hands the listing back
    lists: bool,
}

/// What a [`Job`] found, errors as the inline call returns them
pub(in crate::runner) enum Done {
    View {
        number: u64,
        listed: Option<Result<Vec<OpenPullRequest>, ForgeError>>,
        view: Result<PullRequest, ForgeError>,
    },
}

impl Job {
    /// The work item as it stood when the job was built
    pub(in crate::runner) fn key(&self) -> &ItemKey {
        &self.key
    }

    /// Reads, in the order and with the calls the step would have made inline
    pub(in crate::runner) fn run(self) -> Done {
        match self.read {
            Read::View(View {
                forge,
                remote,
                number,
                lists,
            }) => {
                let listed = lists.then(|| forge.open_pull_requests(&remote));
                let served = listed.as_ref().and_then(|listed| view_of(listed, number));
                let view = match served {
                    Some(pr) => Ok(pr),
                    None => forge.pull_request(&remote, number),
                };
                Done::View {
                    number,
                    listed,
                    view,
                }
            }
        }
    }
}

impl Runner {
    // Pull request `number` as the pass read it for the step, errors included, taken once
    pub(in crate::runner) fn take_view(
        &mut self,
        number: u64,
    ) -> Option<Result<PullRequest, ForgeError>> {
        self.reads.views.remove(&number)
    }

    // The job that reads pull request `number` for the focused work item's step, or none when
    // the pass's listing holds it, which `pull_request_view` serves with no call, or when no
    // pass runs to make the read and step again, as at a turn's end
    pub(in crate::runner) fn view_job(&self, number: u64) -> Option<Box<Job>> {
        if !self.snapshot.in_pass() || self.snapshot.serves(number) {
            return None;
        }
        let key = ItemKey::of(self.current()?);
        let view = View {
            forge: Arc::clone(&self.ports.forge),
            remote: self.remote.clone(),
            number,
            lists: self.snapshot.lists_next(),
        };
        Some(Box::new(Job {
            key,
            read: Read::View(view),
        }))
    }

    // Keeps what a job found for the step that asked
    pub(in crate::runner) fn keep_job_read(&mut self, done: Done) {
        match done {
            Done::View {
                number,
                listed,
                view,
            } => {
                if let Some(listed) = listed {
                    self.snapshot.preload(listed);
                }
                self.reads.views.insert(number, view);
            }
        }
    }
}
