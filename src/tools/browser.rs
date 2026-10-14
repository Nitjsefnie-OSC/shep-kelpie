//! Playwright's MCP server and the Chromium it drives, installed under kelpie's tools
//!
//! The server comes from `package.json` with the sandbox runtime. Chromium is
//! fetched by the server's own `playwright-core` into `browsers`, at the build
//! that release pins, and `headed` needs the full build it installs beside the
//! headless shell.

use std::path::PathBuf;

use super::{Tools, run};

const MCP: &str = "@playwright/mcp";
const CORE: &str = "playwright-core";

impl Tools {
    /// Where Chromium is installed, which the server finds through `PLAYWRIGHT_BROWSERS_PATH`
    pub fn browsers(&self) -> PathBuf {
        self.0.join("browsers")
    }

    /// The Playwright MCP server's entry point, which `node` runs
    pub fn playwright_mcp(&self) -> PathBuf {
        self.package(MCP).join("cli.js")
    }

    fn playwright_cli(&self) -> PathBuf {
        self.package(CORE).join("cli.js")
    }

    /// Whether the server and a Chromium build are installed
    pub fn has_browser(&self) -> bool {
        let chromium = std::fs::read_dir(self.browsers()).is_ok_and(|mut entries| {
            entries
                .any(|e| e.is_ok_and(|e| e.file_name().to_string_lossy().starts_with("chromium-")))
        });
        chromium && self.playwright_mcp().is_file()
    }

    /// The Chromium build folder the server's `playwright-core` pins, such as `chromium-1200`
    ///
    /// # Errors
    ///
    /// Why `browsers.json` could not be read as expected, or that the build is not in
    /// [`Tools::browsers`].
    pub(super) fn chromium_build(&self) -> Result<String, String> {
        let list = self.playwright_cli().with_file_name("browsers.json");
        let text = std::fs::read_to_string(&list)
            .map_err(|e| format!("cannot read {}: {e}", list.display()))?;
        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("{} is not JSON: {e}", list.display()))?;
        let revision = json["browsers"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|b| b["name"] == "chromium")
            .and_then(|b| b["revision"].as_str())
            .ok_or_else(|| format!("{} names no chromium revision", list.display()))?;
        let build = format!("chromium-{revision}");
        if !self.browsers().join(&build).is_dir() {
            return Err(format!(
                "playwright installed no {build} in {}",
                self.browsers().display()
            ));
        }
        Ok(build)
    }

    /// Fetches Chromium into [`Tools::browsers`], with the server's `playwright-core`
    ///
    /// # Errors
    ///
    /// The step that failed, and why.
    pub(super) fn install_browser(&self) -> Result<(), String> {
        let browsers = self.browsers();
        std::fs::create_dir_all(&browsers)
            .map_err(|e| format!("cannot make {}: {e}", browsers.display()))?;
        run(crate::spawn::command("node")
            .arg(self.playwright_cli())
            .args(["install", "chromium"])
            .env("PLAYWRIGHT_BROWSERS_PATH", &browsers)
            .current_dir(&self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(dir: &std::path::Path) -> Tools {
        Tools::at(dir.join("tools"))
    }

    #[test]
    fn the_browser_is_installed_when_the_server_and_a_chromium_build_are_both_there() {
        let dir = tempfile::tempdir().unwrap();
        let tools = fake(dir.path());
        assert!(!tools.has_browser());

        std::fs::create_dir_all(tools.playwright_mcp().parent().unwrap()).unwrap();
        std::fs::write(tools.playwright_mcp(), "").unwrap();
        assert!(!tools.has_browser(), "the server alone is not a browser");

        std::fs::create_dir_all(tools.browsers().join("ffmpeg-1011")).unwrap();
        assert!(!tools.has_browser(), "only Chromium counts");
        std::fs::create_dir_all(tools.browsers().join("chromium-1200")).unwrap();
        assert!(tools.has_browser());
    }

    #[test]
    fn the_install_makes_the_browsers_folder_and_fetches_chromium_into_it() {
        let dir = tempfile::tempdir().unwrap();
        let tools = fake(dir.path());
        let cli = tools.playwright_cli();
        std::fs::create_dir_all(cli.parent().unwrap()).unwrap();
        std::fs::write(
            &cli,
            "require('fs').writeFileSync(require('path').join(process.env.PLAYWRIGHT_BROWSERS_PATH, \
             'asked.txt'), process.argv.slice(2).join(' '));",
        )
        .unwrap();
        tools.install_browser().unwrap();
        let asked = std::fs::read_to_string(tools.browsers().join("asked.txt")).unwrap();
        assert_eq!(asked, "install chromium");
    }

    #[test]
    fn the_chromium_build_is_the_one_playwright_pins_even_beside_a_newer_stale_one() {
        let dir = tempfile::tempdir().unwrap();
        let tools = fake(dir.path());
        let list = tools.playwright_cli().with_file_name("browsers.json");
        std::fs::create_dir_all(list.parent().unwrap()).unwrap();
        std::fs::write(
            &list,
            r#"{"browsers":[{"name":"firefox","revision":"1549"},{"name":"chromium","revision":"1200"}]}"#,
        )
        .unwrap();
        std::fs::create_dir_all(tools.browsers().join("chromium-1300")).unwrap();
        let err = tools.chromium_build().unwrap_err();
        assert!(err.contains("chromium-1200"), "{err}");

        std::fs::create_dir_all(tools.browsers().join("chromium-1200")).unwrap();
        assert_eq!(tools.chromium_build().unwrap(), "chromium-1200");
    }
}
