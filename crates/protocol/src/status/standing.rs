//! What `status` shows of the runner's own state

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use shep_kelpie_core::pacer::{Hold, Reading};
use shep_kelpie_core::ports::{Gpu, Timestamp};
use shep_kelpie_core::settings::{Account, Decide};
use shep_kelpie_core::ticket::TicketId;

/// What `status` shows of the pacer
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PacerStatus {
    /// Whether the limits hold anything
    pub enabled: bool,
    /// Each account the worker spends or a reading was taken of, by its name
    #[serde(flatten)]
    pub accounts: BTreeMap<Account, AccountStatus>,
}

/// What `status` shows of one account's usage
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountStatus {
    /// The last time its usage was read, and what it makes of it
    pub reading: Option<Reading>,
    /// Why nothing new is starting on it, while a hold lasts
    pub holding: Option<Hold>,
}

/// The GPU, as the metrics page last said
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuStatus {
    /// Each GPU on the page
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gpus: Vec<Gpu>,
    /// Why the page could not be read, or that it has not been yet
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Seconds since that read ended, once there has been one
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age_seconds: Option<u64>,
}

/// What `status` shows while the runner drains
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draining {
    /// The calls still running: each work item's, then the project manager's
    pub calls: Vec<CallRunning>,
    /// The longest a call the runner starts may run before it is ended, in
    /// seconds
    pub ceiling: u64,
    /// The work items whose worker turn ends at its next tool call
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub at_tool: Vec<TicketId>,
}

/// A call in flight
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallRunning {
    /// The ticket of the work item it is for, and none for the project manager's
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<TicketId>,
    /// Whose call it is
    pub role: CallRole,
}

/// Whose call is in flight, as `status` names it
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallRole {
    /// A worker's turn
    Worker,
    /// A reviewer's session or local round
    Reviewer,
    /// The project manager's wake
    Pm,
    /// The issue writer, labelling an issue
    IssueWriter,
    /// The security review, reading a contribution's head
    Security,
    /// The triager, researching an untouched issue
    Triager,
    /// The panel's experts, voting on a ruling
    Panel,
    /// The project manager's agent, running a batch retro
    Retro,
    /// The e2e run of a dependency bump's head
    E2e,
    /// A session in the maintainer's terminal, as its harness's facts name it
    Attached(String),
}

impl CallRole {
    /// The name `status` shows
    pub fn name(&self) -> String {
        match self {
            Self::Worker => "worker".to_owned(),
            Self::Reviewer => "reviewer".to_owned(),
            Self::Pm => "pm".to_owned(),
            Self::IssueWriter => "issue_writer".to_owned(),
            Self::Security => "security".to_owned(),
            Self::Triager => "triager".to_owned(),
            Self::Panel => "panel".to_owned(),
            Self::Retro => "retro".to_owned(),
            Self::E2e => "e2e".to_owned(),
            Self::Attached(name) => name.clone(),
        }
    }
}

impl Serialize for CallRole {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.name())
    }
}

impl<'de> Deserialize<'de> for CallRole {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match String::deserialize(deserializer)?.as_str() {
            "worker" => Self::Worker,
            "reviewer" => Self::Reviewer,
            "pm" => Self::Pm,
            "issue_writer" => Self::IssueWriter,
            "security" => Self::Security,
            "triager" => Self::Triager,
            "panel" => Self::Panel,
            "retro" => Self::Retro,
            "e2e" => Self::E2e,
            other => Self::Attached(other.to_owned()),
        })
    }
}

/// The reason the stored settings were refused, and since when
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    /// What did not hold
    pub reason: String,
    /// When it was first refused, in seconds since the Unix epoch
    pub at: Timestamp,
}

/// A workflow red on `main`, as `status` shows it
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedStatus {
    /// The workflow's name
    pub workflow: String,
    /// The forge's id for the first red run kelpie read since it last read the
    /// workflow green
    pub first_run: u64,
    /// The forge's id for its latest red run
    pub run: u64,
    /// The commit the latest red run ran on
    pub head: String,
    /// When `first_run` was created
    pub since: Timestamp,
    /// How long it has been red, in seconds
    pub red_for: u64,
    /// The latest red run's page on the forge
    pub url: String,
    /// What is being done about the latest red head
    pub doing: String,
}

/// How the panel stands, as `status` reports it
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Standing {
    /// Whether the panel decides or only votes
    pub decide: Decide,
    /// Rulings the panel has voted on, whoever answered them
    pub voted: usize,
    /// Rulings the panel decided
    pub decided: usize,
    /// Rulings the panel voted on that the maintainer answered
    pub answered: usize,
    /// The run of those, latest last, where the panel's recommendation was the
    /// maintainer's answer
    pub streak: u32,
    /// For each kind of ruling, the run where the project manager's own vote
    /// was the maintainer's answer
    pub pm_alone: BTreeMap<String, u32>,
    /// Each expert's votes against the maintainer's answers
    pub experts: BTreeMap<String, Expert>,
    /// What each panel, asked alone, would have decided against the answers
    pub panels: BTreeMap<String, Panel>,
    /// What each candidate policy for who votes would have done, by its name
    pub whatif: BTreeMap<String, Score>,
    /// Whether the panel's facts entry is due
    pub facts_due: bool,
    /// Rulings answered that the panel cast no vote on, by the reason
    pub skipped: BTreeMap<String, usize>,
}

/// One expert's votes against the maintainer's answers
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Expert {
    /// Votes it gave on rulings the maintainer answered
    pub votes: u32,
    /// Times it abstained
    pub abstained: u32,
    /// Times it was ended because the maintainer answered before its vote
    pub overtaken: u32,
    /// Votes that were the maintainer's answer
    pub agreed: u32,
    /// Times it was the only one whose vote was the maintainer's answer
    pub lone: u32,
    /// `agreed` and `lone` by kind of ruling
    pub by_kind: BTreeMap<String, Agreement>,
}

/// How often an expert agreed with the maintainer on one kind of ruling
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agreement {
    /// Votes that were the maintainer's answer
    pub agreed: u32,
    /// Times it was the only one whose vote was the maintainer's answer
    pub lone: u32,
}

/// What a panel would have decided, asked alone, against the answers
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Panel {
    /// Rulings it would have decided
    pub decided: u32,
    /// Of those, the ones it decided as the maintainer answered
    pub matched: u32,
}

/// What a policy for who votes would have done over the rulings answered
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Score {
    /// Rulings it would have decided without the maintainer
    pub decided: u32,
    /// Of those, the ones it decided as the maintainer answered
    pub matched: u32,
    /// Rulings it would have sent to the maintainer
    pub maintainer: u32,
    /// What it would have cost a ruling, in US dollars, or none where the
    /// ledger could not be read
    pub cost: Option<f64>,
}

/// Where a repo's pull request template comes from
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateSource {
    /// The repo's own
    Repo,
    /// The owner's `.github` repository, named here
    Owner(String),
    /// None: the worker writes the body with the `pr` skill
    None,
}

impl fmt::Display for TemplateSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repo => f.write_str("the repo's own"),
            Self::Owner(repo) => write!(f, "the owner's, from {repo}"),
            Self::None => f.write_str("none"),
        }
    }
}
