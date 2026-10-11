//! Codex's transcript, its rollout: one JSON line per event, in
//! `<codex home>/sessions/<year>/<month>/<day>/rollout-<time>-<thread>.jsonl`

use jiff::Timestamp;
use serde_json::Value;

use super::line::{Event, Kind, text_of, tool_line};

/// What a rollout line records, if it records anything worth a line
///
/// Only the `response_item` lines count: the `event_msg` lines repeat them.
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
    if entry["type"] != "response_item" {
        return Vec::new();
    }
    let item = &entry["payload"];
    let event = |kind, text: String| vec![Event { at, kind, text }];
    match item["type"].as_str() {
        Some("message") => {
            let kind = match item["role"].as_str() {
                Some("user") => Kind::Prompt,
                Some("assistant") => Kind::Said,
                _ => return Vec::new(),
            };
            event(kind, text_of(&item["content"]))
        }
        Some("reasoning") => {
            let thought = text_of(&item["summary"]);
            if thought.is_empty() {
                Vec::new()
            } else {
                event(Kind::Thinking, thought)
            }
        }
        Some("function_call") => {
            let name = item["name"].as_str().unwrap_or("tool");
            // Codex gives a call's arguments as JSON in a string.
            let input = (item["arguments"].as_str())
                .and_then(|args| serde_json::from_str(args).ok())
                .unwrap_or(Value::Null);
            event(Kind::Tool, tool_line(name, &input))
        }
        Some("custom_tool_call") => {
            let name = item["name"].as_str().unwrap_or("tool");
            let input = item["input"].as_str().unwrap_or_default();
            event(Kind::Tool, format!("{name}  {input}"))
        }
        Some("local_shell_call") => event(Kind::Tool, tool_line("shell", &item["action"])),
        Some("function_call_output" | "custom_tool_call_output") => {
            // An output is its text, or JSON with the text in `output`.
            let output = match &item["output"] {
                Value::String(text) => serde_json::from_str::<Value>(text)
                    .ok()
                    .and_then(|v| v["output"].as_str().map(str::to_owned))
                    .unwrap_or_else(|| text.clone()),
                other => other["output"]
                    .as_str()
                    .map_or_else(|| text_of(&other["content"]), str::to_owned),
            };
            event(Kind::Result, output)
        }
        _ => Vec::new(),
    }
}
