//! What goose's stdout file says of a call's activity
//!
//! goose prints a banner first, and a `complete` event when the call ends.
//! Neither is the model's doing. goose streams no text: a whole assistant
//! message is one line, so a long answer shows nothing until it ends.

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
        .any(|event| matches!(event.kind.as_str(), "message" | "notification" | "error"))
}

#[derive(Deserialize)]
struct Event {
    #[serde(rename = "type")]
    kind: String,
}
