//! The migrations an upgrade runs after the build is installed and before the restarts
//!
//! The agent files are migrated first, then the projects' settings. A bare
//! upgrade runs this process's steps, since the file on disk is the build
//! already. An install runs the installed build's own `agents migrate` and
//! `settings migrate`, since this process predates the build it installed and
//! does not know its kinds. A rollback runs none.

use std::path::{Path, PathBuf};

use super::{Action, Scene};
use crate::agent_verbs;
use crate::flock::set::Homes;
use crate::home::KELPIE_VAR;
use crate::shepherd::Connection;

/// Migrates the agent files and the settings for `action`, giving each line to `say`
///
/// A migration that fails is said and not fatal: the restarted dog migrates
/// the agent files at its start, and `settings migrate` can be run again.
pub(super) async fn run(
    scene: &Scene<'_>,
    action: &Action,
    installed: &Path,
    client: &dyn Connection,
    say: &mut dyn FnMut(String),
) {
    let (agents, settings) = match action {
        Action::Rollback => return,
        Action::Installed => {
            let folder = scene.kelpie_home.join(crate::agents::FOLDER);
            let agents = agent_verbs::migrate(&folder);
            (agents, in_process(scene, client).await)
        }
        Action::Install(_) => (
            by_build(scene, installed, "agents"),
            by_build(scene, installed, "settings"),
        ),
    };
    match agents {
        Ok(lines) => lines.into_iter().for_each(&mut *say),
        Err(why) => say(format!(
            "the agent files were not migrated: {why}. The dog migrates them when it starts, or \
             `shep kelpie agents migrate` does"
        )),
    }
    match settings {
        Ok(lines) => lines.into_iter().for_each(say),
        Err(why) => say(format!(
            "the settings were not migrated: {why}. `shep kelpie settings migrate` migrates \
             them once that is put right"
        )),
    }
}

// This process's own `settings migrate`, with the maintainer's home as the verb takes it.
async fn in_process(scene: &Scene<'_>, client: &dyn Connection) -> Result<Vec<String>, String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("HOME is not set")?;
    let homes = Homes {
        home: &home,
        kelpie_home: scene.kelpie_home,
        shep_home: scene.shep_home,
    };
    crate::flock::set::migrate::migrate(client, homes).await
}

// The lines `<installed> <noun> migrate` prints.
fn by_build(scene: &Scene<'_>, installed: &Path, noun: &str) -> Result<Vec<String>, String> {
    let output = crate::spawn::command(installed)
        .args([noun, "migrate"])
        .env("SHEP_HOME", scene.shep_home)
        .env(KELPIE_VAR, scene.kelpie_home)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("cannot run {} {noun} migrate: {e}", installed.display()))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{} {noun} migrate exited {}: {}",
            installed.display(),
            output.status,
            err.trim()
        ));
    }
    let said = String::from_utf8_lossy(&output.stdout);
    Ok(said.lines().map(str::to_owned).collect())
}
