//! What `status` shows of the work a runner has open

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use shep_kelpie_core::ports::{AuthorAssociation, SessionId, Timestamp, Wait};
use shep_kelpie_core::settings::AgentName;
use shep_kelpie_core::state::{Contribution, Triaged};
use shep_kelpie_core::ticket::TicketId;

/// A worker's turn in flight, as `status` shows it while it waits for its model
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Waiting {
    /// When it began to wait
    pub since: Timestamp,
    /// What it waits for
    pub why: Wait,
}

/// A due turn that has not started, as `status` shows it
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldUp {
    /// When the turn was first held up, unbroken since
    pub since: Timestamp,
    /// What it waits on
    pub because: String,
}

/// A work item's step that failed again and again, as `status` shows it
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failing {
    /// When the step first failed, unbroken since
    pub since: Timestamp,
    /// How many times it failed in a row
    pub failures: u32,
    /// Why it failed last
    pub reason: String,
}

/// A followed contribution, as `status` shows it: the record, and its clock
/// while one runs
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Followed {
    /// The contribution as the state saved it
    #[serde(flatten)]
    pub contribution: Contribution,
    /// Its clock, left out while kelpie waits on no one
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock: Option<ClockStatus>,
}

/// How long a contribution has waited on its contributor, as `status` shows it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockStatus {
    /// Seconds kelpie has waited on the contributor
    pub waiting_for: u64,
    /// Whole days until the nudge, 0 once it is due
    pub days_to_nudge: u64,
    /// Whole days until the contribution counts as quiet, 0 once it does
    pub days_to_quiet: u64,
    /// What is due now, left out while nothing is
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<ContributionDue>,
}

/// What a contribution's wait has come to
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContributionDue {
    /// The nudge days have passed
    Nudge,
    /// The quiet days have passed
    Quiet,
}

/// A bump from a bot as `status` lists it
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BumpStatus {
    /// The pull request
    pub pull_request: u64,
    /// Its author's login
    pub author: String,
    /// Its head commit's hash
    pub head: String,
    /// Where it stands, in a line
    pub standing: String,
}

/// What `status` says of triage
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriageStatus {
    /// The untouched issues, oldest first
    pub due: Vec<DueStatus>,
    /// The issue the research call is on, while it runs
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running: Option<u64>,
    /// The issues kelpie has commented on, and how far each has got
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<Triaged>,
    /// The day's cap and how many of its places are taken, while triage is on
    /// and the cap is
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily: Option<DailyStatus>,
}

/// An untouched issue, as `status` lists it
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DueStatus {
    /// Its number
    pub issue: u64,
    /// Its title
    pub title: String,
    /// The login of whoever opened it, empty when their account is gone
    pub author: String,
    /// How the forge counts whoever opened it to the repo it is on, or none
    /// when the listing does not say
    pub author_association: Option<AuthorAssociation>,
    /// When it was opened
    pub created: Timestamp,
}

/// What the daily triage cap says, as `status` shows it
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DailyStatus {
    /// The triage calls the records place in the last 24 hours
    pub used: u32,
    /// `triage.per_day`
    pub cap: u32,
    /// When the next place opens, while the day's are all taken
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opens: Option<Timestamp>,
    /// How long from now that is, in seconds
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<u64>,
}

/// The project manager as `status` shows it
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PmStatus {
    /// Its agent
    pub agent: AgentName,
    /// Its session, once it has one
    pub session: Option<SessionId>,
    /// Its folder: the board as its last wake read it, and its notes
    pub folder: PathBuf,
    /// Whether a call of its is in flight
    pub answering: bool,
    /// Whether the maintainer's terminal holds its session, pausing its wakes
    pub attached: bool,
    /// Until when it is passed over after a call failed
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub down_until: Option<Timestamp>,
    /// What the maintainer told it that its next wake carries
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub told: Vec<String>,
    /// The ready issues it holds, each with the open work item it waits on
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub held: Vec<Held>,
    /// The last few worker questions it answered or passed on, newest last
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settled: Vec<Settled>,
    /// Its last message to the maintainer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
}

/// A ready issue the project manager holds back
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Held {
    /// The ready issue
    pub issue: TicketId,
    /// The open work item it waits on, or none for any work item closing
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<TicketId>,
}

/// A worker's question the project manager answered or passed on
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settled {
    /// The ruling the question raised
    pub ruling: TicketId,
    /// The work item's ticket
    pub issue: TicketId,
    /// What it did: `answered`, `passed on`, or for a follow-up ruling
    /// `filed` or `dropped`
    pub how: String,
    /// The choice it recommended, when it passed the question on with one
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommends: Option<String>,
    /// Why, in its words
    pub why: String,
}
