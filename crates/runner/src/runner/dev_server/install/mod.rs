//! The packages of a checkout, installed before its dev server starts
//!
//! A checkout of a bump's head or a fresh worktree has no `node_modules`, and a
//! server that needs a package would fetch it into a temp folder its sandbox
//! refuses. The install is part of the server's command, so it runs in the
//! server's sandbox, which writes only the checkout and its build folder and
//! reaches only the hosts listed. It runs only where the lockfile's folder has no
//! `node_modules`, or an earlier install left its mark and did not finish, so a
//! worker's own install is never wiped by `ci`. The package manager is the one
//! whose lockfile the project keeps, run from the lockfile as it stands (`ci`, or
//! a frozen lockfile) and with no package's install script, since a bump's new
//! version is code nobody has read yet.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::ports::DevRun;

/// How long the install and the server together get to answer on the port
const INSTALL_WITHIN: Duration = Duration::from_secs(300);

/// The file in the build folder that an install leaves while it runs
///
/// An install stopped part way leaves a `node_modules` that looks whole, so the next start
/// installs again while this is there.
const UNFINISHED: &str = "install-unfinished";

// What the shell runs: the install in the lockfile's folder unless it holds `node_modules`
// that no install left unfinished, which must succeed, and then the server. The folder is
// `$1`, the mark `$2`, and the server's program and arguments follow them.
const SCRIPT: &str = "(cd \"$1\" && { { [ -d node_modules ] && [ ! -e \"$2\" ]; } \
                      || { touch \"$2\" && INSTALL && rm -f \"$2\"; }; }) \
                      || exit 1; shift 2; exec \"$@\"";

/// A package manager, known by its lockfile
struct Manager {
    lockfile: &'static str,
    install: &'static str,
    /// The registries it fetches from
    hosts: &'static [&'static str],
    /// The variables that put its cache in the build folder, with the folder in it
    caches: &'static [(&'static str, &'static str)],
}

const MANAGERS: [Manager; 5] = [
    Manager {
        lockfile: "package-lock.json",
        install: "npm ci --ignore-scripts --no-audit --no-fund",
        hosts: &["registry.npmjs.org"],
        caches: &[("npm_config_cache", "npm-cache")],
    },
    Manager {
        lockfile: "pnpm-lock.yaml",
        install: "pnpm install --frozen-lockfile --ignore-scripts",
        hosts: &["registry.npmjs.org"],
        caches: &[("npm_config_store_dir", "pnpm-store")],
    },
    Manager {
        lockfile: "yarn.lock",
        install: "yarn install --frozen-lockfile --ignore-scripts",
        hosts: &["registry.yarnpkg.com", "registry.npmjs.org"],
        caches: &[("YARN_CACHE_FOLDER", "yarn-cache")],
    },
    Manager {
        lockfile: "bun.lock",
        install: "bun install --frozen-lockfile --ignore-scripts",
        hosts: &["registry.npmjs.org"],
        caches: &[("BUN_INSTALL_CACHE_DIR", "bun-cache")],
    },
    Manager {
        lockfile: "bun.lockb",
        install: "bun install --frozen-lockfile --ignore-scripts",
        hosts: &["registry.npmjs.org"],
        caches: &[("BUN_INSTALL_CACHE_DIR", "bun-cache")],
    },
];

/// The folder with a lockfile that `cwd` or the first folder above it, up to `worktree`, holds
fn lockfile_folder(worktree: &Path, cwd: &Path) -> Option<(PathBuf, &'static Manager)> {
    cwd.ancestors()
        .take_while(|folder| folder.starts_with(worktree))
        .find_map(|folder| {
            MANAGERS
                .iter()
                .find(|m| folder.join(m.lockfile).is_file())
                .map(|manager| (folder.to_owned(), manager))
        })
}

/// Has `run` install the packages its checkout keeps a lockfile for before it starts the server
///
/// Does nothing where no lockfile is in the server's folder or above it in the checkout. The
/// server gets the time to install only where the packages are not there yet, and a cache
/// that `run` already names is kept.
pub(super) fn before(run: &mut DevRun, worktree: &Path, build: &Path) {
    let Some((folder, manager)) = lockfile_folder(worktree, &run.cwd) else {
        return;
    };
    let mark = build.join(UNFINISHED);
    let server = std::mem::replace(
        &mut run.program,
        crate::spawn::sh().to_string_lossy().into_owned(),
    );
    let mut args = vec![
        "-c".to_owned(),
        SCRIPT.replace("INSTALL", manager.install),
        "kelpie-dev-server".to_owned(),
        folder.display().to_string(),
        mark.display().to_string(),
        server,
    ];
    args.append(&mut run.args);
    run.args = args;
    for (variable, folder) in manager.caches {
        (run.env.entry((*variable).to_owned()))
            .or_insert_with(|| build.join(folder).display().to_string());
    }
    (run.policy.hosts).extend(manager.hosts.iter().map(|&h| h.to_owned()));
    if !folder.join("node_modules").is_dir() || mark.exists() {
        run.ready_within = INSTALL_WITHIN;
    }
}

#[cfg(test)]
mod tests;
