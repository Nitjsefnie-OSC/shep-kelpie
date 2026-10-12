//! The work item in flight, as the state file keeps it

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ports::{Cost, Finding, Role, SessionId, Timestamp, Usage};
use crate::review_bot::Bot;
use crate::settings::AgentName;

mod bots;
mod follow_ups;
mod heads;
mod known;
mod local;
mod review;
mod seat;
mod spend;
mod tally;
mod threads;
mod timings;

pub use bots::BotSkipped;
pub use follow_ups::FollowUps;
pub use known::{Known, foreign_change};
pub use local::LOCAL_FAILURES_DOWN;
pub use review::{Awaited, Review, ReviewStage};
pub use seat::Seat;
pub use spend::{QwenTally, RoleSpend, Spend};
pub use tally::{Counts, RoleTally, Tally};
pub use threads::ThreadAnswer;
pub use timings::{CallKind, Seconds, Split, TimingPhase, Timings};

/// The work item in flight
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkItem {
    /// The issue it resolves
    pub issue: u64,
    /// The issue's title when it was added
    pub title: String,
    /// Its branch, cut from `origin/main` unless it is a rework
    pub branch: String,
    /// Whether it reworks a pull request kelpie opened before, from its
    /// latest review. Its worktree starts at the branch's head on `origin`.
    #[serde(default)]
    pub rework: bool,
    /// Whether it adopts an open pull request kelpie didn't open. Its
    /// worktree starts at the branch's head on `origin`, and kelpie never
    /// rewrites the branch's commits.
    #[serde(default)]
    pub adopted: bool,
    /// An adopted pull request's head as it arrived, which its review diffs
    /// against instead of `origin/main`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrived: Option<String>,
    /// The listed review bots that owe an adopted pull request a review
    /// kelpie summoned. Until a bot's lands, a review from before the
    /// adoption does not stand for that bot's read.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub summons_owed: BTreeSet<Bot>,
    /// An older file's mark that the listed review bots owe an adopted pull
    /// request a summon, which the runner's start reads into `summons_owed`
    #[serde(default, skip_serializing)]
    pub summon_owed: bool,
    /// Whether a work item from a file older than review bots in the pass,
    /// past its review and not yet read by a bot, owes the listed bots a
    /// pass before its merge, which CI's next green run starts
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bots_after_ci: bool,
    /// Whether kelpie caught the branch up with `main` since a review bot
    /// last answered a summon. A bot finds nothing new in a caught-up branch,
    /// so the next summon asks it for a full review.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rebased: bool,
    /// Whether it holds one of the slots `concurrency.active_items` sets, which a phase
    /// that calls a model needs
    #[serde(default, skip_serializing_if = "Seat::is_held")]
    pub seat: Seat,
    /// Rounds in a row that left files unreviewed, by the local reviewer that
    /// ran them: ones that reviewed nothing, and ones that left the same
    /// files unreviewed again. Whatever the cause, the script's own
    /// `not reviewed:` lines are all kelpie sees. The first failure of a
    /// At [`LOCAL_FAILURES_DOWN`] the review goes on without the reviewer
    /// for the rest of the work item. A round that left no file, or only new
    /// ones, unreviewed clears its reviewer's count.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub local_failures: BTreeMap<AgentName, u32>,
    /// The files the last local round that reviewed anything left
    /// unreviewed. The next local round replaces the list with what it leaves.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub local_unreviewed: Vec<String>,
    /// The local reviewer whose round left `local_unreviewed`. Only that
    /// reviewer leaving the same files again counts against it: another
    /// reviewer's first miss on them is its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_unreviewed_by: Option<AgentName>,
    /// Its worktree
    pub worktree: PathBuf,
    /// Its worker's build folder
    pub build: PathBuf,
    /// The implementer its worker runs on, chosen when it opened and kept
    /// until it ends. Each turn runs that agent's file as it then stands.
    pub agent: AgentName,
    /// Whether its issue's `agent:` label ends in `!`, which keeps it on
    /// `agent`: its first turn never falls back to another implementer
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    /// The worker's session, chosen before its first turn
    pub session: SessionId,
    /// Where the worker's turn stands
    pub turn: Turn,
    /// The draft pull request its worker opened, once kelpie has seen it
    pub pull_request: Option<u64>,
    /// Where it stands between the worker's turns and the merge
    #[serde(default)]
    pub phase: Phase,
    /// The head whose red CI run last went to the worker
    #[serde(default)]
    pub red_head: Option<String>,
    /// The conflict with `main` that last went to the worker
    #[serde(default)]
    pub conflict: Option<Conflict>,
    /// What phase to force once the turn now running ends, overriding the
    /// ordinary rule that a known pull request goes straight to CI. Set by
    /// a ruling's answer that needs the review to run again, or by a rework;
    /// cleared once applied.
    #[serde(default)]
    pub resume: Option<Phase>,
    /// Whether a review call is in flight
    #[serde(default)]
    pub review_call: ReviewCallState,
    /// The reads each review bot has made of its pull request, which a
    /// bot's `rounds` limits
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bot_reads: BTreeMap<Bot, u32>,
    /// The listed review bots this pass went on without, and why
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bots_skipped: Vec<BotSkipped>,
    /// The forge's ids of the review bot threads sent to the worker, in the
    /// order its findings file lists them, which kelpie replies on once the
    /// fix turn ends, and resolves when the worker fixed them
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub threads_sent: Vec<String>,
    /// What the worker's reply said of `threads_sent`, which kelpie replies
    /// with: a thread it did not mention has none
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub thread_answers: Vec<ThreadAnswer>,
    /// Steps in a row that could not answer `threads_sent`
    #[serde(default, skip_serializing_if = "is_zero")]
    pub resolve_failures: u32,
    /// The forge's ids of the review bot threads the maintainer merged past
    /// with a yes, which hold no later merge of this work item
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub threads_accepted: Vec<String>,
    /// The reviewers whose calls failed so often in a row that a pass went
    /// on without them
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reviewers_skipped: Vec<AgentName>,
    /// Why the last review pass ended with no reviewer having read the pull
    /// request, as a listed reviewer was down or failed. None once a later
    /// pass has a reviewer read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreviewed: Option<String>,
    /// The pull request's labels, ready state and head, as kelpie and its
    /// worker leave them. A mismatch at the gate is a change kelpie did not make.
    #[serde(default)]
    pub known: Known,
    /// The heads a review round read, oldest first
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reviewed_heads: Vec<String>,
    /// The heads kelpie sends to CI with no round's read, on purpose, oldest
    /// first: a fix turn's push, a pass the project's own list gives nobody
    /// to read, and kelpie's own catch-up of a head it vouches for
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sent_unread: Vec<String>,
    /// The heads a fix turn sent only nits pushed, oldest first. A bot that
    /// read the pull request before has its nits on one of these left open.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nit_fix_heads: Vec<String>,
    /// The head a merge ruling's `no` was about, while the fix it asked for
    /// goes to CI with no reviewer's read. The next merge ruling, on another
    /// head no round read, says that head is the fix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noted_from: Option<String>,
    /// The head a review bot's late round read once the pass had ended, while
    /// its fix goes to CI with no reviewer's read. The next merge ruling, on
    /// another head no round read, says that head is the late round's fix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub late_from: Option<String>,
    /// The head at which the maintainer accepted a change to `.claude` or
    /// `.mcp.json`. Later heads that leave those files as this one has them
    /// pass the gate, and so does a worktree holding this head's copies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_files_accepted: Option<String>,
    /// Its qwen rounds so far
    #[serde(default)]
    pub qwen: QwenTally,
    /// Whether the forge refused a merge under `auto` since the last
    /// ruling on one. A second refusal raises a `stuck` ruling for `merge-refused`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub merge_refused: bool,
    /// Whether a worker turn already ended with no pull request and no
    /// question, and kelpie sent the worker back once. The next such turn
    /// parks it on a ruling. Cleared when a question the worker asked is
    /// answered.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sent_back: bool,
    /// Whether a worker turn ended with uncommitted files and nothing
    /// pushed, and kelpie sent the worker back once to commit them. Cleared
    /// when the next turn ends, so a later turn gets its own chance.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub asked_to_commit: bool,
    /// The head the project's `handover` hooks last passed, so a turn that
    /// ends on the same head runs them no more
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handed_over: Option<String>,
    /// Whether a `handover` hook refused the branch and kelpie sent the
    /// worker back once to fix it. The next refusal parks the work item on
    /// a ruling. Cleared when a hook passes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub handover_refused: bool,
    /// Why the forge refused the pull request kelpie tried to open from the
    /// worker's `pull-request.md` (the guard, a `publish` hook or the forge
    /// itself), with how to fix the file where the worker can, until the next
    /// step tells the worker. Kept in the state, so a restart still names the
    /// refusal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_request_refused: Option<String>,
    /// The head of the last merge under `auto` that answered an error. A
    /// pull request later found merged at it is kelpie's merge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_tried: Option<String>,
    /// The merge queue is holding this pull request, from the pass that
    /// queued it until it merges or the queue removes it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_queued: Option<MergeQueued>,
    /// Every finding above a nit kelpie has sent the worker to fix, across
    /// rounds. Only one of these is filed when the worker defers it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub held: Vec<Finding>,
    /// The findings the worker left unfixed, read once the pull request
    /// merges. None until then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_ups: Option<FollowUps>,
    /// Whether its retro was started, which is never done twice: a retro
    /// cut short by a stop is not asked for again
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub retro: bool,
    /// The worker's last turn's closing message, on one line and cut short
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// The tokens the worker's session held at its last turn's last model
    /// request, where its harness reports it. Unknown after a compaction,
    /// until the next turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    /// Where its wall time went, from the runner that first loaded it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timings: Option<Timings>,
    /// The maintainer's `attach`, which holds it: no call starts for it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attached: Option<Attached>,
    /// Its review rounds, fix turns and rulings so far
    #[serde(default, skip_serializing_if = "Counts::is_zero")]
    pub counts: Counts,
    /// Every Claude call made for it, oldest first
    pub calls: Vec<CallRecord>,
}

/// The `attach` holding a work item, which holds while either of its
/// processes runs
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attached {
    /// The `attach` command itself
    pub by: Holder,
    /// The session it runs in the terminal, once it has started
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<Holder>,
    /// When it asked
    pub since: Timestamp,
}

/// A process, named so that another given its pid later is not it
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Holder {
    /// Its pid
    pub pid: u32,
    /// When it started, as `ps -o lstart=` prints it
    pub started: String,
}

/// A pull request kelpie put in the merge queue
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeQueued {
    /// How many removals the queue had made of it before kelpie queued it
    pub removals: u32,
    /// When kelpie queued it
    pub since: Timestamp,
}

/// A conflict with `main` that went to the worker as its next turn
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conflict {
    /// The branch's head when it conflicted
    pub head: String,
    /// The `origin/main` commit it conflicted with
    pub main: String,
    /// How many conflict turns this work item has had, this one included
    pub turns: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// Where a work item stands between the worker's turns and the merge
///
/// A work item saved before phases existed reads as [`Phase::Implement`],
/// so a restart never puts its pull request through the gate unasked.
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Phase {
    /// The worker's turns. One that ends with a pull request open starts the
    /// review.
    #[default]
    Implement,
    /// The review, between the draft pull request and CI
    Review(Review),
    /// Waiting for CI on the pull request's head
    Ci {
        /// The head kelpie last saw, once it has looked
        head: Option<String>,
        /// When kelpie first saw that head, or entered CI
        since: Timestamp,
    },
    /// Parked on a ruling
    Ruling {
        /// The ruling's id
        id: u64,
    },
    /// The maintainer said yes, or every gate passed under `auto`: merging this head
    Merge {
        /// The head the ruling, or the gate, was about
        head: String,
        /// When kelpie marked the draft ready, which can start a fresh CI run
        #[serde(default)]
        readied: Option<Timestamp>,
        /// Whether the gate started it under `auto`, with no ruling asked
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        auto: bool,
    },
    /// Removing the worktree, branch and build folder
    Done {
        /// Whether the pull request merged, so its branch on the forge goes too
        merged: bool,
        /// Whether its issue was closed while its branch held no commit, so
        /// it ends with no pull request and nothing to keep
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        closed: bool,
    },
}

/// Whether a review call is in flight
///
/// Recorded in state the way a running turn is: `drop` refuses while a
/// call is running, since it runs outside the runner's lock and a dropped
/// work item would leave nothing for the result to land on.
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ReviewCallState {
    /// Nothing is running
    #[default]
    Idle,
    /// A review call is running, started at this time
    Running {
        /// When it started
        since: Timestamp,
    },
}

impl WorkItem {
    /// Forgets the review bot threads sent to the worker, which no fix of
    /// this round will resolve now
    pub fn forget_threads(&mut self) {
        self.threads_sent.clear();
        self.thread_answers.clear();
        self.resolve_failures = 0;
    }

    /// Forgets what the last pass made of the review bots, as a new one
    /// starts, and the head of a late round's fix, which the pass reads
    pub fn new_pass(&mut self) {
        self.forget_threads();
        self.bots_skipped.clear();
        self.late_from = None;
    }

    /// Remembers findings sent to the worker, once each, but for nits: a
    /// nit the worker leaves is never filed as a follow-up
    pub fn record_held(&mut self, held: &[Finding]) {
        for finding in held.iter().filter(|f| !f.is_nit()) {
            if !self.held.iter().any(|known| known.is_same_as(finding)) {
                self.held.push(finding.clone());
            }
        }
    }

    /// Records the head kelpie's own catch-up with `main` pushed, which no
    /// review bot has read
    pub fn caught_up(&mut self, head: Option<String>) {
        self.known.head = head;
        self.rebased = true;
    }

    /// The commit its review diffs against: `origin/main`, or the head an
    /// adopted pull request arrived with, so none of that is reviewed
    pub fn review_base(&self) -> String {
        self.arrived
            .clone()
            .unwrap_or_else(|| format!("origin/{}", crate::worktree::BASE))
    }

    /// What its calls have cost so far
    pub fn cost(&self) -> Cost {
        Cost(self.calls.iter().map(|c| c.cost.0).sum())
    }

    /// Moves what `session` had cost as of its last recorded call up to `to`,
    /// for a call in it that the work item does not count, so the cost of the
    /// next call here leaves that one out
    pub fn advance_session_cost(&mut self, session: &SessionId, to: Cost) {
        let last = self.calls.iter_mut().rev().find(|c| &c.session == session);
        if let Some(last) = last.filter(|c| c.session_cost.0 < to.0) {
            last.session_cost = to;
        }
    }

    /// What `session` had cost as of its last recorded call
    pub fn session_cost(&self, session: &SessionId) -> Cost {
        let last = self.calls.iter().rev().find(|c| &c.session == session);
        last.map_or(Cost(0), |c| c.session_cost)
    }
}

/// A random version 4 UUID, which `claude --session-id` takes
pub fn new_session_id() -> io::Result<SessionId> {
    let mut b = [0u8; 16];
    fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    let (a, rest) = hex.split_at(8);
    let (b, rest) = rest.split_at(4);
    let (c, rest) = rest.split_at(4);
    let (d, e) = rest.split_at(4);
    Ok(SessionId(format!("{a}-{b}-{c}-{d}-{e}")))
}

/// Where the worker's turn stands
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase", deny_unknown_fields)]
pub enum Turn {
    /// The first turn waits for the project to run
    Due,
    /// A turn started and has not ended; after a restart, it is resumed
    Running {
        /// When it started
        since: Timestamp,
        /// The prompt a resumed turn was sent, which a turn that failed
        /// before the harness started never saw. A first turn has none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
    },
    /// A later turn waits to resume the session with this prompt
    Next {
        /// What the turn tells the worker
        prompt: String,
    },
    /// The turn ended and the worker waits for kelpie
    Ended {
        /// When it ended
        at: Timestamp,
    },
    /// The turn could not run, and waits for the maintainer
    Failed {
        /// When it failed
        at: Timestamp,
        /// Why
        reason: String,
    },
}

/// One Claude call made for a work item
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallRecord {
    /// The role it was made for
    pub role: Role,
    /// When it ended
    pub at: Timestamp,
    /// The session it ran in
    pub session: SessionId,
    /// What it used
    pub usage: Usage,
    /// What it cost: the change in its session's cost
    pub cost: Cost,
    /// What its session had cost when it ended
    pub session_cost: Cost,
    /// Whether its harness reported no cost, so `cost` is no measure of it
    #[serde(default, skip_serializing_if = "is_false")]
    pub unpriced: bool,
}

fn is_false(b: &bool) -> bool {
    !b
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::review_bot::Bot;
    use crate::test::a_work_item;

    #[test]
    fn the_format_is_pinned() {
        let mut item = a_work_item();
        item.reviewed_heads = vec!["c0ffee".into()];
        item.sent_unread = vec!["f1x".into()];
        item.nit_fix_heads = vec!["f1x".into()];
        item.late_from = Some("c0ffee".into());
        assert_eq!(
            serde_json::to_value(item).unwrap(),
            json!({
                "issue": 42,
                "title": "Add a thing",
                "branch": "kelpie/42",
                "rework": false,
                "adopted": false,
                "worktree": "/k/wt/shep/42",
                "build": "/k/targets/shep/42",
                "agent": "opus-high",
                "pinned": true,
                "session": "5e55",
                "turn": { "state": "running", "since": 9 },
                "pull_request": 51,
                "phase": { "state": "ci", "head": "c0ffee", "since": 11 },
                "red_head": "bad",
                "conflict": { "head": "c0ffee", "main": "a11ce", "turns": 1 },
                "resume": null,
                "review_call": { "state": "idle" },
                "known": { "labels": ["review please"], "ready": false },
                "reviewed_heads": ["c0ffee"],
                "sent_unread": ["f1x"],
                "nit_fix_heads": ["f1x"],
                "late_from": "c0ffee",
                "qwen": { "rounds": 0, "seconds": 0 },
                "timings": {
                    "created": 5,
                    "since": 12,
                    "seconds": {
                        "worker": 4, "review": 0, "ci": 3,
                        "ruling": 0, "merge": 0, "other": 0,
                    },
                },
                "counts": { "review_rounds": 2, "fix_turns": 1, "rulings": 1, "ci_fix_turns": 1 },
                "calls": [{
                    "role": "worker",
                    "at": 10,
                    "session": "5e55",
                    "usage": { "input": 1, "cache_write": 2, "cache_read": 3, "output": 4 },
                    "cost": 5,
                    "session_cost": 6,
                }],
            })
        );
    }

    #[test]
    fn every_turn_state_is_pinned() {
        let value = |t: Turn| serde_json::to_value(t).unwrap();
        assert_eq!(value(Turn::Due), json!({ "state": "due" }));
        assert_eq!(
            value(Turn::Ended { at: Timestamp(3) }),
            json!({ "state": "ended", "at": 3 })
        );
        assert_eq!(
            value(Turn::Failed {
                at: Timestamp(4),
                reason: "no worktree".into()
            }),
            json!({ "state": "failed", "at": 4, "reason": "no worktree" })
        );
    }

    #[test]
    fn every_phase_is_pinned() {
        let value = |p: Phase| serde_json::to_value(p).unwrap();
        assert_eq!(value(Phase::Implement), json!({ "state": "implement" }));
        assert_eq!(
            value(Phase::Review(Review {
                round: 2,
                ..Review::first()
            })),
            json!({
                "state": "review",
                "round": 2,
                "stage": { "stage": "round" },
                "unread": true,
            })
        );
        let name = |n: &str| Some(AgentName::try_from(n.to_owned()).unwrap());
        assert_eq!(
            value(Phase::Review(Review {
                round: 3,
                reviewer: name("opus"),
                ran: name("qwen").into_iter().collect(),
                failures: 1,
                ..Review::first()
            })),
            json!({
                "state": "review",
                "round": 3,
                "stage": { "stage": "round" },
                "reviewer": "opus",
                "ran": ["qwen"],
                "failures": 1,
                "unread": true,
            })
        );
        let older = json!({
            "state": "review",
            "round": 3,
            "stage": { "stage": "round" },
            "last": "qwen",
        });
        let read: Phase = serde_json::from_value(older.clone()).unwrap();
        assert_eq!(value(read), older, "kept until the next round reads it");
        let bot = |stage: ReviewStage| {
            value(Phase::Review(Review {
                stage,
                reviewer: name("cubic"),
                bots_only: true,
                unread: false,
                ..Review::first()
            }))
        };
        assert_eq!(
            bot(ReviewStage::Summon {
                bot: Bot::Cubic,
                started: Timestamp(10),
                head: "c0ffee".into(),
                readied: None,
                full: true,
            }),
            json!({
                "state": "review",
                "round": 1,
                "stage": { "stage": "summon", "bot": "cubic", "started": 10, "head": "c0ffee", "full": true },
                "reviewer": "cubic",
                "bots_only": true,
            })
        );
        assert_eq!(
            bot(ReviewStage::Summoned {
                bot: Bot::Coderabbit,
                started: Timestamp(10),
                head: "c0ffee".into(),
                at: Timestamp(12),
                full: false,
                resent: true,
                failed: false,
            })["stage"],
            json!({ "stage": "summoned", "bot": "coderabbit", "started": 10, "head": "c0ffee", "at": 12, "resent": true })
        );
        assert_eq!(
            bot(ReviewStage::Summoned {
                bot: Bot::Codex,
                started: Timestamp(10),
                head: "c0ffee".into(),
                at: Timestamp(12),
                full: true,
                resent: false,
                failed: true,
            })["stage"],
            json!({ "stage": "summoned", "bot": "codex", "started": 10, "head": "c0ffee", "at": 12, "full": true, "failed": true })
        );
        let found = ReviewStage::Found {
            findings: Vec::new(),
            threads: vec!["PRRT_1".into()],
        };
        assert_eq!(
            bot(found)["stage"],
            json!({ "stage": "found", "findings": [], "threads": ["PRRT_1"] })
        );
        assert_eq!(
            bot(ReviewStage::Settling {
                bot: Bot::Cubic,
                since: Timestamp(20),
                read: Timestamp(40),
                open: vec!["PRRT_2".into()],
            })["stage"],
            json!({ "stage": "settling", "bot": "cubic", "since": 20, "read": 40, "open": ["PRRT_2"] })
        );
        assert_eq!(
            bot(ReviewStage::Pushing)["stage"],
            json!({ "stage": "pushing" })
        );
        assert_eq!(
            value(Phase::Ruling { id: 3 }),
            json!({ "state": "ruling", "id": 3 })
        );
        assert_eq!(
            value(Phase::Merge {
                head: "c0ffee".into(),
                readied: Some(Timestamp(12)),
                auto: false,
            }),
            json!({ "state": "merge", "head": "c0ffee", "readied": 12 })
        );
        assert_eq!(
            value(Phase::Merge {
                head: "c0ffee".into(),
                readied: None,
                auto: true,
            }),
            json!({ "state": "merge", "head": "c0ffee", "readied": null, "auto": true })
        );
        assert_eq!(
            value(Phase::Done {
                merged: true,
                closed: false
            }),
            json!({ "state": "done", "merged": true })
        );
        assert_eq!(
            value(Phase::Done {
                merged: false,
                closed: true
            }),
            json!({ "state": "done", "merged": false, "closed": true })
        );
        assert_eq!(
            serde_json::to_value(Turn::Next {
                prompt: "fix it".into()
            })
            .unwrap(),
            json!({ "state": "next", "prompt": "fix it" })
        );
    }

    #[test]
    fn review_call_state_is_pinned() {
        assert_eq!(
            serde_json::to_value(ReviewCallState::Idle).unwrap(),
            json!({ "state": "idle" })
        );
        assert_eq!(
            serde_json::to_value(ReviewCallState::Running {
                since: Timestamp(9)
            })
            .unwrap(),
            json!({ "state": "running", "since": 9 })
        );
    }

    #[test]
    fn a_work_item_saved_before_phases_reads_as_implementing() {
        let mut value = serde_json::to_value(a_work_item()).unwrap();
        let fields = value.as_object_mut().unwrap();
        fields.remove("phase");
        fields.remove("red_head");
        let item: WorkItem = serde_json::from_value(value).unwrap();
        assert_eq!((item.phase, item.red_head), (Phase::Implement, None));
    }

    #[test]
    fn the_review_diffs_from_main_unless_a_pull_request_arrived_at_a_head() {
        let mut item = a_work_item();
        assert_eq!(item.review_base(), "origin/main");
        item.arrived = Some("c0ffee".into());
        assert_eq!(item.review_base(), "c0ffee");
    }

    #[test]
    fn a_work_item_saved_before_reworks_and_adoptions_is_neither() {
        let mut value = serde_json::to_value(a_work_item()).unwrap();
        let fields = value.as_object_mut().unwrap();
        fields.remove("rework");
        fields.remove("adopted");
        let item: WorkItem = serde_json::from_value(value).unwrap();
        assert!(!item.rework && !item.adopted);
    }

    #[test]
    fn a_refused_merge_is_saved_only_while_it_stands() {
        let mut item = a_work_item();
        let value = serde_json::to_value(&item).unwrap();
        assert!(value.get("merge_refused").is_none(), "{value}");
        assert!(
            !serde_json::from_value::<WorkItem>(value)
                .unwrap()
                .merge_refused
        );
        item.merge_refused = true;
        let value = serde_json::to_value(&item).unwrap();
        assert_eq!(value["merge_refused"], true);
        assert!(
            serde_json::from_value::<WorkItem>(value)
                .unwrap()
                .merge_refused
        );
    }

    #[test]
    fn session_ids_are_distinct_version_4_uuids() {
        let a = new_session_id().unwrap().0;
        let b = new_session_id().unwrap().0;
        assert_ne!(a, b);
        let groups: Vec<_> = a.split('-').map(str::len).collect();
        assert_eq!(groups, [8, 4, 4, 4, 12]);
        assert_eq!(&a[14..15], "4");
        assert!(matches!(&a[19..20], "8" | "9" | "a" | "b"), "{a}");
    }

    #[test]
    fn a_calls_cost_is_measured_from_its_own_sessions_last_call() {
        let mut item = a_work_item();
        let other = SessionId("0th3r".into());
        let mut call = item.calls[0].clone();
        call.session = other.clone();
        call.session_cost = Cost(40);
        call.cost = Cost(40);
        item.calls.push(call);
        assert_eq!(item.session_cost(&item.session), Cost(6));
        assert_eq!(item.session_cost(&other), Cost(40));
        assert_eq!(item.session_cost(&SessionId("new".into())), Cost(0));
        assert_eq!(item.cost(), Cost(45));
    }

    #[test]
    fn a_call_the_item_does_not_count_moves_its_sessions_cost_without_adding_to_its_own() {
        let mut item = a_work_item();
        let session = item.session.clone();
        let before = item.cost();

        item.advance_session_cost(&session, Cost(30));
        item.advance_session_cost(&session, Cost(10));
        item.advance_session_cost(&SessionId("new".into()), Cost(99));

        assert_eq!(item.session_cost(&session), Cost(30));
        assert_eq!(item.cost(), before);
        let next = item.record_call(
            Role::Worker,
            Timestamp(9),
            session,
            Usage::default(),
            Some(Cost(35)),
        );
        assert_eq!(next, Some(Cost(5)));
    }
}
