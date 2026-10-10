//! The login a worker holds on a repo kelpie's App covers
//!
//! Its token is in a `gh` config folder of its own in the build folder, in the
//! `hosts.yml` that `gh` reads and that git's credential helper, `gh auth
//! git-credential`, answers from. No variable carries it: Codex drops any
//! whose name holds `KEY`, `SECRET` or `TOKEN` from the commands it runs, and
//! a variable shows in every process's environment. Git's own helpers are
//! emptied, which drops the keychain's, and github.com's is `gh`'s, so
//! nothing the maintainer set up is asked.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use crate::github::InstallationToken;

// The account the token is kept under, and the user git sends with it. `gh`
// reads the older shape, a token directly under the host, only after asking
// GitHub whose it is to migrate it, which a worker's fence may not allow.
const USER: &str = "x-access-token";

/// The folder `gh` reads its config from, in `build`
pub(super) fn config_dir(build: &Path) -> PathBuf {
    build.join("gh")
}

/// Writes `token` where the worker's `gh` finds it, in `build`, readable by
/// the user alone and replacing the last call's
///
/// # Errors
///
/// The error of making the folder or writing the file.
pub fn write(build: &Path, token: &InstallationToken) -> io::Result<()> {
    let dir = config_dir(build);
    fs::create_dir_all(&dir)?;
    let text = format!(
        "github.com:\n    git_protocol: https\n    user: {USER}\n    oauth_token: {token}\n    \
         users:\n        {USER}:\n            oauth_token: {token}\n",
        token = token.expose()
    );
    // The version says the config is already in the shape `gh` migrates to.
    fs::write(dir.join("config.yml"), "version: \"1\"\n")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(dir.join("hosts.yml"))?;
    file.write_all(text.as_bytes())
}

/// The variables that point `gh` and git at that login
///
/// `GIT_CONFIG_PARAMETERS` is what `git -c` sets for the commands it runs. Git
/// reads it after every config file, so the helper list is emptied after
/// whatever the maintainer's own files named, and github.com's is the only one
/// left. None of the names holds `KEY`, `SECRET` or `TOKEN`.
pub(super) fn env(build: &Path) -> Vec<(String, String)> {
    let pair = |name: &str, value: &str| (name.to_owned(), value.to_owned());
    vec![
        pair("GH_CONFIG_DIR", &config_dir(build).to_string_lossy()),
        pair("GH_PROMPT_DISABLED", "1"),
        pair("GIT_TERMINAL_PROMPT", "0"),
        pair(
            "GIT_CONFIG_PARAMETERS",
            "'credential.helper'='' \
             'credential.https://github.com.helper'='!gh auth git-credential'",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};

    use super::*;

    fn token(text: &str) -> InstallationToken {
        InstallationToken::new(text.to_owned())
    }

    #[test]
    fn the_token_is_written_for_the_user_alone_and_replaced_by_the_next_calls() {
        let build = tempfile::tempdir().unwrap();

        write(build.path(), &token("ghs_first")).unwrap();
        write(build.path(), &token("ghs_second")).unwrap();

        let file = config_dir(build.path()).join("hosts.yml");
        let text = fs::read_to_string(&file).unwrap();
        assert!(text.contains("oauth_token: ghs_second"), "{text}");
        assert!(!text.contains("ghs_first"), "{text}");
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    // Measured with git and gh themselves: a helper the maintainer set up, the
    // keychain's among them, is not asked, and github.com is answered with the
    // token from the folder the worker's `gh` reads, from nothing in a variable.
    #[test]
    fn git_pushes_with_the_token_and_asks_none_of_the_helpers_the_maintainer_set_up() {
        let (build, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        write(build.path(), &token("ghs_scoped")).unwrap();
        let global = home.path().join("gitconfig");
        fs::write(
            &global,
            "[credential]\n\thelper = !f() { echo username=maintainer; echo password=their-login; }; f\n",
        )
        .unwrap();
        let mut git = Command::new("git");
        git.args(["credential", "fill"])
            .env("GIT_CONFIG_GLOBAL", &global)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .envs(env(build.path()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut git = git.spawn().unwrap();
        git.stdin
            .take()
            .unwrap()
            .write_all(b"protocol=https\nhost=github.com\n\n")
            .unwrap();
        let out = git.wait_with_output().unwrap();
        let answer = String::from_utf8_lossy(&out.stdout);

        assert!(
            answer.contains("password=ghs_scoped\n"),
            "{answer} {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!answer.contains("their-login"), "{answer}");
    }
}
