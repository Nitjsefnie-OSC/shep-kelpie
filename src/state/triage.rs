//! What kelpie keeps of triaging issues other people open

use serde::{Deserialize, Serialize};

use crate::ports::Timestamp;

/// Where triaging stands
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriageState {
    /// When `triage.pick` went past `off`: only an issue opened after it is
    /// triaged
    pub since: Timestamp,
    /// The issues triaged or being triaged, oldest first, the last [`TRIAGED_KEPT`]
    /// finished ones
    ///
    /// Left out while empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<Triaged>,
}

/// How many finished records are kept, the oldest going first. An issue whose
/// record went is told apart from an untouched one by its `needs-triage` label
/// and, if that is taken off, by the marker in its comments.
///
/// A record that closed an issue as a duplicate is kept past the count: the
/// issue carries no `needs-triage` label, and the record is what keeps a
/// reopened one out of the due list.
pub const TRIAGED_KEPT: usize = 200;

impl TriageState {
    /// Drops the oldest finished records past [`TRIAGED_KEPT`], keeping every
    /// one that closed an issue as a duplicate
    pub fn prune(&mut self) {
        let kept = |t: &Triaged| {
            matches!(t.step, TriageStep::Done | TriageStep::Ready) && t.duplicate_of.is_none()
        };
        let mut extra = self
            .issues
            .iter()
            .filter(|t| kept(t))
            .count()
            .saturating_sub(TRIAGED_KEPT);
        self.issues.retain(|t| {
            let drop = extra > 0 && kept(t);
            extra -= usize::from(drop);
            !drop
        });
    }
}

/// An issue whose triage comment kelpie has posted, or which a triage limit
/// held with no call at all
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Triaged {
    /// The issue's number
    pub issue: u64,
    /// The id of the comment kelpie posted on it, 0 when no comment was
    /// posted, which is what a limit's hold leaves
    pub comment: u64,
    /// How far the issue has got
    pub step: TriageStep,
    /// When the record was made: the moment its triage comment was recorded,
    /// or the moment a limit held the issue. What the daily and per-author
    /// caps count their 24 hours from. A record saved before it existed reads
    /// as 0, which is past any window.
    #[serde(default = "no_time")]
    pub at: Timestamp,
    /// The login of whoever opened the issue, which the per-author cap counts
    /// by, ignoring case. Empty for a deleted account, and for a record saved
    /// before the cap existed.
    ///
    /// Left out while empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author: String,
    /// The limit that held this issue, which kelpie labelled `needs-triage`
    /// with no call and no comment
    ///
    /// Left out unless a limit held the issue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<TriageHeld>,
    /// The labels the research asked for that are not all on yet, so that a
    /// restart at `commented` puts them on
    ///
    /// Left out while empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    /// The open issue or pull request this one was closed as a duplicate of,
    /// which the comment named before the close
    ///
    /// Left out unless the issue was closed as a duplicate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_of: Option<u64>,
    /// The body kelpie is owed to write: the agent-ready body over the
    /// issue's on the ready path, or the issue's body with the settled answers
    /// under its author's text on the answers path. Kept so a restart writes
    /// it and never writes it twice
    ///
    /// Left out unless an edit is owed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ready_body: Option<String>,
    /// The labels the issue carried when this pass began, kept so a pass that
    /// resumes after a restart still tells a triage label the maintainer put on
    /// from kelpie's own
    ///
    /// Left out while empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seen: Vec<String>,
    /// The ask kelpie's triage comment put to the author, while the triage
    /// waits for their word: `github` or `none`. No label goes on meanwhile.
    ///
    /// Left out unless the triage waits on the ask.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asked: Option<TriageAsked>,
    /// The questions comment kelpie posted for the author to answer, while the
    /// triage waits for the answers.
    ///
    /// Left out unless the triage waits on the answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub questions: Option<TriageQuestions>,
    /// The prompt kelpie posted for the author to run in their own terminal,
    /// while the triage waits for the body their session writes back. No label
    /// goes on meanwhile.
    ///
    /// Left out unless the triage waits on a local session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<TriageLocal>,
}

/// The ask kelpie waits an author's answer to
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriageAsked {
    /// When the ask kelpie waits on went up: the triage comment's, or the
    /// plain ask that followed a reply kelpie could not read as either word.
    /// Silence this long past it counts as `none`.
    pub at: Timestamp,
    /// How many times kelpie has asked: 1, or 2 once its plain ask has gone
    /// up. A second reply it can read as neither word counts as `none`.
    ///
    /// Left out while it has asked once.
    #[serde(default = "asked_once", skip_serializing_if = "is_once")]
    pub asks: u32,
}

fn asked_once() -> u32 {
    1
}

// A record saved before `at` existed has no time, which is past any cap's
// 24 hours, so no cap counts it.
fn no_time() -> Timestamp {
    Timestamp(0)
}

fn is_once(asks: &u32) -> bool {
    *asks <= 1
}

impl TriageAsked {
    /// The first ask, the one the triage comment ends with
    pub fn first(at: Timestamp) -> Self {
        Self { at, asks: 1 }
    }

    /// Whether kelpie has put its one plain ask again, so the next reply it
    /// cannot read counts as `none`
    pub fn asked_again(&self) -> bool {
        self.asks > 1
    }
}

/// The questions comment kelpie waits an author's answers to
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriageQuestions {
    /// The id of the comment holding them
    pub comment: u64,
    /// When it went up: the answers are the author's comments after it, and
    /// silence this long past it ends the round
    pub at: Timestamp,
}

/// The prompt kelpie waits an author's own session to answer
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriageLocal {
    /// When its prompt went up: silence this long past it, with no body edit,
    /// ends the round as `none`
    pub at: Timestamp,
    /// A hash of the issue body as kelpie last saw it. A body whose hash
    /// differs is one the author's session rewrote, and kelpie triages it
    /// again. Kelpie's own body edits set it, so its own write is never read
    /// as the author's
    pub body: u64,
}

/// How far an issue's triage has got, saved before the next write to the forge
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TriageStep {
    /// The comment is posted and the labels are not all on. On the ready path
    /// the `ready_body` edit is owed too
    Commented,
    /// The comment naming the other item is posted, and the close as a
    /// duplicate of `duplicate_of` is not
    Closed,
    /// The agent-ready body is written over the issue's and the type and
    /// priority labels are not all on
    BodyWritten,
    /// The answers round's comment is posted, and its settled answers are
    /// owed under the issue's author's text, in `ready_body`
    Answers,
    /// The labels the reply gave are on and `needs-triage` is not
    Labelled,
    /// `needs-triage` is on, or the maintainer's own triage label stood
    Done,
    /// `ready-for-agent` is on
    Ready,
}

/// Which triage limit held an issue, so it got `needs-triage` with no call and
/// no comment. The hold is final for that issue: the pass does not come back to
/// it, and `shep kelpie triage` can still be asked of it.
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TriageHeld {
    /// The author already had `triage.per_author_per_day` issues triaged in
    /// the 24 hours before it
    AuthorCap,
    /// The author's account is younger than `triage.min_account_days` and the
    /// forge counts them as having no history in the repo
    NewAccount,
}

impl TriageHeld {
    /// What `status` says of the issue's limit
    pub fn said(&self) -> &'static str {
        match self {
            Self::AuthorCap => "its author already had the day's triage calls",
            Self::NewAccount => "its author's account is new and has no history in the repo",
        }
    }
}
