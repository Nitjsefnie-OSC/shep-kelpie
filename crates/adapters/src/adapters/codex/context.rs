//! How large a Codex session's context is, read from its rollout
//!
//! `codex exec --json` reports a session's tokens as a running total, which
//! says what the session has spent and not what its next request carries.
//! The rollout Codex keeps for the session has a `token_count` event after
//! each model request, whose `last_token_usage` is that request's, and a
//! `token_usage_record` event whose `thread_token_usage` counts every request
//! the session made, a compaction's included.

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
    Some(
        last_count(&rollout_text(home, thread)?)?
            .last_token_usage
            .input_tokens,
    )
}

/// The tokens the session `thread` has spent in all, as its rollout's last
/// `token_usage_record` counts them, or `None` when it has none, or none that
/// reaches `reported`, the total the turn's own report gave
///
/// A compaction's own model request is in that count and not in the turn's
/// report or the `token_count` events. A running total never falls, so a
/// record under `reported` is an earlier turn's, left by a Codex that wrote
/// none for this one.
pub(super) fn thread_total(home: &Path, thread: &str, reported: Spent) -> Option<Spent> {
    let total = last_record(&rollout_text(home, thread)?)?;
    total.reaches(reported.into()).then(|| spent(total))
}

/// The tokens the session `thread` has spent in all, beside its last
/// request's input
///
/// The total is the last `token_usage_record`'s when it reaches the last
/// `token_count` event's, which a record from before it does not, and that
/// event's otherwise.
pub(super) fn running_total(home: &Path, thread: &str) -> Option<(Spent, u64)> {
    let text = rollout_text(home, thread)?;
    let count = last_count(&text)?;
    let total = last_record(&text)
        .filter(|record| record.reaches(count.total_token_usage))
        .unwrap_or(count.total_token_usage);
    Some((spent(total), count.last_token_usage.input_tokens))
}

fn spent(total: Tokens) -> Spent {
    Spent {
        input_tokens: total.input_tokens,
        cached_input_tokens: total.cached_input_tokens,
        cache_write_input_tokens: total.cache_write_input_tokens,
        output_tokens: total.output_tokens,
    }
}

fn rollout_text(home: &Path, thread: &str) -> Option<String> {
    let rollout = rollout_of(&home.join("sessions"), thread, DEPTH)?;
    std::fs::read_to_string(rollout).ok()
}

fn last_count(text: &str) -> Option<Count> {
    text.lines()
        .rev()
        .filter(|line| line.contains("token_count"))
        .find_map(count)
}

fn last_record(text: &str) -> Option<Tokens> {
    text.lines()
        .rev()
        .filter(|line| line.contains("token_usage_record"))
        .find_map(record)
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

// A `token_usage_record` line's thread total, if the line is one: a
// `compacted` line holds a record's usage too, under another name.
fn record(line: &str) -> Option<Tokens> {
    #[derive(Deserialize)]
    struct Line {
        #[serde(rename = "type")]
        kind: String,
        payload: Record,
    }
    #[derive(Deserialize)]
    struct Record {
        thread_token_usage: Tokens,
    }
    let line: Line = serde_json::from_str(line).ok()?;
    (line.kind == "token_usage_record").then_some(line.payload.thread_token_usage)
}

#[derive(Deserialize)]
struct Count {
    last_token_usage: Tokens,
    total_token_usage: Tokens,
}

#[derive(Deserialize, Clone, Copy)]
struct Tokens {
    input_tokens: u64,
    #[serde(default)]
    cached_input_tokens: u64,
    #[serde(default)]
    cache_write_input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

impl Tokens {
    fn reaches(self, other: Self) -> bool {
        self.input_tokens >= other.input_tokens
            && self.cached_input_tokens >= other.cached_input_tokens
            && self.cache_write_input_tokens >= other.cache_write_input_tokens
            && self.output_tokens >= other.output_tokens
    }
}

impl From<Spent> for Tokens {
    fn from(spent: Spent) -> Self {
        Self {
            input_tokens: spent.input_tokens,
            cached_input_tokens: spent.cached_input_tokens,
            cache_write_input_tokens: spent.cache_write_input_tokens,
            output_tokens: spent.output_tokens,
        }
    }
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
