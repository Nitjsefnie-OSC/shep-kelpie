//! What a call's transcript says of each request it made to the model: how
//! large its context was, how much of that it wrote to the cache, and what
//! came before it
//!
//! A transcript has one usage a request, where the ledger has one a call.
//! Claude Code writes a request as a line for each block of its reply, all
//! of them holding the request's usage, so they count once. A request is
//! `after` the session's start if it is the first the file holds, after a
//! gap if over five minutes passed since the request before it, which is
//! longer than the cache keeps what it wrote, and after growth otherwise.
//! The request before may be an earlier call's, so a resumed call's first
//! request reads the gap since its session last spoke. `docs/facts/transcript-requests.md`

use std::collections::BTreeSet;

use serde_json::Value;

// Milliseconds after which the cache has dropped what a request wrote
const GAP_MILLIS: i64 = 5 * 60 * 1000;

/// What came before a request
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum After {
    /// Nothing: the session's first request
    Start,
    /// A wait of over five minutes
    Gap,
    /// A request within five minutes
    Growth,
}

impl After {
    /// Every case, as the series name them
    pub(crate) const ALL: [Self; 3] = [Self::Start, Self::Gap, Self::Growth];

    /// The case's name in a series' `after` label
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Gap => "gap",
            Self::Growth => "growth",
        }
    }
}

/// One request to the model
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Request {
    /// What it sent: its input, and what it read from and wrote to the cache
    pub(crate) context: u64,
    /// What it wrote to the cache
    pub(crate) cache_write: u64,
    /// What came before it
    pub(crate) after: After,
}

/// The requests Claude Code's transcript `text` holds from byte `from` on,
/// where the call whose lines begin there made them
pub(crate) fn claude(text: &str, from: usize) -> Vec<Request> {
    let mut keys = BTreeSet::new();
    requests(text, from, |entry| {
        let message = &entry["message"];
        if entry["type"] != "assistant" || entry["isSidechain"] == true {
            return None;
        }
        let usage = &message["usage"];
        let used = |key: &str| usage[key].as_u64().unwrap_or(0);
        // One request is a line for each block of its reply.
        let key = (message["id"].as_str()).or_else(|| entry["requestId"].as_str());
        if key.is_some_and(|key| !keys.insert(key.to_owned())) {
            return None;
        }
        Some((
            used("input_tokens"),
            used("cache_read_input_tokens"),
            used("cache_creation_input_tokens"),
        ))
    })
}

/// The requests pi's session file `text` holds from byte `from` on, where
/// the call whose lines begin there made them
pub(crate) fn pi(text: &str, from: usize) -> Vec<Request> {
    requests(text, from, |entry| {
        let message = &entry["message"];
        if entry["type"] != "message" || message["role"] != "assistant" {
            return None;
        }
        let usage = &message["usage"];
        let used = |key: &str| usage[key].as_u64().unwrap_or(0);
        Some((used("input"), used("cacheRead"), used("cacheWrite")))
    })
}

// The requests of `text` from byte `from`, as `read` finds each line's (input, cache read, cache
// write), the first line of a request only. A request that sent nothing, as Claude Code's
// `<synthetic>` replies, is no request.
fn requests(
    text: &str,
    from: usize,
    mut read: impl FnMut(&Value) -> Option<(u64, u64, u64)>,
) -> Vec<Request> {
    // A file shorter than `from` is a new one.
    let from = if text.get(from..).is_some() { from } else { 0 };
    let mut offset = 0;
    let mut before: Option<Option<i64>> = None;
    let mut found = Vec::new();
    for line in text.split_inclusive('\n') {
        let at = offset;
        offset += line.len();
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some((input, cache_read, cache_write)) = read(&entry) else {
            continue;
        };
        let context = input.saturating_add(cache_read).saturating_add(cache_write);
        if context == 0 {
            continue;
        }
        let when = (entry["timestamp"].as_str())
            .and_then(|time| time.parse::<jiff::Timestamp>().ok())
            .map(jiff::Timestamp::as_millisecond);
        let after = match (before, when) {
            (None, _) => After::Start,
            (Some(Some(was)), Some(now)) if now.saturating_sub(was) > GAP_MILLIS => After::Gap,
            _ => After::Growth,
        };
        before = Some(when);
        if at >= from {
            found.push(Request {
                context,
                cache_write,
                after,
            });
        }
    }
    found
}

#[cfg(test)]
mod tests;
