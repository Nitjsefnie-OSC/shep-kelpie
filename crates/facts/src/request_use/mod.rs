//! What a call's transcript says of each request it made to the model: how
//! large its context was, how much of that it wrote to the cache, and what
//! came before it
//!
//! A transcript has one usage a request, where the ledger has one a call.
//! Claude Code writes a line for each block of a reply, all holding the
//! request's usage, so they count once; Codex writes one record a request.
//! A request is `after` the start if it is the file's first, after a gap if
//! longer passed since the request before than the cache keeps a write, and
//! after growth otherwise. `docs/facts/transcript-requests.md`

use std::collections::BTreeSet;

use serde_json::Value;

// Milliseconds after which the cache has dropped what a request wrote to its five-minute cache,
// which is also what a request is taken to have written if its usage does not say
const FIVE_MINUTES: i64 = 5 * 60 * 1000;
// The same for a write to the one-hour cache
const ONE_HOUR: i64 = 60 * 60 * 1000;

/// What came before a request
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    /// Nothing: the session's first request
    Start,
    /// A wait longer than the cache kept what the request before wrote
    Gap,
    /// A request within the time the cache kept it
    Growth,
}

impl After {
    /// Every case, as the series name them
    pub const ALL: [Self; 3] = [Self::Start, Self::Gap, Self::Growth];

    /// The case's name in a series' `after` label
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Gap => "gap",
            Self::Growth => "growth",
        }
    }
}

/// One request to the model
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    /// What it sent: its input, and what it read from and wrote to the cache
    pub context: u64,
    /// What it wrote to the cache
    pub cache_write: u64,
    /// What came before it
    pub after: After,
}

/// The requests Claude Code's transcript `text` holds from byte `from` on,
/// where the call whose lines begin there made them
pub fn claude(text: &str, from: usize) -> Vec<Request> {
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
        // The cache keeps a write for as long as the shortest cache it went to.
        let split = &usage["cache_creation"];
        let lives = if split["ephemeral_5m_input_tokens"].as_u64().unwrap_or(0) > 0 {
            Some(FIVE_MINUTES)
        } else if split["ephemeral_1h_input_tokens"].as_u64().unwrap_or(0) > 0 {
            Some(ONE_HOUR)
        } else {
            None
        };
        let cache_write = used("cache_creation_input_tokens");
        Some(Read {
            context: (used("input_tokens"))
                .saturating_add(used("cache_read_input_tokens"))
                .saturating_add(cache_write),
            cache_write,
            lives,
        })
    })
}

/// The requests pi's session file `text` holds from byte `from` on, where
/// the call whose lines begin there made them
pub fn pi(text: &str, from: usize) -> Vec<Request> {
    requests(text, from, |entry| {
        let message = &entry["message"];
        if entry["type"] != "message" || message["role"] != "assistant" {
            return None;
        }
        let usage = &message["usage"];
        let used = |key: &str| usage[key].as_u64().unwrap_or(0);
        let cache_write = used("cacheWrite");
        Some(Read {
            context: (used("input"))
                .saturating_add(used("cacheRead"))
                .saturating_add(cache_write),
            cache_write,
            lives: None,
        })
    })
}

/// The requests Codex's rollout `text` holds from byte `from` on, where the
/// call whose lines begin there made them
pub fn codex(text: &str, from: usize) -> Vec<Request> {
    let mut ids = BTreeSet::new();
    requests(text, from, |entry| {
        if entry["type"] != "token_usage_record" {
            return None;
        }
        let payload = &entry["payload"];
        // A request is one `response_id`.
        if payload["response_id"]
            .as_str()
            .is_some_and(|id| !ids.insert(id.to_owned()))
        {
            return None;
        }
        let usage = &payload["usage"];
        let cache_write = usage["cache_write_input_tokens"].as_u64().unwrap_or(0);
        Some(Read {
            // `input_tokens` holds the cached tokens already.
            context: usage["input_tokens"].as_u64().unwrap_or(0),
            cache_write,
            lives: (cache_write > 0).then_some(FIVE_MINUTES),
        })
    })
}

// What a line of a transcript says of its request
struct Read {
    context: u64,
    cache_write: u64,
    // Milliseconds the cache keeps what the request wrote, if its usage says
    lives: Option<i64>,
}

// The requests of `text` from byte `from`, as `read` finds each line's usage, the first line of a
// request only. A request that sent nothing, as Claude Code's `<synthetic>` replies, is no request.
// How long the cache kept what the request before wrote is `lasts`: the time of the last request
// whose usage said, else five minutes.
fn requests(text: &str, from: usize, mut read: impl FnMut(&Value) -> Option<Read>) -> Vec<Request> {
    // A file shorter than `from` is a new one.
    let from = if text.get(from..).is_some() { from } else { 0 };
    let mut offset = 0;
    let mut before: Option<Option<i64>> = None;
    let mut lasts = FIVE_MINUTES;
    let mut found = Vec::new();
    for line in text.split_inclusive('\n') {
        let at = offset;
        offset += line.len();
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(used) = read(&entry) else {
            continue;
        };
        if used.context == 0 {
            continue;
        }
        let when = (entry["timestamp"].as_str())
            .and_then(|time| time.parse::<jiff::Timestamp>().ok())
            .map(jiff::Timestamp::as_millisecond);
        let after = match (before, when) {
            (None, _) => After::Start,
            (Some(Some(was)), Some(now)) if now.saturating_sub(was) > lasts => After::Gap,
            _ => After::Growth,
        };
        before = Some(when);
        lasts = used.lives.unwrap_or(lasts);
        if at >= from {
            found.push(Request {
                context: used.context,
                cache_write: used.cache_write,
                after,
            });
        }
    }
    found
}

#[cfg(test)]
mod tests;
