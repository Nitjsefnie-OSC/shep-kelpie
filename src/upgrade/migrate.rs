//! The agent-file migration an upgrade runs after the build is installed and before the restarts
//!
//! A bare upgrade runs this process's step, since the file on disk is the build
//! already. An install runs the installed build's own `agents migrate`, since
//! this process predates the build it installed. A rollback runs none.

use std::path::Path;

use super::{Action, Scene};
use crate::agent_verbs;
use crate::home::KELPIE_VAR;

/// Migrates the agent files of `scene`'s kelpie home for `action`, giving each line to `say`
///
/// A migration that fails is said and not fatal: the restarted dog migrates
/// at its start, so the files are put right either way.
pub(super) fn run(
    scene: &Scene<'_>,
    action: &Action,
    installed: &Path,
    say: &mut dyn FnMut(String),
) {
    let done = match action {
        Action::Rollback => return,
        Action::Installed => {
            let folder = scene.kelpie_home.join(crate::agents::FOLDER);
            agent_verbs::migrate(&folder)
        }
        Action::Install(_) => by_build(scene, installed),
    };
    match done {
        Ok(lines) => lines.into_iter().for_each(say),
        Err(why) => say(format!(
            "the agent files were not migrated: {why}. The dog migrates them when it starts, or \
             `shep kelpie agents migrate` does"
        )),
    }
}

// The lines `<installed> agents migrate` prints.
fn by_build(scene: &Scene<'_>, installed: &Path) -> Result<Vec<String>, String> {
    let output = crate::spawn::command(installed)
        .args(["agents", "migrate"])
        .env("SHEP_HOME", scene.shep_home)
        .env(KELPIE_VAR, scene.kelpie_home)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("cannot run {} agents migrate: {e}", installed.display()))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{} agents migrate exited {}: {}",
            installed.display(),
            output.status,
            err.trim()
        ));
    }
    let said = String::from_utf8_lossy(&output.stdout);
    Ok(said.lines().map(str::to_owned).collect())
}
