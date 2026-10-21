//! Codex calls handed from one image of the runner to the next
//!
//! The call's `codex` process survives the `execve`, and so do its record,
//! output and thread folder. A Codex call holds no gateway lease and goes
//! through no forwarder, so the old image carries only the session id kelpie
//! gave it.

use std::path::{Path, PathBuf};

// The settings path a call's record sits beside, which every other file of
// the call is named from: `<stem>.call.json` for the settings `<stem>.json`.
pub(super) fn settings_of(record: &Path) -> PathBuf {
    let name = record.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let stem = name.strip_suffix(".call.json").unwrap_or(name);
    record.with_file_name(format!("{stem}.json"))
}
