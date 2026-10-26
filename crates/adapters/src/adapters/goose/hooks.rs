//! The plugin that fences a goose worker, reviewer, advisor and retro
//!
//! goose runs a `PreToolUse` hook before every tool call and refuses the call
//! when it exits 2. `on_failure: block` makes a hook that cannot run refuse too.
//! Kelpie writes the plugin into the call's goose root, beside a `config.yaml`
//! that has it on. Three things keep the worker from switching it off: the
//! root's `.agents` and `config` folders are read only to the sandbox, the
//! worktree's `.config/goose` is fenced (a `disabledPlugins` entry there turns a
//! plugin off), and a same-named project plugin would win, so the call refuses
//! to start when the plugin folder holds anything but this one.

use std::path::{Path, PathBuf};

use serde_json::json;

use super::files::Files;
use crate::guard::{FOLDER_FLAG, GOOSE_FLAG, GOOSE_SERVER_FLAG, GOOSE_WRITE_FLAG, READ_ONLY_FLAG};
use crate::hooks::GUARD_BUDGET;
use crate::ports::{AgentError, Fence, Guard};
use crate::spawn::shell_quote;

/// The plugin's name, and the one entry its folder may hold
pub(crate) const PLUGIN: &str = "kelpie-fence";

/// The plugin's folder under the root
pub(super) fn folder(files: &Files) -> PathBuf {
    files.plugins().join(PLUGIN)
}

/// Fails when the root's plugin folder holds a plugin besides the fence's
///
/// goose gives a plugin of the same name the first place among them, and
/// loads every one it finds, so a plugin kelpie did not write is a hook that
/// runs unjudged.
pub(super) fn only_ours(files: &Files) -> Result<(), AgentError> {
    let folder = files.plugins();
    let entries = match std::fs::read_dir(&folder) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(setup("read", &folder, &e)),
    };
    for entry in entries {
        let entry = entry.map_err(|e| setup("read", &folder, &e))?;
        if entry.file_name() != PLUGIN {
            return Err(AgentError::Setup(format!(
                "goose would load {} as a plugin of the call: the plugin folder may hold only \
                 `{PLUGIN}`, so remove it",
                entry.path().display()
            )));
        }
    }
    Ok(())
}

/// What the hook of a call lets it do
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Judging<'a> {
    /// A worker's: its shell and its file tools, held to its folders
    Work,
    /// The read-only guard's: the call writes no file
    Reading,
    /// The read-only guard's, and a `write` into this folder alone: a reviewer asked for tests
    Tests(&'a Path),
}

/// Writes the plugin and the config that turns it on, for a call held by `fence`
///
/// `judging` says what the hook lets the call do. The hook lets through the tools of each MCP
/// server in `servers`, which are the keys goose names them by.
pub(super) fn write(
    files: &Files,
    fence: &Fence,
    judging: Judging<'_>,
    servers: &[String],
) -> Result<(), AgentError> {
    let plugin = folder(files);
    // A link there would take the files below somewhere else.
    match std::fs::symlink_metadata(&plugin) {
        Ok(found) if !found.is_dir() => {
            return Err(AgentError::Setup(format!(
                "{} is not a folder of the plugin's own",
                plugin.display()
            )));
        }
        Ok(_) => std::fs::remove_dir_all(&plugin).map_err(|e| setup("empty", &plugin, &e))?,
        Err(_) => {}
    }
    let hooks = plugin.join("hooks");
    std::fs::create_dir_all(&hooks).map_err(|e| setup("make", &hooks, &e))?;
    let manifest = json!({
        "name": PLUGIN,
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Kelpie's fence: judges every tool call of a worker, reviewer, advisor or retro before it runs",
    });
    put(&plugin.join("plugin.json"), &manifest)?;
    put(
        &hooks.join("hooks.json"),
        &hooks_json(&fence.guard, judging, servers),
    )?;
    put(&files.config_file(), &config(files, &plugin)?)
}

// The root's `config.yaml` with the plugin on, keyed by the plugin's path.
// Other settings stay, since goose may have written them; a file that is not a
// map is replaced. JSON is YAML, so the value is written as JSON.
fn config(files: &Files, plugin: &Path) -> Result<serde_json::Value, AgentError> {
    let path = files.config_file();
    let kept = match std::fs::read_to_string(&path) {
        Ok(text) => serde_saphyr::from_str::<serde_json::Value>(&text).ok(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(setup("read", &path, &e)),
    };
    let mut config = match kept {
        Some(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    let mut plugins = match config.remove("plugins") {
        Some(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    plugins.insert(
        plugin.to_string_lossy().into_owned(),
        json!({ "enabled": true }),
    );
    config.insert("plugins".into(), serde_json::Value::Object(plugins));
    Ok(serde_json::Value::Object(config))
}

/// Fails when the root holds a goose settings file that turns plugins off
///
/// The root's `.config/goose/settings.json` is read by goose beside the
/// working folder's, and a `disabledPlugins` entry in it drops a plugin by
/// name. The sandbox leaves the folder read only, so a file there was not
/// the call's to write: kelpie refuses it and does not guess at its entries.
pub(super) fn no_settings_in_root(files: &Files) -> Result<(), AgentError> {
    let folder = files.root.join(".config").join("goose");
    for name in ["settings.json", "settings.local.json"] {
        let file = folder.join(name);
        if std::fs::symlink_metadata(&file).is_ok() {
            return Err(AgentError::Setup(format!(
                "goose would read {} and it can switch plugins off: remove it",
                file.display()
            )));
        }
    }
    Ok(())
}

// One `PreToolUse` hook with no matcher, so it runs for every tool.
fn hooks_json(guard: &Guard, judging: Judging<'_>, servers: &[String]) -> serde_json::Value {
    json!({
        "hooks": {
            "PreToolUse": [{
                "hooks": [{
                    "type": "command",
                    "command": command(guard, judging, servers),
                    "on_failure": "block",
                    // The guard's own budget ends first, and refuses.
                    "timeout": GUARD_BUDGET.as_secs() + 5,
                }]
            }]
        }
    })
}

/// The command goose runs for each tool call: `kelpie guard` and its flags
///
/// When reading it leaves out the folders, which only the worker's command
/// guard reads, and asks for the read-only guard after the build folder:
/// `--goose=<build> --read-only`. For a reviewer asked for tests, the folder
/// its `write` may fill follows, as `--goose-write=<folder>`.
fn command(guard: &Guard, judging: Judging<'_>, servers: &[String]) -> String {
    let read_only = judging != Judging::Work;
    let quoted = |p: &Path| shell_quote(&p.to_string_lossy());
    let folders = (guard.folders.iter())
        .filter(|_| !read_only)
        .map(|p| shell_quote(&format!("{FOLDER_FLAG}{}", p.display())));
    [
        quoted(&guard.kelpie),
        shell_quote("guard"),
        quoted(&guard.git_common_dir),
        quoted(&guard.worktree),
    ]
    .into_iter()
    .chain(folders)
    .chain(guard.hooks.flag().map(|flag| shell_quote(&flag)))
    .chain(guard.base_flag().map(|flag| shell_quote(&flag)))
    .chain(guard.kelpie_pushes_flag().map(shell_quote))
    .chain([shell_quote(&format!(
        "{GOOSE_FLAG}{}",
        guard.build.display()
    ))])
    .chain(read_only.then(|| shell_quote(READ_ONLY_FLAG)))
    .chain(match judging {
        Judging::Tests(folder) => Some(shell_quote(&format!(
            "{GOOSE_WRITE_FLAG}{}",
            folder.display()
        ))),
        _ => None,
    })
    .chain(
        servers
            .iter()
            .map(|key| shell_quote(&format!("{GOOSE_SERVER_FLAG}{key}"))),
    )
    .collect::<Vec<_>>()
    .join(" ")
}

fn put(path: &Path, value: &serde_json::Value) -> Result<(), AgentError> {
    let text = serde_json::to_string_pretty(value).expect("a plugin file is JSON");
    std::fs::write(path, text).map_err(|e| setup("write", path, &e))
}

fn setup(verb: &str, path: &Path, e: &std::io::Error) -> AgentError {
    AgentError::Setup(format!("cannot {verb} {}: {}", path.display(), e.kind()))
}
