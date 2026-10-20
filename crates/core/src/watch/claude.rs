//! Claude Code's transcript: one JSON line per message block, in
//! `~/.claude/projects/<folder>/<session>.jsonl`

use jiff::Timestamp;
use serde_json::Value;

use super::line::{Event, Kind, text_of, tool_line};

/// What a transcript line records, if it records anything worth a line
pub(super) fn events(line: &str) -> Vec<Event> {
    let Ok(entry) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    let Some(at) = entry["timestamp"]
        .as_str()
        .and_then(|at| at.parse::<Timestamp>().ok())
    else {
        return Vec::new();
    };
    let content = &entry["message"]["content"];
    let event = |kind, text: String| Event { at, kind, text };
    match entry["type"].as_str() {
        // Claude Code's own notes to itself, such as a skill's expansion
        Some("user") if entry["isMeta"] == true => Vec::new(),
        Some("user") => match content {
            Value::String(text) => vec![event(Kind::Prompt, text.clone())],
            Value::Array(blocks) => blocks
                .iter()
                .filter_map(|block| match block["type"].as_str() {
                    Some("text") => Some(event(Kind::Prompt, text_of(block))),
                    Some("tool_result") => {
                        let said = text_of(&block["content"]);
                        let failed = block["is_error"] == true;
                        Some(event(
                            Kind::Result,
                            if failed {
                                format!("error: {said}")
                            } else {
                                said
                            },
                        ))
                    }
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        },
        Some("assistant") => content
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|block| match block["type"].as_str() {
                Some("text") => Some(event(Kind::Said, text_of(block))),
                Some("thinking") => {
                    let thought = block["thinking"].as_str().unwrap_or_default();
                    (!thought.is_empty()).then(|| event(Kind::Thinking, thought.to_owned()))
                }
                Some("tool_use") => {
                    let name = block["name"].as_str().unwrap_or("tool");
                    Some(event(Kind::Tool, tool_line(name, &block["input"])))
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}
