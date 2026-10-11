//! Finding a session's transcript file, which each harness names its own way

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use jiff::Timestamp;

use super::log::{Format, Started};

/// How deep under its `sessions` folder Codex keeps a rollout: year, month, day
const CODEX_DEPTH: usize = 3;

/// The transcript file of the session `started` names, once the harness has
/// written it
pub(super) fn find(started: &Started) -> Option<PathBuf> {
    match started.format {
        // Named for the session, from the id kelpie gave it.
        Format::Claude => {
            let file = started.folder.join(format!("{}.jsonl", started.session));
            file.is_file().then_some(file)
        }
        // Named `<time>_<session>.jsonl` when pi first writes it.
        Format::Pi => {
            let suffix = format!("_{}.jsonl", started.session);
            files(&started.folder, 0).into_iter().find(|file| {
                (file.file_name().and_then(|n| n.to_str())).is_some_and(|n| n.ends_with(&suffix))
            })
        }
        Format::Codex => codex(started),
    }
}

// Codex names a rollout for its own thread. Kelpie keeps which thread each
// of its session ids is once the first call ends. Until then, a rollout no
// session claims, written since this one began, is its.
fn codex(started: &Started) -> Option<PathBuf> {
    let threads = started.threads.as_deref();
    let claimed = |dir: &Path| -> Vec<(String, String)> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                let session = path.file_stem()?.to_str()?.to_owned();
                let thread: serde_json::Value =
                    serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
                Some((session, thread["id"].as_str()?.to_owned()))
            })
            .collect()
    };
    let claims = threads.map(claimed).unwrap_or_default();
    let rollouts = files(&started.folder, CODEX_DEPTH);
    let named = |thread: &str| {
        let suffix = format!("-{thread}.jsonl");
        rollouts.iter().find(|file| {
            (file.file_name().and_then(|n| n.to_str())).is_some_and(|n| n.ends_with(&suffix))
        })
    };
    if let Some((_, thread)) = claims
        .iter()
        .find(|(session, _)| *session == started.session)
    {
        return named(thread).cloned();
    }
    let since = SystemTime::from(started.at);
    rollouts
        .iter()
        .filter(|file| {
            !claims
                .iter()
                .any(|(_, thread)| named(thread).is_some_and(|claimed| claimed == *file))
        })
        .filter_map(|file| {
            let modified = std::fs::metadata(file).and_then(|m| m.modified()).ok()?;
            (modified >= since).then_some((modified, file))
        })
        .max()
        .map(|(_, file)| file.clone())
}

// Every `.jsonl` file under `dir`, down `depth` more folders
fn files(dir: &Path, depth: usize) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        if path.is_dir() && depth > 0 {
            found.extend(files(&path, depth - 1));
        } else if path.extension().is_some_and(|ext| ext == "jsonl") {
            found.push(path);
        }
    }
    found
}

/// When the file at `path` was last written
pub(super) fn written(path: &Path) -> Option<Timestamp> {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    Timestamp::try_from(modified).ok()
}
