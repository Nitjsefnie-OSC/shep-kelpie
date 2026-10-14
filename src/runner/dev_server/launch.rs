//! The project's launch configuration: `.claude/launch.json` as `main` has it
//!
//! It is read from `origin/main`, never the worktree, so a worker cannot widen
//! its own dev server's sandbox by editing it.

use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::worktree::{BASE, git};

/// The file, relative to the repo's root
pub(super) const FILE: &str = ".claude/launch.json";

#[derive(Debug, Deserialize)]
struct File {
    #[serde(default)]
    configurations: Vec<Configuration>,
}

/// One entry of the file's `configurations`
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Configuration {
    pub(super) name: Option<String>,
    pub(super) runtime_executable: String,
    #[serde(default)]
    pub(super) runtime_args: Vec<String>,
    pub(super) port: Option<u16>,
    pub(super) cwd: Option<String>,
    #[serde(default)]
    pub(super) auto_port: bool,
}

/// The file's text on `origin/main` of `checkout`, or `None` when `main` has none
///
/// # Errors
///
/// Why git could not show a file that `main` has.
pub(super) fn read(checkout: &Path) -> Result<Option<String>, String> {
    let spec = format!("origin/{BASE}:{FILE}");
    if git(checkout, ["cat-file", "-e", &spec]).is_err() {
        return Ok(None);
    }
    git(checkout, ["show", &spec])
        .map(Some)
        .map_err(|e| format!("cannot read {FILE} on {BASE}: {e}"))
}

/// The configuration `name` picks in `text`, or the first when `name` is `None`
///
/// A file with no configuration gives `None`.
///
/// # Errors
///
/// Why `text` is not a launch file, or that it has no configuration `name`.
pub(super) fn select(text: &str, name: Option<&str>) -> Result<Option<Configuration>, String> {
    let file: File = serde_json::from_str(text)
        .map_err(|e| format!("{FILE} on {BASE} is not a launch file: {e}"))?;
    let mut configurations = file.configurations.into_iter();
    match name {
        None => Ok(configurations.next()),
        Some(name) => configurations
            .find(|c| c.name.as_deref() == Some(name))
            .map(Some)
            .ok_or_else(|| format!("{FILE} on {BASE} has no configuration named `{name}`")),
    }
}

/// The folder `configuration` starts in, which is under `worktree`
///
/// # Errors
///
/// Why the folder is refused: it leaves the worktree through `..`, an absolute
/// path or a link, or it is not a folder.
pub(super) fn folder(worktree: &Path, configuration: &Configuration) -> Result<PathBuf, String> {
    let Some(cwd) = configuration.cwd.as_deref().filter(|c| !c.is_empty()) else {
        return Ok(worktree.to_owned());
    };
    let relative = Path::new(cwd);
    let stays = relative
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if !stays {
        return Err(format!(
            "its `cwd` `{cwd}` leaves the worktree: only a path inside it is allowed"
        ));
    }
    let found = (worktree.join(relative).canonicalize())
        .map_err(|e| format!("its `cwd` `{cwd}` is not a folder in the worktree: {e}"))?;
    let root = (worktree.canonicalize()).map_err(|e| format!("the worktree is gone: {e}"))?;
    match found.starts_with(&root) && found.is_dir() {
        true => Ok(worktree.join(relative)),
        false => Err(format!(
            "its `cwd` `{cwd}` leaves the worktree through a link"
        )),
    }
}

#[cfg(test)]
mod tests;
