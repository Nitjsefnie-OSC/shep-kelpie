//! Kelpie's own tools, under `<kelpie home>/tools`
//!
//! `shep kelpie tools install` fills the folder with the sandbox runtime every
//! agent runs in, at the version [`PACKAGE`] pins, and on macOS a copy of it
//! with the keychain's services cut from its profile. It also fetches the code
//! graph server, at the release [`graph::RELEASE`] pins. The packages pi loads
//! as extensions, and the MCP server a researcher on goose has its web from, come with the
//! runtime, from the same `package.json`.

use std::path::{Path, PathBuf};
use std::process::Command;

mod goose_packages;
mod graph;
mod pi_packages;

pub use goose_packages::{WEB_SERVER as GOOSE_WEB, WEB_SERVER_FILE as GOOSE_WEB_FILE};
pub use graph::RELEASE as CODE_GRAPH_RELEASE;
pub use pi_packages::WEB_ACCESS as PI_WEB_ACCESS;

/// Kelpie's own copy of the sandbox runtime, under `<kelpie home>/tools`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools(PathBuf);

/// The `package.json` kelpie installs its tools from
pub const PACKAGE: &str = include_str!("tools/package.json");

impl Tools {
    /// The tools under `kelpie_home`
    pub fn under(kelpie_home: &Path) -> Self {
        Self::at(kelpie_home.join("tools"))
    }

    /// The tools in `dir` itself
    pub fn at(dir: PathBuf) -> Self {
        Self(dir)
    }

    /// Installs the pinned packages with `npm`
    ///
    /// # Errors
    ///
    /// The step that failed, and why.
    pub fn install(&self) -> Result<(), String> {
        self.install_with("npm")
    }

    // `install` with `npm` as the program that fetches the packages.
    fn install_with(&self, npm: &str) -> Result<(), String> {
        std::fs::create_dir_all(&self.0)
            .and_then(|()| std::fs::write(self.0.join("package.json"), PACKAGE))
            .map_err(|e| format!("cannot write {}: {e}", self.0.display()))?;
        // The copy of the last install is of the last version. It goes before npm
        // can fail, so a failed install leaves none for a call to find blocked.
        self.remove_keychain_copy()?;
        let args = ["install", "--no-audit", "--no-fund"];
        run(crate::spawn::command(npm).args(args).current_dir(&self.0))?;
        // Only macOS has a keychain, and only its profile names the services.
        if cfg!(target_os = "macos") {
            self.block_keychain()?;
        }
        if !self.install_code_graph()? {
            eprintln!(
                "the code graph server has no build for this machine, so projects that turn \
                 it on run without it"
            );
        }
        Ok(())
    }

    /// The folder itself
    #[inline]
    pub fn dir(&self) -> &Path {
        &self.0
    }

    /// The sandbox runtime's command line, `srt`
    pub fn sandbox(&self) -> PathBuf {
        self.package(PACKAGE_NAME).join(CLI)
    }

    /// The same runtime with the two macOS keychain services out of its profile
    ///
    /// `srt` writes `com.apple.securityd.xpc` and `com.apple.SecurityServer`
    /// into every macOS profile, and its settings cannot take them out. With
    /// them a sandboxed command can ask for a login that the `security`
    /// command stored, with no prompt. A repo kelpie's App does not cover
    /// keeps them, since its worker pushes with the keychain's login.
    pub fn sandbox_without_keychain(&self) -> PathBuf {
        self.package(BLOCKED_NAME).join(CLI)
    }

    /// Whether the copy of the runtime without the keychain services is
    /// installed and has neither in its profile
    pub fn keychain_blocked(&self) -> bool {
        let profile = self.package(BLOCKED_NAME).join(PROFILE);
        let Ok(text) = std::fs::read_to_string(profile) else {
            return false;
        };
        self.sandbox_without_keychain().is_file()
            && !KEYCHAIN_SERVICES
                .iter()
                .any(|service| text.contains(service))
    }

    /// Makes the copy of the runtime that [`Tools::sandbox_without_keychain`] names
    ///
    /// # Errors
    ///
    /// Why the runtime's profile could not be read as expected, or the copy not made.
    pub(crate) fn block_keychain(&self) -> Result<(), String> {
        let to = self.package(BLOCKED_NAME);
        // The last install's copy goes first, so a step that fails leaves none
        // to be found blocked, whether it is the last one's or half of this one.
        self.remove_keychain_copy()?;
        let made = self.copy_without_keychain(&to);
        if made.is_err() {
            let _ = std::fs::remove_dir_all(&to);
        }
        made
    }

    fn remove_keychain_copy(&self) -> Result<(), String> {
        let to = self.package(BLOCKED_NAME);
        match std::fs::remove_dir_all(&to) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("cannot remove {}: {e}", to.display()))
            }
            _ => Ok(()),
        }
    }

    fn copy_without_keychain(&self, to: &Path) -> Result<(), String> {
        let from = self.package(PACKAGE_NAME);
        let profile = from.join(PROFILE);
        let text = std::fs::read_to_string(&profile)
            .map_err(|e| format!("cannot read {}: {e}", profile.display()))?;
        let patched = without_keychain(&text)?;
        run(crate::spawn::command("cp").arg("-R").arg(&from).arg(to))?;
        let copy = to.join(PROFILE);
        std::fs::write(&copy, patched)
            .map_err(|e| format!("cannot write {}: {e}", copy.display()))?;
        // A line cut from the middle of an expression would break the file,
        // and the first call to find out would be a worker's.
        run(crate::spawn::command("node").arg("--check").arg(&copy))
    }

    fn package(&self, name: &str) -> PathBuf {
        self.0.join("node_modules").join(name)
    }
}

const PACKAGE_NAME: &str = "@anthropic-ai/sandbox-runtime";
const BLOCKED_NAME: &str = "@anthropic-ai/sandbox-runtime-no-keychain";
const CLI: &str = "dist/cli.js";
const PROFILE: &str = "dist/sandbox/macos-sandbox-utils.js";
const KEYCHAIN_SERVICES: [&str; 2] = ["com.apple.securityd.xpc", "com.apple.SecurityServer"];

// `profile` without the line that names each keychain service. A runtime that
// names one on any other number of lines than one is not the one measured
// (0.0.77 names each on one line), so nothing is cut from it.
fn without_keychain(profile: &str) -> Result<String, String> {
    for service in KEYCHAIN_SERVICES {
        let named = profile.lines().filter(|l| l.contains(service)).count();
        if named != 1 {
            return Err(format!(
                "{PACKAGE_NAME} names {service} on {named} lines of {PROFILE}, not one: \
                 read what changed in its profile before kelpie cuts anything from it"
            ));
        }
    }
    let mut kept = String::with_capacity(profile.len());
    for line in profile.lines() {
        if !KEYCHAIN_SERVICES
            .iter()
            .any(|service| line.contains(service))
        {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    Ok(kept)
}

// Runs `command` with its output passed through, as an install shows progress.
fn run(command: &mut Command) -> Result<(), String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let status = command
        .status()
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    if !status.success() {
        return Err(format!("{program} failed: {status}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The shape of srt's profile builder around the two services.
    const PROFILE: &str = "\
const lines = [
    '(allow mach-lookup',
    '  (global-name \"com.apple.bsd.dirhelper\")',
    '  (global-name \"com.apple.securityd.xpc\")',
    '  (global-name \"com.apple.coreservices.launchservicesd\")',
    ')',
    '(allow file-ioctl (literal \"/dev/null\"))',
    '(allow mach-lookup (global-name \"com.apple.SecurityServer\"))',
    '(allow ipc-posix-shm)',
];
";

    fn utils(tools: &Tools) -> PathBuf {
        let cli = tools.sandbox();
        cli.with_file_name("sandbox/macos-sandbox-utils.js")
    }

    fn installed(dir: &Path, profile: &str) -> Tools {
        let tools = Tools::at(dir.to_owned());
        std::fs::create_dir_all(utils(&tools).parent().unwrap()).unwrap();
        std::fs::write(utils(&tools), profile).unwrap();
        std::fs::write(tools.sandbox(), "// srt").unwrap();
        tools
    }

    #[test]
    fn the_profile_loses_its_two_keychain_services_and_nothing_else() {
        let patched = without_keychain(PROFILE).unwrap();
        assert!(!patched.contains("securityd"), "{patched}");
        assert!(!patched.contains("SecurityServer"), "{patched}");
        let kept: Vec<_> = PROFILE
            .lines()
            .filter(|l| !l.contains("securityd") && !l.contains("SecurityServer"))
            .collect();
        assert_eq!(patched.lines().collect::<Vec<_>>(), kept);
        assert!(patched.ends_with('\n'));
    }

    #[test]
    fn a_profile_that_does_not_name_each_service_once_is_not_patched() {
        let twice = format!("{PROFILE}{}\n", PROFILE.lines().nth(3).unwrap());
        let moved = PROFILE.replace("com.apple.SecurityServer", "com.apple.Elsewhere");
        for profile in [twice, moved, String::new()] {
            let err = without_keychain(&profile).unwrap_err();
            assert!(err.contains(PACKAGE_NAME), "{err}");
        }
    }

    #[test]
    fn the_blocked_copy_is_beside_the_original_which_keeps_its_services() {
        let dir = tempfile::tempdir().unwrap();
        let tools = installed(dir.path(), PROFILE);
        assert!(!tools.keychain_blocked(), "nothing patched yet");
        tools.block_keychain().unwrap();
        assert!(tools.keychain_blocked());
        assert!(tools.sandbox_without_keychain().is_file());
        assert_ne!(tools.sandbox_without_keychain(), tools.sandbox());
        assert_eq!(std::fs::read_to_string(utils(&tools)).unwrap(), PROFILE);
        // A second install replaces the copy.
        tools.block_keychain().unwrap();
        assert!(tools.keychain_blocked());
    }

    // An install that stops partway must not leave the last install's copy, or
    // half of this one, for a call to find blocked.
    #[test]
    fn an_install_that_fails_leaves_no_copy_behind() {
        let dir = tempfile::tempdir().unwrap();
        let tools = installed(dir.path(), PROFILE);
        tools.block_keychain().unwrap();
        assert!(tools.keychain_blocked());

        // A runtime whose profile no longer names a service once.
        let renamed = PROFILE.replace("com.apple.SecurityServer", "com.apple.Elsewhere");
        std::fs::write(utils(&tools), renamed).unwrap();
        tools.block_keychain().unwrap_err();
        assert!(!tools.keychain_blocked(), "the old copy was left");
        assert!(!tools.sandbox_without_keychain().exists());

        // A profile that stops parsing once the lines are cut.
        let unclosed = "call(\n'com.apple.securityd.xpc'\n'com.apple.SecurityServer'\n";
        std::fs::write(utils(&tools), unclosed).unwrap();
        tools.block_keychain().unwrap_err();
        assert!(
            !tools.keychain_blocked(),
            "a copy that does not parse was left"
        );
    }

    // A pinned srt that npm cannot fetch leaves the old package, and a copy of
    // the old one is not the runtime the main one now is.
    #[test]
    fn an_install_whose_npm_fails_leaves_the_last_installs_copy_gone() {
        let dir = tempfile::tempdir().unwrap();
        let tools = installed(dir.path(), PROFILE);
        tools.block_keychain().unwrap();
        assert!(tools.keychain_blocked());
        let npm = dir.path().join("failing-npm");
        crate::test::write_script(&npm, "#!/bin/sh\nexit 1\n");
        tools.install_with(npm.to_str().unwrap()).unwrap_err();
        assert!(!tools.keychain_blocked());
        assert!(!tools.sandbox_without_keychain().exists());
    }

    // The install is of the exact version measured, not of a range a new
    // release of a package that reaches the web could move.
    #[test]
    fn the_install_writes_the_pinned_web_package_into_package_json() {
        let dir = tempfile::tempdir().unwrap();
        let tools = Tools::at(dir.path().join("tools"));
        let npm = dir.path().join("failing-npm");
        crate::test::write_script(&npm, "#!/bin/sh\nexit 1\n");
        tools.install_with(npm.to_str().unwrap()).unwrap_err();

        let text = std::fs::read_to_string(tools.dir().join("package.json")).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["dependencies"][PI_WEB_ACCESS], "0.40.0", "{text}");
        assert_eq!(json["dependencies"][PACKAGE_NAME], "0.0.77", "{text}");
    }

    // The web server a goose researcher runs is a third party's, so the install is of the
    // exact version measured, and a range would let a new release change what it dials.
    #[test]
    fn the_install_writes_the_pinned_goose_web_packages_into_package_json() {
        let dir = tempfile::tempdir().unwrap();
        let tools = Tools::at(dir.path().join("tools"));
        let npm = dir.path().join("failing-npm");
        crate::test::write_script(&npm, "#!/bin/sh\nexit 1\n");
        tools.install_with(npm.to_str().unwrap()).unwrap_err();

        let text = std::fs::read_to_string(tools.dir().join("package.json")).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["dependencies"][GOOSE_WEB], "2.0.3", "{text}");
        for (name, version) in json["dependencies"].as_object().unwrap() {
            let parts: Vec<_> = version.as_str().unwrap().split('.').collect();
            let exact = parts.len() == 3
                && parts
                    .iter()
                    .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
            assert!(exact, "{name} is pinned to {version}, not an exact version");
        }
    }

    #[test]
    fn a_copy_with_either_service_back_in_it_is_not_blocked() {
        let dir = tempfile::tempdir().unwrap();
        let tools = installed(dir.path(), PROFILE);
        tools.block_keychain().unwrap();
        let copy = tools
            .sandbox_without_keychain()
            .with_file_name("sandbox/macos-sandbox-utils.js");
        std::fs::write(copy, PROFILE).unwrap();
        assert!(!tools.keychain_blocked());
    }
}
