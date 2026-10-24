//! The reads a step asks for with the runner's lock let go
//!
//! A step that finds it needs a read returns `Begin::Reads` with a [`Job`], built from cloned
//! handles and owned ids. The rotation runs the job with the lock let go, takes the lock again,
//! keeps the answer in the pass's [`Reads`] and steps the work item again, which finds the
//! answer where it would have asked the forge. A read the store lacks is made in place.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::Runner;
use crate::board::OpenPullRequest;
use crate::ports::{FailedLogs, Forge, ForgeError, PullRequest, QueueStanding, Talk};
use crate::review_bot::{Activity, Bot, Profile};
use crate::runner::off_lock::ItemKey;
use crate::runner::snapshot::view_of;
use crate::settings::ForgeSlug;

/// What the pass has read for a step, which the step made again finds where it would have asked
/// the forge, as often as it asks, until the step ends
#[derive(Debug, Default)]
pub(in crate::runner) struct Reads {
    views: BTreeMap<u64, Result<PullRequest, ForgeError>>,
    activities: BTreeMap<(Bot, u64), Result<Activity, ForgeError>>,
    talks: BTreeMap<u64, Result<Talk, ForgeError>>,
    // By head, and whether they are the merge queue's runs
    logs: BTreeMap<(String, bool), Result<FailedLogs, ForgeError>>,
    standings: BTreeMap<u64, Result<QueueStanding, ForgeError>>,
    // The pull requests whose comments the step has handled, so a step made again does not
    // handle them twice
    talked: BTreeSet<u64>,
}

impl Reads {
    /// Forgets what was read, as a pass begins or ends or a step ends
    pub(in crate::runner) fn clear(&mut self) {
        self.views.clear();
        self.activities.clear();
        self.talks.clear();
        self.logs.clear();
        self.standings.clear();
        self.talked.clear();
    }

    /// Pull request `number`, as the step read it
    pub(in crate::runner) fn view(&self, number: u64) -> Option<Result<PullRequest, ForgeError>> {
        self.views.get(&number).cloned()
    }

    /// What `bot` has posted on pull request `number`, as the step read it
    pub(in crate::runner) fn activity(
        &self,
        bot: Bot,
        number: u64,
    ) -> Option<Result<Activity, ForgeError>> {
        self.activities.get(&(bot, number)).cloned()
    }

    /// What people wrote on pull request `number`, as the step read it
    pub(in crate::runner) fn talk(&self, number: u64) -> Option<Result<Talk, ForgeError>> {
        self.talks.get(&number).cloned()
    }

    /// The end of the failed runs' logs at `head`, as the step read them
    pub(in crate::runner) fn logs(
        &self,
        head: &str,
        merge_group: bool,
    ) -> Option<Result<FailedLogs, ForgeError>> {
        self.logs.get(&(head.to_owned(), merge_group)).cloned()
    }

    /// Where pull request `number` stands in the merge queue, as the step read it
    pub(in crate::runner) fn standing(
        &self,
        number: u64,
    ) -> Option<Result<QueueStanding, ForgeError>> {
        self.standings.get(&number).cloned()
    }

    /// Whether the step has handled the comments on pull request `number`
    pub(in crate::runner) fn talked(&self, number: u64) -> bool {
        self.talked.contains(&number)
    }

    /// Notes that the step has handled the comments on pull request `number`
    pub(in crate::runner) fn talked_on(&mut self, number: u64) {
        self.talked.insert(number);
    }
}

/// A read for a work item's step, which needs no lock
pub(in crate::runner) struct Job {
    key: ItemKey,
    read: Read,
}

enum Read {
    View(View),
    Activity(Posted),
    Talk(Written),
    Logs(Failed),
    Standing(Queued),
}

// The read `pull_request_view` makes, from the pass's listing or the pull request itself
struct View {
    forge: Arc<dyn Forge>,
    remote: ForgeSlug,
    number: u64,
    // Whether the pass has listed nothing yet, so the read lists first and hands the listing back
    lists: bool,
}

// What a review bot has posted on a pull request
struct Posted {
    forge: Arc<dyn Forge>,
    remote: ForgeSlug,
    number: u64,
    profile: Arc<dyn Profile>,
}

// What people wrote on a pull request
struct Written {
    forge: Arc<dyn Forge>,
    remote: ForgeSlug,
    number: u64,
}

// The end of the logs of the runs that failed at a head
struct Failed {
    forge: Arc<dyn Forge>,
    remote: ForgeSlug,
    head: String,
    merge_group: bool,
}

// Where a pull request stands in the merge queue
struct Queued {
    forge: Arc<dyn Forge>,
    remote: ForgeSlug,
    number: u64,
}

/// What a [`Job`] found, errors as the inline call returns them
pub(in crate::runner) enum Done {
    Activity {
        bot: Bot,
        number: u64,
        activity: Result<Activity, ForgeError>,
    },
    Talk {
        number: u64,
        talk: Result<Talk, ForgeError>,
    },
    Logs {
        head: String,
        merge_group: bool,
        logs: Result<FailedLogs, ForgeError>,
    },
    Standing {
        number: u64,
        standing: Result<QueueStanding, ForgeError>,
    },
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
            Read::Activity(Posted {
                forge,
                remote,
                number,
                profile,
            }) => Done::Activity {
                bot: profile.bot(),
                number,
                activity: forge.review_bot(&remote, number, profile.login()),
            },
            Read::Talk(Written {
                forge,
                remote,
                number,
            }) => Done::Talk {
                number,
                talk: forge.talk(&remote, number),
            },
            Read::Logs(Failed {
                forge,
                remote,
                head,
                merge_group,
            }) => Done::Logs {
                logs: forge.failed_run_logs(&remote, &head, merge_group),
                head,
                merge_group,
            },
            Read::Standing(Queued {
                forge,
                remote,
                number,
            }) => Done::Standing {
                number,
                standing: forge.merge_queue(&remote, number),
            },
        }
    }
}

impl Runner {
    // Pull request `number` as the pass read it for the step, errors included
    pub(in crate::runner) fn kept_view(
        &self,
        number: u64,
    ) -> Option<Result<PullRequest, ForgeError>> {
        self.reads.view(number)
    }

    // The job that reads pull request `number` for the focused work item's step, or none when
    // the pass's listing holds it, which `pull_request_view` serves with no call, or when no
    // pass runs to make the read and step again, as at a turn's end
    pub(in crate::runner) fn view_job(&self, number: u64) -> Option<Box<Job>> {
        if self.snapshot.serves(number) {
            return None;
        }
        let lists = self.snapshot.lists_next();
        self.job(|forge, remote| {
            Read::View(View {
                forge,
                remote,
                number,
                lists,
            })
        })
    }

    // The job that reads pull request `number` itself, whatever the pass has listed
    pub(in crate::runner) fn pull_request_job(&self, number: u64) -> Option<Box<Job>> {
        self.job(|forge, remote| {
            Read::View(View {
                forge,
                remote,
                number,
                lists: false,
            })
        })
    }

    // The job that reads what `bot` has posted on pull request `number`
    pub(in crate::runner) fn activity_job(&self, bot: Bot, number: u64) -> Option<Box<Job>> {
        let profile = self.profile(bot);
        self.job(|forge, remote| {
            Read::Activity(Posted {
                forge,
                remote,
                number,
                profile,
            })
        })
    }

    // The job that reads what people wrote on pull request `number`
    pub(in crate::runner) fn talk_job(&self, number: u64) -> Option<Box<Job>> {
        self.job(|forge, remote| {
            Read::Talk(Written {
                forge,
                remote,
                number,
            })
        })
    }

    // The job that reads the end of the failed runs' logs at `head`
    pub(in crate::runner) fn logs_job(&self, head: &str, merge_group: bool) -> Option<Box<Job>> {
        self.job(|forge, remote| {
            Read::Logs(Failed {
                forge,
                remote,
                head: head.to_owned(),
                merge_group,
            })
        })
    }

    // The job that reads where pull request `number` stands in the merge queue
    pub(in crate::runner) fn standing_job(&self, number: u64) -> Option<Box<Job>> {
        self.job(|forge, remote| {
            Read::Standing(Queued {
                forge,
                remote,
                number,
            })
        })
    }

    // A job for the focused work item's step, or none when no pass runs to make the read and step
    // again, as at a turn's end
    fn job(&self, read: impl FnOnce(Arc<dyn Forge>, ForgeSlug) -> Read) -> Option<Box<Job>> {
        if !self.snapshot.in_pass() {
            return None;
        }
        let key = ItemKey::of(self.current()?);
        let read = read(Arc::clone(&self.ports.forge), self.remote.clone());
        Some(Box::new(Job { key, read }))
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
            Done::Activity {
                bot,
                number,
                activity,
            } => {
                if let Ok(activity) = &activity {
                    self.note_quota(bot, activity);
                }
                self.reads.activities.insert((bot, number), activity);
            }
            Done::Talk { number, talk } => {
                self.reads.talks.insert(number, talk);
            }
            Done::Logs {
                head,
                merge_group,
                logs,
            } => {
                self.reads.logs.insert((head, merge_group), logs);
            }
            Done::Standing { number, standing } => {
                self.reads.standings.insert(number, standing);
            }
        }
    }
}
