//! Agent sessions as text, for the researcher to read
//!
//! Each CLI keeps its sessions in its own place and shape. [`KEPT`] is the
//! one small table of both: where a harness writes them, how deep under
//! that place a session file lies, and how a file is told to be its own.
//! How one is read is [`crate::watch`]'s, which `shep kelpie watch` reads
//! the same sessions with. The table is meant for #456's harness facts to
//! absorb.
//!
//! | Harness | Under the home folder | Depth | A file's first lines |
//! |---|---|---|---|
//! | Claude Code | `.claude/projects/<folder>/<session>.jsonl` | 1 | `user`, `assistant`, `summary`, ... |
//! | Codex | `.codex/sessions/<year>/<month>/<day>/rollout-*.jsonl` | 3 | `session_meta`, `response_item`, ... |
//! | pi | `.pi/agent/sessions/<folder>/<time>_<session>.jsonl` | 1 | `session`, `message`, ... |
//!
//! The maintainer names the folders (`researcher.transcripts`), and a
//! session in one is read only when the folder it ran in is the project's
//! checkout or lies under it, so a root that holds every project's sessions
//! can be named. Sessions are read newest first and cut to a total, so one
//! ruling never copies a maintainer's whole history.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use crate::watch::{Format, Said, SaidKind, said_in};

/// How deep under a folder the maintainer names a session file may lie: Codex's
/// year, month and day
const DEEPEST: usize = 3;

/// The most sessions copied for one ruling
pub(crate) const MOST_SESSIONS: usize = 12;

/// The most text copied for one session, in bytes: the end of it, since a
/// decision is made late
pub(crate) const MOST_PER_SESSION: usize = 96 * 1024;

/// The most text copied for one ruling, in bytes, over every session
pub(crate) const MOST_IN_ALL: usize = 512 * 1024;

/// The most one message keeps, in characters
const MESSAGE: usize = 2_000;

/// A harness whose sessions are read
pub(crate) struct Kept {
    /// The harness, as an agent file names it
    pub harness: &'static str,
    /// How its transcripts are read
    pub format: Format,
    /// The top-level `type` of a line only this harness writes
    tells: &'static [&'static str],
}

/// Every harness whose sessions kelpie reads, with its place in the module
/// doc's table
pub(crate) const KEPT: [Kept; 3] = [
    Kept {
        harness: "claude-code",
        format: Format::Claude,
        tells: &[
            "user",
            "assistant",
            "summary",
            "system",
            "file-history-snapshot",
        ],
    },
    Kept {
        harness: "codex",
        format: Format::Codex,
        tells: &["session_meta", "response_item", "event_msg", "turn_context"],
    },
    Kept {
        harness: "pi",
        format: Format::Pi,
        tells: &[
            "session",
            "message",
            "model_change",
            "thinking_level_change",
        ],
    },
];

/// The format of the transcript `harness` writes, if kelpie reads it
pub(crate) fn format_of(harness: crate::settings::Harness) -> Option<Format> {
    use crate::settings::Harness;
    match harness {
        Harness::ClaudeCode => Some(Format::Claude),
        Harness::Codex => Some(Format::Codex),
        Harness::Pi => Some(Format::Pi),
        // The watch reads goose's stdout file, which each call empties, so no
        // file holds a whole session.
        Harness::Goose => None,
        #[cfg(test)]
        Harness::StandIn => Some(Format::Claude),
    }
}

/// What a rendering keeps of a session
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Keep {
    /// What was asked and what was said
    Talk,
    /// What was asked, said and called, one line to a tool
    Work,
}

/// A maintainer's session found under a named folder
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Found {
    /// The harness that wrote it
    pub harness: &'static str,
    /// Its file
    pub path: PathBuf,
    /// The session as text
    pub text: String,
}

/// `lines` of a `format` transcript as text, one paragraph a message, cut to
/// its last `most` bytes
pub(crate) fn render(format: Format, lines: &str, keep: Keep, most: usize) -> String {
    let said: Vec<Said> = lines
        .lines()
        .flat_map(|line| said_in(format, line))
        .collect();
    let mut out = String::new();
    for event in said {
        let who = match event.kind {
            SaidKind::Prompt => "asked",
            SaidKind::Said => "said",
            SaidKind::Tool if keep == Keep::Work => "called",
            _ => continue,
        };
        let text = event.text.trim();
        if text.is_empty() {
            continue;
        }
        let at = event.at.strftime("%Y-%m-%d %H:%M");
        let text = clip(text, MESSAGE);
        out.push_str(&format!("[{at}] {who}: {text}\n\n"));
    }
    last(out, most)
}

// `text` cut to `most` characters, with an ellipsis.
fn clip(text: &str, most: usize) -> String {
    match text.char_indices().nth(most) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_owned(),
    }
}

// The last `most` bytes of `text`, from a message's start where one is in reach.
fn last(text: String, most: usize) -> String {
    if text.len() <= most {
        return text;
    }
    let mut from = text.len() - most;
    while !text.is_char_boundary(from) {
        from += 1;
    }
    let kept = &text[from..];
    let whole = kept.find("\n\n[").map_or(kept, |at| &kept[at + 2..]);
    format!("(earlier messages left out)\n\n{whole}")
}

/// The sessions under `folders` that ran in `within` or below it, newest
/// first, each rendered, to the limits above
///
/// A folder that is not there, or a file that cannot be read, is skipped.
pub(crate) fn sessions_in(folders: &[PathBuf], within: &Path) -> Vec<Found> {
    let mut files: Vec<(SystemTime, PathBuf)> = Vec::new();
    for folder in folders {
        collect(folder, DEEPEST, &mut files);
    }
    files.sort_by(|a, b| b.cmp(a));
    let mut found = Vec::new();
    let mut total = 0;
    for (_, path) in files {
        if found.len() >= MOST_SESSIONS || total >= MOST_IN_ALL {
            break;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some((kept, cwd)) = identify(&text) else {
            continue;
        };
        if !Path::new(&cwd).starts_with(within) {
            continue;
        }
        let room = MOST_PER_SESSION.min(MOST_IN_ALL - total);
        let rendered = render(kept.format, &text, Keep::Talk, room);
        if rendered.is_empty() {
            continue;
        }
        total += rendered.len();
        found.push(Found {
            harness: kept.harness,
            path,
            text: rendered,
        });
    }
    found
}

// Every `.jsonl` file under `dir`, down `depth` more folders, with when it was written.
fn collect(dir: &Path, depth: usize, into: &mut Vec<(SystemTime, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        if path.is_dir() {
            if depth > 0 {
                collect(&path, depth - 1, into);
            }
        } else if path.extension().is_some_and(|ext| ext == "jsonl")
            && let Ok(modified) = std::fs::metadata(&path).and_then(|m| m.modified())
        {
            into.push((modified, path));
        }
    }
}

// Which harness wrote a file's lines, and the folder it ran in, from its
// first lines. Every harness records the folder, as `cwd`, in a line near
// its start; Codex's is in the `payload` of its first.
fn identify(text: &str) -> Option<(&'static Kept, String)> {
    let mut kept: Option<&'static Kept> = None;
    for line in text.lines().take(40) {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if kept.is_none() {
            let kind = entry["type"].as_str().unwrap_or_default();
            kept = KEPT.iter().find(|k| k.tells.contains(&kind));
        }
        let cwd = (entry["cwd"].as_str()).or_else(|| entry["payload"]["cwd"].as_str());
        if let (Some(kept), Some(cwd)) = (kept, cwd) {
            return Some((kept, cwd.to_owned()));
        }
    }
    None
}

#[cfg(test)]
mod tests;
