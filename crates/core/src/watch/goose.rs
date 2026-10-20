//! goose's transcript, the call's stdout file: a banner, and then one JSON
//! event a line, tagged `type`
//!
//! goose streams no text, so a message is one whole line.

use jiff::Timestamp;
use serde_json::Value;

use super::line::{Event, Kind, text_of, tool_line};

/// What a stdout line records, if it records anything worth a line
///
/// An `error` event carries no time, so it is stamped `since`: the time of the
/// line before it.
pub(super) fn events(line: &str, since: Timestamp) -> Vec<Event> {
    let Ok(entry) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    match entry["type"].as_str() {
        Some("message") => message(&entry["message"]),
        Some("error") => {
            let said =
                (entry["error"].as_str()).map_or_else(|| entry["error"].to_string(), str::to_owned);
            vec![Event {
                at: since,
                kind: Kind::Result,
                text: format!("error: {said}"),
            }]
        }
        _ => Vec::new(),
    }
}

fn message(message: &Value) -> Vec<Event> {
    let Some(at) = message["created"]
        .as_i64()
        .and_then(|secs| Timestamp::from_second(secs).ok())
    else {
        return Vec::new();
    };
    let event = |kind, text: String| Event { at, kind, text };
    let prompt = message["role"] == "user";
    message["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| match block["type"].as_str() {
            Some("text") if prompt => Some(event(Kind::Prompt, text_of(block))),
            Some("text") => Some(event(Kind::Said, text_of(block))),
            Some("thinking") => {
                let thought = block["thinking"].as_str().unwrap_or_default();
                (!thought.is_empty()).then(|| event(Kind::Thinking, thought.to_owned()))
            }
            Some("toolRequest") => Some(event(Kind::Tool, request(&block["toolCall"]))),
            Some("toolResponse") => Some(event(Kind::Result, response(&block["toolResult"]))),
            _ => None,
        })
        .collect()
}

// A tool call goose could not make has an `error` in place of its `value`.
fn request(call: &Value) -> String {
    match call["value"]["name"].as_str() {
        Some(name) => tool_line(name, &call["value"]["arguments"]),
        None => format!(
            "tool  error: {}",
            call["error"].as_str().unwrap_or_default()
        ),
    }
}

fn response(result: &Value) -> String {
    let value = &result["value"];
    if result["status"] == "error" || value["isError"] == true {
        let said = match value["content"].as_array() {
            Some(_) => text_of(&value["content"]),
            None => result["error"].as_str().unwrap_or_default().to_owned(),
        };
        return format!("error: {said}");
    }
    text_of(&value["content"])
}
