//! Kelpie's own tools, under `<kelpie home>/tools`
//!
//! `shep kelpie tools install` fills the folder with the browser a worker's
//! browser runs through, at the version [`PACKAGE`] pins, and the code graph
//! server, at the release [`graph::RELEASE`] pins. The sandbox runtime is no
//! longer among them: crabpot is in kelpie's own binary (shep-pm/shep-kelpie#1217).

use std::path::{Path, PathBuf};
use std::process::Command;

mod browser;
mod graph;
mod install;
mod report;

pub use graph::RELEASE as CODE_GRAPH_RELEASE;

/// Kelpie's own tools, under `<kelpie home>/tools`
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

    fn package(&self, name: &str) -> PathBuf {
        self.0.join("node_modules").join(name)
    }
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
    use shep_kelpie_test_support::{Fake, write_fake};

    // An npm that fails.
    fn failing_npm(dir: &Path) -> PathBuf {
        write_fake(dir, "failing-npm", &Fake::new().exits(1))
    }

    // The browser's server is a third party's, so the install is of the exact
    // version measured, and a range would let a new release change what it dials.
    #[test]
    fn the_install_writes_the_pinned_packages_into_package_json() {
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
        assert_eq!(json["dependencies"]["@playwright/mcp"], "0.0.82", "{text}");
        assert!(
            json["dependencies"]
                .get("@anthropic-ai/sandbox-runtime")
                .is_none(),
            "the sandbox runtime is in kelpie: {text}"
        );
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
