//! How large a Codex session's context is, read from its rollout
//!
//! `codex exec --json` reports a session's tokens as a running total, which
//! says what the session has spent and not what its next request carries.
//! The rollout Codex keeps for the session has a `token_count` event after
//! each model request, whose `last_token_usage` is that request's.

use std::ffi::OsString;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{AgentCall, AgentError, AgentReply, CODEX, Files, Spent, Thread};
use crate::ports::Usage;

/// How deep under its `sessions` folder Codex keeps a rollout: year, month, day
const DEPTH: usize = 3;

/// The input tokens of the last model request in the rollout of Codex session
/// `thread`, cached ones included, or `None` when the call left no rollout or
/// no request in it
pub(super) fn last_request_input(home: &Path, thread: &str) -> Option<u64> {
    Some(last_count(home, thread)?.last_token_usage.input_tokens)
}

/// The tokens the session `thread` has spent in all, as its rollout's last
/// `token_count` event ran them up, beside its last request's input
///
/// The rollout holds no count of cache writes, so those stay what `known` had.
pub(super) fn running_total(home: &Path, thread: &str, known: Spent) -> Option<(Spent, u64)> {
    let count = last_count(home, thread)?;
    let total = count.total_token_usage;
    let spent = Spent {
        input_tokens: total.input_tokens,
        cached_input_tokens: total.cached_input_tokens,
        cache_write_input_tokens: known.cache_write_input_tokens,
        output_tokens: total.output_tokens,
    };
    Some((spent, count.last_token_usage.input_tokens))
}

fn last_count(home: &Path, thread: &str) -> Option<Count> {
    let rollout = rollout_of(&home.join("sessions"), thread, DEPTH)?;
    let text = std::fs::read_to_string(rollout).ok()?;
    text.lines()
        .rev()
        .filter(|line| line.contains("token_count"))
        .find_map(count)
}

// The rollout is named for its thread: `rollout-<time>-<thread>.jsonl`.
fn rollout_of(dir: &Path, thread: &str, depth: usize) -> Option<PathBuf> {
    let suffix = format!("-{thread}.jsonl");
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find_map(|path| {
            if path.is_dir() {
                return rollout_of(&path, thread, depth.checked_sub(1)?);
            }
            let name = path.file_name()?.to_str()?;
            (name.starts_with("rollout-") && name.ends_with(&suffix)).then_some(path)
        })
}

// A `token_count` event's tokens, if the line is one and has usage: Codex
// also sends the event with no usage, to update the rate limits.
fn count(line: &str) -> Option<Count> {
    #[derive(Deserialize)]
    struct Line {
        #[serde(rename = "type")]
        kind: String,
        payload: Event,
    }
    #[derive(Deserialize)]
    struct Event {
        #[serde(rename = "type")]
        kind: String,
        info: Option<Count>,
    }
    let line: Line = serde_json::from_str(line).ok()?;
    (line.kind == "event_msg" && line.payload.kind == "token_count").then_some(line.payload.info?)
}

#[derive(Deserialize)]
struct Count {
    last_token_usage: Tokens,
    total_token_usage: Tokens,
}

#[derive(Deserialize)]
struct Tokens {
    input_tokens: u64,
    #[serde(default)]
    cached_input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

/// The `-c` options that make Codex compact the session at `window` tokens
/// of context, before a resumed turn and between its model requests
///
/// The turn after a `COMPACT` call has a limit one under the context the
/// session held, which Codex has already reached, so it compacts before it
/// runs the turn's prompt. The scope is the whole request, system prompt and
/// tools included, as `last_request_input` counts it.
pub(super) fn limit_args(window: Option<NonZeroU32>, resumed: Option<&Thread>) -> Vec<OsString> {
    let compact_now = resumed
        .filter(|t| t.compact)
        .and_then(|t| t.context)
        .and_then(|context| context.checked_sub(1))
        .filter(|limit| *limit > 0);
    let window = window.map(|w| u64::from(w.get()));
    let Some(limit) = [window, compact_now].into_iter().flatten().min() else {
        return Vec::new();
    };
    [
        super::config(
            "model_auto_compact_token_limit",
            i64::try_from(limit).unwrap_or(i64::MAX),
        ),
        super::config("model_auto_compact_token_limit_scope", "total"),
    ]
    .into_iter()
    .flat_map(|c| [OsString::from("-c"), c])
    .collect()
}

/// Answers a `COMPACT` call without running Codex, which has no command
/// for it in `codex exec`, and leaves the session to compact before its
/// next turn
pub(super) fn compact(call: &AgentCall) -> Result<AgentReply, AgentError> {
    let files = Files::of(call);
    let id = call.session.id();
    let Some(known) = super::thread(&files, id)? else {
        return Err(AgentError::NoSession(CODEX, id.clone()));
    };
    super::save_thread(
        &files,
        id,
        &Thread {
            compact: true,
            ..known
        },
    )?;
    Ok(AgentReply {
        session_id: id.clone(),
        text: String::new(),
        usage: Usage::default(),
        session_cost: None,
        context: None,
        compactions: 0,
    })
}
