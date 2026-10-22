//! The project's CI and how many work items run at once,
//! `[app.dogs.kelpie.ci]` and `[app.dogs.kelpie.concurrency]`

use std::num::NonZeroU32;

use schemars::JsonSchema;
use serde::Deserialize;

/// The project's CI: whether a merge waits for it, and how many times the
/// worker is sent back to fix a red run
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "CI")]
pub struct Ci {
    /// Whether the repo runs CI on pull requests, which a merge waits for
    ///
    /// On, a pull request with no checks yet waits for them, a merge waits
    /// for green, and no reviewer reads a head until CI has passed on it,
    /// so CI must run on the draft pull request too. Off, kelpie reads no
    /// checks at all, asks for the merge once the branch has the latest
    /// `main`, and starts the review at once, and `fix_attempts` does not
    /// apply.
    pub block: bool,
    /// How many fix turns the worker gets on red runs for one work item,
    /// before kelpie raises a ruling instead
    ///
    /// 0 raises the ruling on the first red run, and -1 sets no cap. A head
    /// red twice with nothing pushed raises it whatever this is. The fix
    /// turn for a pull request the merge queue removed is not counted. -1
    /// when absent.
    #[serde(default)]
    pub fix_attempts: FixAttempts,
    /// How many minutes a check may sit queued before kelpie reruns its
    /// workflow run
    ///
    /// GitHub sometimes leaves one job of a finished run queued, and it never
    /// starts. A work item's check still queued after this long, counted from
    /// when kelpie began waiting on the head, gets its whole run rerun once,
    /// and the count starts again from the rerun. Still queued after that,
    /// kelpie raises a `check-queued` ruling. A check that started is never
    /// rerun for being slow. 60 when absent.
    #[serde(default = "default_queued_minutes")]
    pub queued_minutes: NonZeroU32,
    /// A command that runs one failing test a reviewer wrote, with the test's
    /// path at `{test}` or its file stem at `{name}`, such as
    /// `cargo test --test {name}`
    ///
    /// Set, a reviewer on Claude Code is asked to leave a failing test for each
    /// finding it can, in a folder it may write. Absent, no reviewer is asked
    /// for one.
    #[serde(default)]
    pub test_command: Option<TestCommand>,
}

/// An hour, which no job of a healthy run waits for a runner past
fn default_queued_minutes() -> NonZeroU32 {
    NonZeroU32::new(60).expect("60 is not 0")
}

/// A command that runs one test file: it holds `{test}`, the file's path in
/// the repo, or `{name}`, the file's name without its extension
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(try_from = "String")]
// schemars describes a `try_from` type by its source, so the bound goes here.
#[schemars(extend("pattern" = "\\{(test|name)\\}"))]
pub struct TestCommand(String);

impl TestCommand {
    /// The command as written
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for TestCommand {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.contains("{test}") || value.contains("{name}") {
            true => Ok(Self(value)),
            false => Err("must hold `{test}`, the test file's path, or `{name}`, its file stem"),
        }
    }
}

/// A cap on the worker's fix turns for red runs: a count from 0, or -1 for none
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(try_from = "i64")]
// schemars describes a `try_from` type by its source, so the bounds go here.
#[schemars(extend("minimum" = -1, "maximum" = u32::MAX, "default" = -1))]
pub struct FixAttempts(Option<u32>);

impl FixAttempts {
    /// The most fix turns allowed, or `None` for no cap
    #[inline]
    pub fn cap(self) -> Option<u32> {
        self.0
    }
}

impl TryFrom<i64> for FixAttempts {
    type Error = &'static str;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            -1 => Ok(Self(None)),
            n => u32::try_from(n)
                .map(|n| Self(Some(n)))
                .map_err(|_| "must be -1 for no cap, or a count from 0"),
        }
    }
}

/// How many work items run at once, and how many may wait on rulings
/// before the board opens no more
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Concurrency")]
pub struct Concurrency {
    /// How many work items may hold a slot, which a worker's turn or a
    /// review needs, each with its own branch, worktree and gates. One
    /// parked on a ruling gives its slot up. 1 when absent.
    #[serde(default = "default_active_items")]
    pub active_items: NonZeroU32,
    /// How many work items may wait parked on rulings before the board
    /// opens no new work item
    ///
    /// It stops new work items opening and is not a cap on rulings: items
    /// already working can still park past it, and a merged item on its
    /// follow-up ruling does not count. 0 opens nothing while any ruling
    /// waits. 2 when absent.
    #[serde(default = "default_pending_rulings")]
    pub pending_rulings: u32,
}

impl Default for Concurrency {
    fn default() -> Self {
        Self {
            active_items: default_active_items(),
            pending_rulings: default_pending_rulings(),
        }
    }
}

/// One work item at a time, as every project ran before several could
fn default_active_items() -> NonZeroU32 {
    NonZeroU32::MIN
}

/// Two items waiting on the maintainer, so one unanswered ruling cannot
/// stall a project and a run of them cannot open pull request after pull request
fn default_pending_rulings() -> u32 {
    2
}
