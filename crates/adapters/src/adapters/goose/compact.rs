//! What goose reports of its context and its compactions, and the call that
//! compacts a session
//!
//! goose compacts a session by itself once, at the start of a turn, when the
//! last request's tokens pass `GOOSE_AUTO_COMPACT_THRESHOLD` of
//! `GOOSE_CONTEXT_LIMIT`. A call with a `compact_window` sets that fraction.
//! A call whose prompt is `COMPACT` sends goose's own `/compact` command, which
//! compacts and answers `Compaction complete`, and exits 0 either way.
//!
//! No `message` event carries tokens, so a worker's context is read after its
//! process ends, from `goose session list --format json`.

use std::ffi::OsString;
use std::process::Output;
use std::time::Duration;

use serde_json::Value;

use super::output::{self, Read, failed};
use super::{GOOSE, GooseCli};
use crate::adapters::harness::tail;
use crate::ports::{AgentCall, AgentError, AgentReply, COMPACT, Role, Session, SessionId};

/// What a compact call sends in place of `COMPACT`: goose's own command
const COMMAND: &str = "/compact";

/// What goose answers when `COMMAND` compacted the session
const COMPLETE: &str = "Compaction complete";

/// The variable that sets the fraction of the context at which goose compacts
const THRESHOLD: &str = "GOOSE_AUTO_COMPACT_THRESHOLD";

/// goose's own fraction, which a window past it leaves alone
const DEFAULT_THRESHOLD: u64 = 8_000;

/// A fraction's denominator: four decimals
const SCALE: u64 = 10_000;

/// How long `goose session list` may take, which reads a database and no model
const LIST_LIMIT: Duration = Duration::from_secs(30);

/// A compact call's arguments: goose resumes the session and runs its command
///
/// None for any other call. The command is no prompt, so the step skill's
/// mapping, the system text and the extensions are left out.
pub(super) fn argv(call: &AgentCall) -> Option<Vec<OsString>> {
    if call.prompt != COMPACT {
        return None;
    }
    let mut argv: Vec<OsString> = vec![
        "run".into(),
        "--no-profile".into(),
        "--output-format".into(),
        "stream-json".into(),
        format!("--text={COMMAND}").into(),
    ];
    if matches!(call.session, Session::Resume(_)) {
        argv.push("--resume".into());
    }
    argv.extend(["--name".into(), call.session.id().0.as_str().into()]);
    Some(argv)
}

/// The variable that makes goose compact at `window`, for a call that has one
///
/// goose compacts when the last request's tokens divided by the context pass
/// the fraction. The fraction is rounded up to four decimals, so goose never
/// compacts under the window. A window at or past goose's own 0.8 of the
/// context leaves it unset, and so does a call with no window or one that
/// compacts: goose reads a value of 1 or more as off, so none is ever set.
pub(super) fn threshold_var(call: &AgentCall, context: u32) -> Option<(String, OsString)> {
    let window = u64::from(call.compact_window?.get());
    let context = u64::from(context);
    if call.prompt == COMPACT || context == 0 || window * SCALE >= context * DEFAULT_THRESHOLD {
        return None;
    }
    let tenths = (window * SCALE).div_ceil(context);
    let fraction = format!("{}.{:04}", tenths / SCALE, tenths % SCALE);
    Some((THRESHOLD.to_owned(), fraction.into()))
}

/// A call's reply, read as a compaction's if `compacting`
///
/// The exit status and an unreachable model are read as for any goose call.
pub(super) fn read(
    compacting: bool,
    output: &Output,
    session: &Session,
    before: output::Spent,
) -> Read {
    let mut read = output::read(output, session, before);
    if compacting {
        read.reply = read.reply.and_then(|reply| compacted(reply, output));
    }
    read
}

// goose answers a failed compaction with the error's text and exits 0, so
// the reply is a success only when it says the session compacted.
fn compacted(reply: AgentReply, output: &Output) -> Result<AgentReply, AgentError> {
    match reply.text.trim() {
        COMPLETE => Ok(AgentReply {
            text: String::new(),
            context: None,
            ..reply
        }),
        "" => Err(AgentError::Unreadable(
            GOOSE,
            tail(&String::from_utf8_lossy(&output.stdout)).to_owned(),
        )),
        said => Err(failed(said)),
    }
}

impl GooseCli {
    /// `reply` with the tokens of its session's last request, for a worker's
    /// call that compacted nothing
    pub(super) fn with_context(&self, call: &AgentCall, reply: AgentReply) -> AgentReply {
        if call.role != Role::Worker || call.prompt == COMPACT {
            return reply;
        }
        let context = self.context_of(call).map_err(|why| {
            eprintln!(
                "kelpie: cannot read the context of goose session {}: {why}",
                call.session.id().0
            );
        });
        AgentReply {
            context: context.ok(),
            ..reply
        }
    }

    // Runs `goose session list` as the call's goose, in its sandbox and root. A
    // failed read is the caller's to log, never an error of the call.
    fn context_of(&self, call: &AgentCall) -> Result<u64, String> {
        let args = ["session", "list", "--format", "json"].map(OsString::from);
        let (mut command, _forwarder) = self
            .wrapped_command(call, None, args.into(), None)
            .map_err(|e| e.to_string())?;
        let output = (self.base.processes)
            .output_within(&mut command, LIST_LIMIT)
            .map_err(|e| format!("{e:?}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("it exited {}: {}", output.status, tail(&stderr)));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        input_tokens(&stdout, call.session.id())
    }
}

// The `usage.input_tokens` of the session `asked` in the list's JSON, or why
// there is none. kelpie names its session with `--name`, and goose picks an id
// of its own, so a session is found by either.
fn input_tokens(list: &str, asked: &SessionId) -> Result<u64, String> {
    let sessions: Vec<Value> = serde_json::from_str(list.trim())
        .map_err(|e| format!("its list is not JSON: {e}: {}", tail(list)))?;
    let named = |field: &str| {
        sessions
            .iter()
            .find(|s| s[field].as_str() == Some(asked.0.as_str()))
    };
    let session = named("id")
        .or_else(|| named("name"))
        .ok_or_else(|| format!("its list holds no session {}", asked.0))?;
    (session
        .pointer("/usage/input_tokens")
        .and_then(Value::as_u64))
    .ok_or_else(|| "its usage holds no input token count".to_owned())
}
