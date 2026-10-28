//! The reads a step asks for with the runner's lock let go
//!
//! A step that finds it needs a read returns `Begin::Reads` with a [`Job`], built from cloned
//! handles and owned ids. The rotation runs the job with the lock let go, takes the lock again,
//! keeps the answer in the pass's [`Reads`] and steps the work item again, which finds the
//! answer where it would have asked the forge. A read the store lacks is made in place.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::Runner;
use super::end_reads::EndRead;
use crate::board::OpenPullRequest;
use crate::ports::{
    FailedLogs, Forge, ForgeError, InstallationToken, Issue, Meter, MeterError, PullRequest,
    QueueStanding, Talk, ThreadComment, Tickets, Timestamp, Utilization, Voice,
};
use crate::review_bot::{Activity, Bot, Profile};
use crate::runner::git_reads::{Found, Git, Kept};
use crate::runner::merge::call::{Marking, Merged, Merging};
use crate::runner::off_lock::ItemKey;
use crate::runner::report::Begin;
use crate::runner::snapshot::view_of;
use crate::runner::worktree_jobs::{Change, Changed};
use crate::settings::{Account, ForgeSlug};
use crate::state::StateError;
use crate::ticket::TicketId;
use crate::work_item::WorkItem;

/// What the pass has read for a step, which the step made again finds where it would have asked
/// the forge, as often as it asks, until the step ends
#[derive(Debug, Default)]
pub(in crate::runner) struct Reads {
    views: BTreeMap<u64, Result<PullRequest, ForgeError>>,
    // Read again after the step's other reads, so not the view the step began with
    rechecks: BTreeMap<u64, Result<PullRequest, ForgeError>>,
    activities: BTreeMap<(Bot, u64), Result<Activity, ForgeError>>,
    talks: BTreeMap<u64, Result<Talk, ForgeError>>,
    // By head, and whether they are the merge queue's runs
    logs: BTreeMap<(String, bool), Result<FailedLogs, ForgeError>>,
    standings: BTreeMap<u64, Result<QueueStanding, ForgeError>>,
    readied: BTreeMap<u64, Result<(), ForgeError>>,
    merged: BTreeMap<u64, Merged>,
    // The pull requests whose comments the step has handled, so a step made again does not
    // handle them twice
    talked: BTreeSet<u64>,
    // What a work item's turn reads before its call, by ticket
    starts: BTreeMap<TicketId, Started>,
    usage: BTreeMap<Account, Result<Utilization, MeterError>>,
    // What git said, the fetches and diffs that need no write access to the worktree
    pub(in crate::runner) git: Kept,
    // What a turn's end read, which `end_turn` takes
    pub(in crate::runner) ended: Option<EndRead>,
}

/// What a turn's start read, each as the inline call returned it
///
/// A read is missing when the job did not make it: a turn whose token or issue failed reads
/// nothing after, as the inline start would not have.
#[derive(Debug, Default, Clone)]
pub(in crate::runner) struct Started {
    token: Option<Result<Option<InstallationToken>, ForgeError>>,
    issue: Option<Result<Issue, ForgeError>>,
    comments: Option<Result<Vec<ThreadComment>, ForgeError>>,
}

impl Reads {
    /// Forgets what was read, as a pass begins or ends or a step ends
    pub(in crate::runner) fn clear(&mut self) {
        self.views.clear();
        self.rechecks.clear();
        self.activities.clear();
        self.talks.clear();
        self.logs.clear();
        self.standings.clear();
        self.readied.clear();
        self.merged.clear();
        self.talked.clear();
        self.starts.clear();
        self.usage.clear();
        self.git.clear();
        self.ended = None;
    }

    /// What `ticket`'s turn read before its call, as the step read it
    pub(in crate::runner) fn started(&self, ticket: TicketId) -> Started {
        self.starts.get(&ticket).cloned().unwrap_or_default()
    }

    /// Whether `ticket`'s turn has read its start, so this step is the one made again over it
    pub(in crate::runner) fn has_started(&self, ticket: TicketId) -> bool {
        self.starts.contains_key(&ticket)
    }

    /// Pull request `number`, as the step read it
    pub(in crate::runner) fn view(&self, number: u64) -> Option<Result<PullRequest, ForgeError>> {
        self.views.get(&number).cloned()
    }

    /// Pull request `number`, as the step read it again after its other reads
    pub(in crate::runner) fn recheck(
        &self,
        number: u64,
    ) -> Option<Result<PullRequest, ForgeError>> {
        self.rechecks.get(&number).cloned()
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

    /// `account`'s usage, as the step or the board's read read it
    pub(in crate::runner) fn usage(
        &self,
        account: Account,
    ) -> Option<Result<Utilization, MeterError>> {
        self.usage.get(&account).cloned()
    }

    /// Keeps `account`'s usage for the step
    pub(in crate::runner) fn keep_usage(
        &mut self,
        account: Account,
        usage: Result<Utilization, MeterError>,
    ) {
        self.usage.insert(account, usage);
    }

    /// Whether marking pull request `number` ready came to, as the step asked it
    pub(in crate::runner) fn readied(&self, number: u64) -> Option<Result<(), ForgeError>> {
        self.readied.get(&number).cloned()
    }

    /// What merging pull request `number` came to, as the step asked it
    pub(in crate::runner) fn merged(&self, number: u64) -> Option<Merged> {
        self.merged.get(&number).cloned()
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
    Ready(Marking),
    Merge(Merging),
    Start(Starting),
    Git(Git),
    Change(Change),
    Usage(Metered),
}

// The read `pull_request_view` makes, from the pass's listing or the pull request itself
struct View {
    forge: Arc<dyn Forge>,
    remote: ForgeSlug,
    number: u64,
    // Whether the pass has listed nothing yet, so the read lists first and hands the listing back
    lists: bool,
    // Whether the answer is kept apart from the view the step began with
    recheck: bool,
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

// What a turn reads before its call: the worker's token, then the issue it is first prompted with
// and the comments under it, each only where the turn asks for it
struct Starting {
    voice: Arc<dyn Voice>,
    tickets: Arc<dyn Tickets>,
    remote: ForgeSlug,
    ticket: TicketId,
    token: bool,
    issue: bool,
    comments: bool,
}

// An account's usage, which the pacer reads before a call on it starts
struct Metered {
    meter: Arc<dyn Meter>,
    account: Account,
    now: Timestamp,
}

/// What a [`Job`] found, errors as the inline call returns them
pub(in crate::runner) enum Done {
    Start {
        ticket: TicketId,
        started: Started,
    },
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
    Readied {
        number: u64,
        marked: Result<(), ForgeError>,
    },
    Merged {
        number: u64,
        merged: Merged,
    },
    View {
        number: u64,
        listed: Option<Result<Vec<OpenPullRequest>, ForgeError>>,
        view: Result<PullRequest, ForgeError>,
        recheck: bool,
    },
    Git(Found),
    Changed(Changed),
    Usage {
        account: Account,
        usage: Result<Utilization, MeterError>,
    },
}

impl Job {
    /// The work item as it stood when the job was built
    pub(in crate::runner) fn key(&self) -> &ItemKey {
        &self.key
    }

    /// Whether the job changes the work item's worktree, which a trigger on it then waits for
    pub(in crate::runner) fn changes_worktree(&self) -> bool {
        match &self.read {
            Read::Change(_) => true,
            Read::Git(git) => git.changes_worktree(),
            _ => false,
        }
    }

    /// Reads, in the order and with the calls the step would have made inline
    pub(in crate::runner) fn run(self) -> Done {
        match self.read {
            Read::View(View {
                forge,
                remote,
                number,
                lists,
                recheck,
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
                    recheck,
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
            Read::Ready(marking) => Done::Readied {
                number: marking.number,
                marked: marking.run(),
            },
            Read::Merge(merging) => Done::Merged {
                number: merging.number,
                merged: merging.run(),
            },
            Read::Start(starting) => Done::Start {
                ticket: starting.ticket,
                started: starting.run(),
            },
            Read::Git(git) => Done::Git(git.run()),
            Read::Change(change) => Done::Changed(change.run()),
            Read::Usage(Metered {
                meter,
                account,
                now,
            }) => Done::Usage {
                account,
                usage: meter.read(now),
            },
        }
    }
}

impl Starting {
    // In the order the inline start reads, and stopping where it would have: a token or an issue
    // that fails ends the turn's start before the next is read
    fn run(self) -> Started {
        let mut started = Started::default();
        if self.token {
            let token = self.voice.worker_token(&self.remote);
            let failed = token.is_err();
            started.token = Some(token);
            if failed {
                return started;
            }
        }
        if self.issue {
            let issue = self.tickets.issue(&self.remote, self.ticket);
            let failed = issue.is_err();
            started.issue = Some(issue);
            if failed {
                return started;
            }
        }
        if self.comments {
            started.comments = Some(self.tickets.issue_comments(&self.remote, self.ticket));
        }
        started
    }
}

impl Started {
    // Adds what a later job read to what the step already holds, so a job that asked for less
    // (the comments alone, once the maintainer can be named) does not drop what an earlier one read
    fn merge(&mut self, later: Started) {
        self.token = later.token.or(self.token.take());
        self.issue = later.issue.or(self.issue.take());
        self.comments = later.comments.or(self.comments.take());
    }

    /// The worker's token, as the job minted it
    pub(in crate::runner) fn token(&self) -> Option<Result<Option<InstallationToken>, ForgeError>> {
        self.token.clone()
    }

    /// The issue a first turn is prompted with, as the job read it
    pub(in crate::runner) fn issue(&self) -> Option<Result<Issue, ForgeError>> {
        self.issue.clone()
    }

    /// The comments on the issue, as the job listed them
    pub(in crate::runner) fn comments(&self) -> Option<Result<Vec<ThreadComment>, ForgeError>> {
        self.comments.clone()
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
                recheck: false,
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
                recheck: false,
            })
        })
    }

    // The job that reads pull request `number` itself again, for a step whose earlier reads
    // may have left its first view behind
    pub(in crate::runner) fn recheck_job(&self, number: u64) -> Option<Box<Job>> {
        self.job(|forge, remote| {
            Read::View(View {
                forge,
                remote,
                number,
                lists: false,
                recheck: true,
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

    // The job that marks pull request `number` ready
    pub(in crate::runner) fn ready_job(&self, number: u64) -> Option<Box<Job>> {
        self.job(|forge, remote| {
            Read::Ready(Marking {
                forge,
                remote,
                number,
            })
        })
    }

    // The job that merges pull request `number` at `head`
    pub(in crate::runner) fn merge_job(
        &self,
        number: u64,
        head: &str,
        auto: bool,
    ) -> Option<Box<Job>> {
        self.job(|forge, remote| {
            Read::Merge(Merging {
                forge,
                remote,
                number,
                head: head.to_owned(),
                auto,
            })
        })
    }

    // The job that reads what `item`'s turn needs before its call and the step has not read: the
    // worker's token, and for a first turn the issue and the comments under it. None when the
    // step has them all, or the turn fails before it asks for any, or no pass runs to make the
    // read and step again.
    pub(in crate::runner) fn start_job(&self, item: &WorkItem, first: bool) -> Option<Box<Job>> {
        let ticket = item.issue;
        let kept = self.reads.started(ticket);
        // A read that failed ends the turn's start there, as the inline one would.
        if matches!(kept.token, Some(Err(_))) || matches!(kept.issue, Some(Err(_))) {
            return None;
        }
        let token = kept.token.is_none();
        let issue = first && kept.issue.is_none();
        // A maintainer who cannot be named has no comments to read.
        let comments = first && kept.comments.is_none() && self.maintainer().is_ok();
        // A turn whose agent cannot run fails before it mints a token.
        if !(token || issue || comments) || self.worker_agent(item).is_err() {
            return None;
        }
        self.job(|_, remote| {
            Read::Start(Starting {
                voice: Arc::clone(&self.ports.voice),
                tickets: Arc::clone(&self.ports.tickets),
                remote,
                ticket,
                token,
                issue,
                comments,
            })
        })
    }

    // The job that reads from git for the focused work item's step
    pub(in crate::runner) fn git_job(&self, git: Git) -> Option<Box<Job>> {
        self.job(|_, _| Read::Git(git))
    }

    // The job that changes the focused work item's worktree
    pub(in crate::runner) fn change_job(&self, change: Change) -> Option<Box<Job>> {
        self.job(|_, _| Read::Change(change))
    }

    // The job that reads `account`'s usage at `now` for the focused work item's step
    pub(in crate::runner) fn usage_job(
        &self,
        account: Account,
        now: Timestamp,
    ) -> Option<Box<Job>> {
        let meter = Arc::clone(self.ports.meter_of(account));
        self.job(|_, _| {
            Read::Usage(Metered {
                meter,
                account,
                now,
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

    // Applies what a job found. A read is kept for the step that asked, which is made again, and
    // a change to the worktree finishes the step, whose end this returns.
    pub(in crate::runner) fn apply_job(&mut self, done: Done) -> Result<Option<Begin>, StateError> {
        if let Done::Changed(changed) = done {
            return self.changed(changed).map(Some);
        }
        self.keep_job_read(done);
        Ok(None)
    }

    // Keeps what a read job found for the step that asked
    fn keep_job_read(&mut self, done: Done) {
        match done {
            Done::Start { ticket, started } => {
                self.reads.starts.entry(ticket).or_default().merge(started);
            }
            Done::View {
                number,
                listed,
                view,
                recheck,
            } => {
                if let Some(listed) = listed {
                    self.snapshot.preload(listed);
                }
                let kept = match recheck {
                    true => &mut self.reads.rechecks,
                    false => &mut self.reads.views,
                };
                kept.insert(number, view);
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
            Done::Readied { number, marked } => {
                self.reads.readied.insert(number, marked);
            }
            Done::Merged { number, merged } => {
                self.reads.merged.insert(number, merged);
            }
            Done::Git(found) => self.reads.git.keep(found),
            Done::Usage { account, usage } => self.reads.keep_usage(account, usage),
            Done::Changed(_) => {}
        }
    }
}
