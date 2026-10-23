//! Kelpie's own tools, under `<kelpie home>/tools`
//!
//! `shep kelpie tools install` fills the folder with the sandbox runtime every
//! agent runs in, at the version [`PACKAGE`] pins. It also fetches the code
//! graph server, at the release [`graph::RELEASE`] pins, and Chromium for the
//! Playwright MCP server a worker's browser runs. The packages pi loads
//! as extensions, and the MCP server a researcher on goose has its web from, come with the
//! runtime, from the same `package.json`.

use std::path::{Path, PathBuf};
use std::process::Command;

mod browser;
mod goose_packages;
mod graph;
mod install;
mod pi_packages;
mod report;

pub use goose_packages::{WEB_SERVER as GOOSE_WEB, WEB_SERVER_FILE as GOOSE_WEB_FILE};
pub use graph::RELEASE as CODE_GRAPH_RELEASE;
pub use pi_packages::WEB_ACCESS as PI_WEB_ACCESS;

/// The name goose loads the web package's server under, which its tools are prefixed with
pub const GOOSE_WEB_SERVER: &str = "kelpie-web";

/// Kelpie's own copy of the sandbox runtime, under `<kelpie home>/tools`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools(PathBuf);

/// The `package.json` kelpie installs its tools from
pub const PACKAGE: &str = include_str!("package.json");

impl Tools {
    /// The tools under `kelpie_home`
    pub fn under(kelpie_home: &Path) -> Self {
        Self::at(kelpie_home.join("tools"))
    }

    /// The tools in `dir` itself
    pub fn at(dir: PathBuf) -> Self {
        Self(dir)
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

    fn package(&self, name: &str) -> PathBuf {
        self.0.join("node_modules").join(name)
    }
}

const PACKAGE_NAME: &str = "@anthropic-ai/sandbox-runtime";
const CLI: &str = "dist/cli.js";

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
    use shep_kelpie_test_support::{Fake, write_fake};

    // An npm that fails.
    fn failing_npm(dir: &Path) -> PathBuf {
        write_fake(dir, "failing-npm", &Fake::new().exits(1))
    }

    // The install is of the exact version measured, not of a range a new
    // release of a package that reaches the web could move.
    #[test]
    fn the_install_writes_the_pinned_web_package_into_package_json() {
        let dir = tempfile::tempdir().unwrap();
        let tools = Tools::at(dir.path().join("tools"));
        let npm = failing_npm(dir.path());
        tools
            .install_with(
                &install::Setup::here(npm.to_str().unwrap()),
                &mut Vec::new(),
            )
            .unwrap_err();

        let text = std::fs::read_to_string(tools.dir().join("package.json")).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["dependencies"][PI_WEB_ACCESS], "0.40.0", "{text}");
        assert_eq!(json["dependencies"][PACKAGE_NAME], "0.0.81", "{text}");
        assert_eq!(json["dependencies"]["@playwright/mcp"], "0.0.82", "{text}");
    }

    // The web server a goose researcher runs is a third party's, so the install is of the
    // exact version measured, and a range would let a new release change what it dials.
    #[test]
    fn the_install_writes_the_pinned_goose_web_packages_into_package_json() {
        let dir = tempfile::tempdir().unwrap();
        let tools = Tools::at(dir.path().join("tools"));
        let npm = failing_npm(dir.path());
        tools
            .install_with(
                &install::Setup::here(npm.to_str().unwrap()),
                &mut Vec::new(),
            )
            .unwrap_err();

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
}
