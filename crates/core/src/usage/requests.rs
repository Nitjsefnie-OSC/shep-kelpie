//! What a session's transcript says of its requests and of the files it read
//! and edited
//!
//! Claude Code writes one line per content block, so one request is several
//! `assistant` lines that share a `message.id` and carry the same `usage`;
//! a request is counted once, from its last line. pi writes one `assistant`
//! message per request. Codex writes a `token_usage_record` per request and
//! names no file a tool call read or edited, so its session has no touches.
//! goose is not read. `docs/facts/session-transcripts.md`

use std::collections::BTreeSet;

use jiff::Timestamp;
use serde_json::Value;

use crate::ports::Usage;
use crate::watch::Format;

/// One request to a model, as its transcript records its usage
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Request {
    /// When its first line was written, where the transcript says
    pub at: Option<Timestamp>,
    /// The tokens it used
    pub usage: Usage,
}

impl Request {
    /// What the model was given: input, cache reads and cache writes
    pub fn context(&self) -> u64 {
        (self.usage.input)
            .saturating_add(self.usage.cache_read)
            .saturating_add(self.usage.cache_write)
    }

    /// Every token it used, the output included
    pub fn tokens(&self) -> u64 {
        self.context().saturating_add(self.usage.output)
    }
}

/// A file a session read or changed
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Touch {
    /// A file the session read
    Read(String),
    /// A file the session wrote or edited
    Edit(String),
}

/// What a session's transcript holds
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Parsed {
    /// Its requests, in order
    pub requests: Vec<Request>,
    /// The files it touched, in order
    pub touches: Vec<Touch>,
    /// The working directory it began in, where the transcript records one
    pub root: Option<String>,
}

/// What the transcript `text` of a session written in `format` holds, or
/// `None` when kelpie reads no transcript of that format
pub(super) fn parse(format: Format, text: &str) -> Option<Parsed> {
    // parity: harness.usage-requests, harness.usage-reads
    match format {
        Format::Claude => Some(claude(text)),
        Format::Pi => Some(pi(text)),
        Format::Codex => Some(codex(text)),
        Format::Goose => None,
    }
}

// The size of a count a transcript carries, or none
fn count(value: &Value) -> u64 {
    value.as_u64().unwrap_or(0)
}

fn time(entry: &Value) -> Option<Timestamp> {
    entry["timestamp"].as_str()?.parse().ok()
}

fn claude(text: &str) -> Parsed {
    let mut parsed = Parsed::default();
    // The index in `requests` of each message id seen
    let mut ids: Vec<(String, usize)> = Vec::new();
    let mut cwd = String::new();
    for entry in text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
    {
        let message = &entry["message"];
        // A subagent's lines are not this session's requests.
        if entry["type"] != "assistant" || entry["isSidechain"] == true {
            continue;
        }
        if let Some(dir) = entry["cwd"].as_str() {
            dir.clone_into(&mut cwd);
            parsed.root.get_or_insert_with(|| dir.to_owned());
        }
        for block in message["content"].as_array().into_iter().flatten() {
            if block["type"] == "tool_use"
                && let Some(touch) = touch_of(
                    block["name"].as_str().unwrap_or_default(),
                    &block["input"],
                    &cwd,
                    &["file_path", "notebook_path"],
                    &["Read"],
                    &["Edit", "MultiEdit", "Write", "NotebookEdit"],
                )
            {
                parsed.touches.push(touch);
            }
        }
        // Claude Code writes a request it did not make to a model as `<synthetic>`.
        let usage = &message["usage"];
        if message["model"] == "<synthetic>" || !usage.is_object() {
            continue;
        }
        let cache_write = count(&usage["cache_creation_input_tokens"]);
        let five_minutes = count(&usage["cache_creation"]["ephemeral_5m_input_tokens"]);
        let request = Request {
            at: time(&entry),
            usage: Usage {
                input: count(&usage["input_tokens"]),
                cache_write,
                cache_write_5m: five_minutes.min(cache_write),
                cache_read: count(&usage["cache_read_input_tokens"]),
                output: count(&usage["output_tokens"]),
            },
        };
        let id = message["id"].as_str().unwrap_or_default();
        match ids.iter().find(|(seen, _)| !id.is_empty() && seen == id) {
            Some((_, at)) => {
                parsed.requests[*at] = Request {
                    at: parsed.requests[*at].at,
                    ..request
                }
            }
            None => {
                ids.push((id.to_owned(), parsed.requests.len()));
                parsed.requests.push(request);
            }
        }
    }
    parsed.requests.retain(|request| request.tokens() > 0);
    parsed
}

fn pi(text: &str) -> Parsed {
    let mut parsed = Parsed::default();
    let mut cwd = String::new();
    for entry in text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
    {
        // pi names the working directory once, in its session header.
        if entry["type"] == "session"
            && let Some(dir) = entry["cwd"].as_str()
        {
            dir.clone_into(&mut cwd);
            parsed.root.get_or_insert_with(|| dir.to_owned());
        }
        let message = &entry["message"];
        if entry["type"] != "message" || message["role"] != "assistant" {
            continue;
        }
        for block in message["content"].as_array().into_iter().flatten() {
            if block["type"] == "toolCall"
                && let Some(touch) = touch_of(
                    block["name"].as_str().unwrap_or_default(),
                    &block["arguments"],
                    &cwd,
                    &["path"],
                    &["read"],
                    &["edit", "write"],
                )
            {
                parsed.touches.push(touch);
            }
        }
        let usage = &message["usage"];
        let request = Request {
            at: time(&entry),
            usage: Usage {
                input: count(&usage["input"]),
                cache_write: count(&usage["cacheWrite"]),
                cache_write_5m: 0,
                cache_read: count(&usage["cacheRead"]),
                output: count(&usage["output"]),
            },
        };
        // A message cut off before a model answered has no usage.
        if request.tokens() > 0 {
            parsed.requests.push(request);
        }
    }
    parsed
}

fn codex(text: &str) -> Parsed {
    let mut parsed = Parsed::default();
    let mut ids = BTreeSet::new();
    for entry in text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
    {
        let payload = &entry["payload"];
        if entry["type"] == "session_meta"
            && let Some(dir) = payload["cwd"].as_str()
        {
            parsed.root.get_or_insert_with(|| dir.to_owned());
        }
        if entry["type"] != "token_usage_record" {
            continue;
        }
        // A request is one `response_id`.
        if payload["response_id"]
            .as_str()
            .is_some_and(|id| !ids.insert(id.to_owned()))
        {
            continue;
        }
        // `input_tokens` holds the cached tokens, read and written.
        let usage = &payload["usage"];
        let cache_read = count(&usage["cached_input_tokens"]);
        let cache_write = count(&usage["cache_write_input_tokens"]);
        let request = Request {
            at: time(&entry),
            usage: Usage {
                input: (count(&usage["input_tokens"]))
                    .saturating_sub(cache_read)
                    .saturating_sub(cache_write),
                cache_write,
                cache_write_5m: 0,
                cache_read,
                output: count(&usage["output_tokens"]),
            },
        };
        if request.tokens() > 0 {
            parsed.requests.push(request);
        }
    }
    parsed
}

// The file a tool call named `name` with `input` reads or changes, under
// whichever of `keys` it names the file by, made absolute against `cwd` and
// tidied, so one file spelt two ways is one file
fn touch_of(
    name: &str,
    input: &Value,
    cwd: &str,
    keys: &[&str],
    reads: &[&str],
    edits: &[&str],
) -> Option<Touch> {
    let path = keys.iter().find_map(|key| input[key].as_str())?;
    // pi writes a path given with `@` for a file.
    let path = normalize(cwd, path.strip_prefix('@').unwrap_or(path));
    if reads.contains(&name) {
        Some(Touch::Read(path))
    } else if edits.contains(&name) {
        Some(Touch::Edit(path))
    } else {
        None
    }
}

// `path`, joined to `cwd` when relative, with `.` and `..` folded away and no
// trailing or doubled slash. A `..` above the root of a path stays, and a
// relative path with no `cwd` stays relative.
fn normalize(cwd: &str, path: &str) -> String {
    let joined = if path.starts_with('/') || cwd.is_empty() {
        path.to_owned()
    } else {
        format!("{cwd}/{path}")
    };
    let rooted = joined.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            ".." if rooted => {}
            _ => parts.push(part),
        }
    }
    let rest = parts.join("/");
    if rooted { format!("/{rest}") } else { rest }
}
