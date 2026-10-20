//! pi's transcript: a session header and then one JSON line per message, in
//! `<pi home>/sessions/<time>_<session>.jsonl`

use jiff::Timestamp;
use serde_json::Value;

use super::line::{Event, Kind, text_of, tool_line};

/// What a session file line records, if it records anything worth a line
pub(super) fn events(line: &str) -> Vec<Event> {
    let Ok(entry) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    if entry["type"] != "message" {
        return Vec::new();
    }
    let message = &entry["message"];
    // The entry's time, or the message's own in milliseconds
    let at = entry["timestamp"]
        .as_str()
        .and_then(|at| at.parse::<Timestamp>().ok())
        .or_else(|| {
            let millis = message["timestamp"].as_i64()?;
            Timestamp::from_millisecond(millis).ok()
        });
    let Some(at) = at else {
        return Vec::new();
    };
    let event = |kind, text: String| Event { at, kind, text };
    match message["role"].as_str() {
        Some("user") => vec![event(Kind::Prompt, text_of(&message["content"]))],
        Some("toolResult") => {
            let said = text_of(&message["content"]);
            let failed = message["isError"] == true;
            vec![event(
                Kind::Result,
                if failed {
                    format!("error: {said}")
                } else {
                    said
                },
            )]
        }
        Some("assistant") => message["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|block| match block["type"].as_str() {
                Some("text") => Some(event(Kind::Said, text_of(block))),
                Some("thinking") => {
                    let thought = block["thinking"].as_str().unwrap_or_default();
                    (!thought.is_empty()).then(|| event(Kind::Thinking, thought.to_owned()))
                }
                Some("toolCall") => {
                    let name = block["name"].as_str().unwrap_or("tool");
                    Some(event(Kind::Tool, tool_line(name, &block["arguments"])))
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}
