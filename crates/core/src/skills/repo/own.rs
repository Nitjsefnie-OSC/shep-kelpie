//! The repo's own `.claude` files, which load with no plugin
//!
//! A repo sets Claude Code up for everyone who works in it without a plugin
//! too: skills in `.claude/skills/`, sub-agents in `.claude/agents/`, commands
//! in `.claude/commands/` and hooks in `.claude/settings.json`. A call that runs in
//! a worktree loads them from there, but the project manager, the advisor,
//! the researcher and the issue writer run in folders of their own. So kelpie
//! reads them from `origin`'s default branch, never a worker's branch, and
//! writes them as one more plugin, `kelpie-repo`, which a repo's plugin cannot
//! be named. The hooks go to the plugin's `hooks/hooks.json`, where they run
//! beside the hooks of the call's settings file and cannot replace them.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::{RepoPlugin, Servers, mcp, settings_on_default_branch};
use crate::settings::{Part, RepoSetup};

/// The plugin's name, which the repo's plugins never take: `kelpie-` names are refused
pub(super) const NAME: &str = "kelpie-repo";

/// How `status` and `doctor` name it: where it comes from
const ID: &str = ".claude";

/// The repo's MCP servers, at its root
const MCP_JSON: &str = ".mcp.json";

/// The folders of `.claude` that are one part each
const FOLDERS: [Part; 3] = [Part::Skills, Part::Agents, Part::Commands];

/// What the default branch's `.claude` holds, to be written into the plugin
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Own {
    /// The regular files of the folders that load, by path from the plugin
    files: BTreeMap<PathBuf, Vec<u8>>,
    /// The `hooks` of `.claude/settings.json`, where they load
    hooks: Option<Value>,
    /// The servers of `.mcp.json`, where they load
    mcp: Servers,
}

/// The repo's own files as `setup` loads them, or none where it loads none or the repo has none
///
/// Reads `origin`'s default branch as the checkout last fetched it.
pub(super) fn plan(
    checkout: &Path,
    base: &str,
    into: &Path,
    setup: &RepoSetup,
) -> Option<RepoPlugin> {
    if !setup.own {
        return None;
    }
    super::default_head(checkout, base)?;
    let read = read(checkout, base, setup);
    let own = match read {
        Ok(own) => own,
        Err(reason) => return Some(RepoPlugin::skipped(ID.to_owned(), reason)),
    };
    if own.files.is_empty() && own.hooks.is_none() && own.mcp.is_empty() {
        return None;
    }
    let skills = skills_in(&own.files);
    Some(RepoPlugin {
        plugin: ID.to_owned(),
        skills,
        skipped: None,
        stale: None,
        unreadable: Vec::new(),
        dir: Some(into.join(NAME)),
        found: None,
        digest: 0,
        mcp: own.mcp.clone(),
        own: Some(own),
    })
}

fn read(checkout: &Path, base: &str, setup: &RepoSetup) -> Result<Own, String> {
    let mut own = Own::default();
    for part in FOLDERS.into_iter().filter(|part| setup.loads(*part)) {
        let folder = format!("{}/{}", ".claude", part.as_str());
        for (path, blob) in tree(checkout, base, &folder)? {
            own.files.insert(Path::new(part.as_str()).join(path), blob);
        }
    }
    if setup.loads(Part::Hooks) {
        // The file was read for the repo's plugins already; a file that is not
        // JSON is told there, and holds no hooks here.
        if let Ok(Some((settings, _))) = settings_on_default_branch(checkout, base, false) {
            own.hooks = settings["hooks"]
                .as_object()
                .filter(|hooks| !hooks.is_empty())
                .map(|hooks| Value::Object(hooks.clone()));
        }
    }
    if setup.loads(Part::Mcp) {
        own.mcp = mcp_json(checkout, base);
    }
    Ok(own)
}

// The servers of `.mcp.json` on `origin`'s default branch, none where the file is absent or
// unreadable. The worktree's copy is a worker's to change, so it is never read.
fn mcp_json(checkout: &Path, base: &str) -> Servers {
    use crate::worktree::git;
    let spec = format!("origin/{base}:{MCP_JSON}");
    if git(checkout, ["cat-file", "-e", &spec]).is_err() {
        return Servers::new();
    }
    git(checkout, ["show", &spec])
        .map(|text| mcp::own_servers(&text))
        .unwrap_or_default()
}

// The regular files under `folder` on `origin`'s default branch, by path from `folder`. A link in
// the tree is left out, so nothing the branch holds leads out of the plugin.
fn tree(checkout: &Path, base: &str, folder: &str) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    use crate::worktree::in_repo;
    let run = |args: &[&str]| -> Result<Vec<u8>, String> {
        let output = in_repo(checkout)
            .args(args)
            .output()
            .map_err(|e| e.to_string())?;
        if output.status.success() {
            return Ok(output.stdout);
        }
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    };
    let branch = format!("origin/{base}");
    let listing = run(&[
        "--literal-pathspecs",
        "ls-tree",
        "-r",
        "-z",
        &branch,
        "--",
        folder,
    ])
    .map_err(|e| format!("cannot read {folder} on {base}: {e}"))?;
    let mut files = Vec::new();
    for entry in listing.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        let text = String::from_utf8_lossy(entry);
        let Some((meta, path)) = text.split_once('\t') else {
            continue;
        };
        let fields: Vec<&str> = meta.split(' ').collect();
        let [mode, "blob", sha] = fields[..] else {
            continue;
        };
        if mode != "100644" && mode != "100755" {
            continue;
        }
        let Ok(rest) = Path::new(path).strip_prefix(folder) else {
            continue;
        };
        let blob = run(&["cat-file", "blob", sha])
            .map_err(|e| format!("cannot read {path} on {base}: {e}"))?;
        files.push((rest.to_owned(), blob));
    }
    Ok(files)
}

// The names of the skills among `files`: the folders of `skills/` that hold a `SKILL.md`.
fn skills_in(files: &BTreeMap<PathBuf, Vec<u8>>) -> Vec<String> {
    files
        .keys()
        .filter_map(|path| {
            let mut parts = path.components();
            let (first, name, file) = (parts.next()?, parts.next()?, parts.next()?);
            let skill = first.as_os_str() == "skills"
                && file.as_os_str() == "SKILL.md"
                && parts.next().is_none();
            skill.then(|| name.as_os_str().to_str().map(str::to_owned))?
        })
        .collect()
}

/// Writes the plugin into `dir`, replacing what was there
pub(super) fn install(own: &Own, dir: &Path) -> Result<(), String> {
    let written = (|| {
        match fs::remove_dir_all(dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        fs::create_dir_all(dir.join(".claude-plugin"))?;
        let manifest = json!({
            "name": NAME,
            "description": "The repo's own .claude files, as its default branch holds them",
        });
        fs::write(dir.join(".claude-plugin/plugin.json"), manifest.to_string())?;
        for (path, blob) in &own.files {
            let target = dir.join(path);
            fs::create_dir_all(target.parent().unwrap_or(dir))?;
            fs::write(target, blob)?;
        }
        if let Some(hooks) = &own.hooks {
            fs::create_dir_all(dir.join("hooks"))?;
            fs::write(
                dir.join("hooks/hooks.json"),
                json!({ "hooks": hooks }).to_string(),
            )?;
        }
        Ok(())
    })();
    written.map_err(|e| format!("cannot write it into {}: {e}", dir.display()))
}
