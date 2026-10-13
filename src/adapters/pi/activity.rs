//! What pi's stdout file says of a call's activity
//!
//! pi writes the session header, `agent_start`, `turn_start` and the opening
//! messages at once, before its model has said anything, so those say nothing
//! of the call. A call is active from its first event the model made.

use std::path::Path;

use serde::Deserialize;

use crate::ports::{CallActivity, written_at};

/// A file past this holds a model event, whatever its first lines say
const READ_LIMIT: u64 = 1 << 20;

/// When the call whose stdout is at `stdout` was last written to, once its
/// model has shown something, and [`CallActivity::Nothing`] before
pub(super) fn of_stdout(stdout: &Path) -> CallActivity {
    let Ok(meta) = std::fs::metadata(stdout) else {
        return CallActivity::Nothing;
    };
    if meta.len() <= READ_LIMIT && !holds_model_event(stdout) {
        return CallActivity::Nothing;
    }
    written_at(stdout)
}

fn holds_model_event(stdout: &Path) -> bool {
    let Ok(bytes) = std::fs::read(stdout) else {
        return false;
    };
    // The last line is cut short until its newline is written.
    let Some(end) = bytes.iter().rposition(|&b| b == b'\n') else {
        return false;
    };
    bytes[..end]
        .split(|&b| b == b'\n')
        .filter_map(|line| serde_json::from_slice::<Event>(line).ok())
        .any(|event| event.is_from_model())
}

#[derive(Deserialize)]
struct Event {
    #[serde(rename = "type")]
    kind: String,
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    role: String,
    #[serde(default)]
    content: Vec<serde_json::Value>,
}

impl Event {
    fn is_from_model(&self) -> bool {
        match self.kind.as_str() {
            "message_update" => true,
            // The empty assistant message pi opens with ends at once.
            "message_end" => self
                .message
                .as_ref()
                .is_some_and(|m| m.role == "assistant" && !m.content.is_empty()),
            kind => ["tool_execution_", "compaction_", "auto_retry_"]
                .iter()
                .any(|prefix| kind.starts_with(prefix)),
        }
    }
}
