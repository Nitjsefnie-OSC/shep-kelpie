//! `shep kelpie tools install`: four steps, each announced as it starts and
//! as it ends, closing with a table of every tool and how it fared

use std::io::{IsTerminal, Write};
use std::time::Instant;

use super::graph::{self, Asset};
use super::report::{Outcome, Report};
use super::{PACKAGE, Tools, run};

const STEPS: usize = 4;
const CHROMIUM: &str = "Chromium";
const CODE_GRAPH: &str = "codebase-memory-mcp";

/// The programs an install runs, and whether it shows a download's progress
pub(super) struct Setup<'a> {
    pub(super) npm: &'a str,
    pub(super) curl: &'a str,
    pub(super) asset: Option<Asset>,
    pub(super) progress: bool,
}

impl<'a> Setup<'a> {
    /// The install on this machine, with `npm` as the program that fetches the packages
    ///
    /// Progress bars show when stderr is a terminal, which is where `curl` draws them.
    pub(super) fn here(npm: &'a str) -> Self {
        Self {
            npm,
            curl: "curl",
            asset: graph::asset(std::env::consts::OS, std::env::consts::ARCH),
            progress: std::io::stderr().is_terminal(),
        }
    }
}

// What a step that worked says for itself.
struct Done {
    outcome: Outcome,
    note: String,
}

impl Done {
    fn ok(note: impl Into<String>) -> Self {
        Self {
            outcome: Outcome::Ok,
            note: note.into(),
        }
    }
}

impl Tools {
    /// Installs the pinned packages with `npm`, then the browser and the code graph server
    ///
    /// # Errors
    ///
    /// The step that failed, and why.
    pub fn install(&self) -> Result<(), String> {
        self.install_with(&Setup::here("npm"), &mut std::io::stdout().lock())
    }

    // `install` with `setup`'s programs, saying what it does to `out`.
    pub(super) fn install_with(&self, setup: &Setup, out: &mut dyn Write) -> Result<(), String> {
        let pinned = pinned()?;
        let mut report = Report::new(out, STEPS);
        report.header(&self.0);
        for (name, version) in &pinned {
            report.expect(1, name, version);
        }
        report.expect(2, CHROMIUM, "-");
        report.expect(3, CODE_GRAPH, graph::RELEASE);
        let result = self.steps(setup, &pinned, &mut report);
        report.summary();
        result
    }

    fn steps(
        &self,
        setup: &Setup,
        pinned: &[(String, String)],
        report: &mut Report,
    ) -> Result<(), String> {
        let list: Vec<String> = pinned.iter().map(|(n, v)| format!("{n} {v}")).collect();
        report.start(1, &format!("npm packages: {}", list.join(", ")));
        let began = Instant::now();
        let packages = self
            .install_packages(setup.npm)
            .map(|()| Done::ok(format!("done in {} s", began.elapsed().as_secs())));
        conclude(report, 1, "npm packages", packages)?;

        report.start(
            2,
            &format!(
                "{CHROMIUM} for the worker's browser, into {}",
                self.browsers().display()
            ),
        );
        let browser = self.install_browser().and_then(|()| {
            let build = self.chromium_build()?;
            report.learned(2, CHROMIUM, &build);
            Ok(Done::ok(format!("done: {build}")))
        });
        conclude(report, 2, CHROMIUM, browser)?;

        let release = graph::RELEASE;
        let what = match &setup.asset {
            Some(asset) => asset.name.to_owned(),
            None => format!(
                "no build for {} {}",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
        };
        report.start(
            3,
            &format!("code graph server {CODE_GRAPH} {release} ({what})"),
        );
        let graph = self
            .install_code_graph(setup.asset, setup.curl, setup.progress)
            .map(|installed| {
                if installed {
                    Done::ok(format!(
                        "sha256 ok, unpacked to {}",
                        self.code_graph().display()
                    ))
                } else {
                    Done {
                        outcome: Outcome::Skipped("no build for this machine".to_owned()),
                        note: "skipped, the release has no build for this machine, so projects \
                               that turn the code graph on run without it"
                            .to_owned(),
                    }
                }
            });
        conclude(report, 3, "code graph server", graph)
    }

    // The package.json written, and npm run.
    fn install_packages(&self, npm: &str) -> Result<(), String> {
        std::fs::create_dir_all(&self.0)
            .and_then(|()| std::fs::write(self.0.join("package.json"), PACKAGE))
            .map_err(|e| format!("cannot write {}: {e}", self.0.display()))?;
        let args = ["install", "--no-audit", "--no-fund"];
        run(crate::spawn::command(npm).args(args).current_dir(&self.0))
    }
}

// Says how `step` ended, and turns a failure into the error that names it.
fn conclude(
    report: &mut Report,
    step: usize,
    title: &str,
    result: Result<Done, String>,
) -> Result<(), String> {
    match result {
        Ok(done) => {
            report.note(&done.note);
            report.finish(step, &done.outcome);
            Ok(())
        }
        Err(why) => {
            report.note(&format!("failed: {why}"));
            report.finish(step, &Outcome::Failed);
            Err(format!("step {step} of {STEPS}, {title}, failed: {why}"))
        }
    }
}

// The packages `package.json` pins, in the order it lists them.
fn pinned() -> Result<Vec<(String, String)>, String> {
    let json: serde_json::Value =
        serde_json::from_str(PACKAGE).map_err(|e| format!("package.json is not JSON: {e}"))?;
    let dependencies = json["dependencies"]
        .as_object()
        .ok_or("package.json has no dependencies")?;
    dependencies
        .iter()
        .map(|(name, version)| {
            let version = version
                .as_str()
                .ok_or_else(|| format!("package.json pins {name} to no version string"))?;
            Ok((name.clone(), version.to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use shep_kelpie_test_support::write_script;

    // An npm that lays down what a real install of the three packages would:
    // playwright-core with a cli that fetches a Chromium build.
    const NPM: &str = r#"#!/bin/sh
set -e
echo "added 3 packages"
mkdir -p node_modules/playwright-core node_modules/@playwright/mcp
cat > node_modules/playwright-core/cli.js <<'JS'
const fs = require('fs');
const path = require('path');
const build = path.join(process.env.PLAYWRIGHT_BROWSERS_PATH, 'chromium-1200');
if (process.argv.includes('--dry-run')) {
    console.log('Chrome for Testing 1 (playwright chromium v1200)');
    console.log('  Install location:    ' + build);
} else {
    fs.mkdirSync(build);
}
JS
"#;

    // A curl that records what it was asked and writes `$CURL_BODY` where `--output` says.
    const CURL: &str = r#"#!/bin/sh
echo "$@" > "$0.asked"
while [ $# -gt 0 ]; do
    if [ "$1" = "--output" ]; then out="$2"; fi
    shift
done
cp "$0.body" "$out"
"#;

    struct Rig {
        dir: tempfile::TempDir,
        tools: Tools,
        npm: PathBuf,
        curl: PathBuf,
    }

    impl Rig {
        fn new() -> Self {
            Self::with_npm(NPM)
        }

        fn with_npm(script: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let tools = Tools::at(dir.path().join("tools"));
            let npm = dir.path().join("npm");
            write_script(&npm, script);
            let curl = dir.path().join("curl");
            write_script(&curl, CURL);
            let body = dir.path().join("curl.body");
            std::fs::write(&body, "not an archive").unwrap();
            Self {
                dir,
                tools,
                npm,
                curl,
            }
        }

        // A release archive holding the server, which the rig's curl hands over.
        fn release(&self) -> Asset {
            let src = self.dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join(CODE_GRAPH), "#!/bin/sh\nexit 0\n").unwrap();
            let body = self.dir.path().join("curl.body");
            let made = std::process::Command::new("tar")
                .arg("-czf")
                .arg(&body)
                .arg("-C")
                .arg(&src)
                .arg(".")
                .status()
                .unwrap();
            assert!(made.success());
            let sha = graph::digest(&body).unwrap();
            Asset {
                name: "codebase-memory-mcp-test-1.tar.gz",
                sha256: Box::leak(sha.into_boxed_str()),
            }
        }

        fn setup(&self, asset: Option<Asset>, progress: bool) -> Setup<'_> {
            Setup {
                npm: self.npm.to_str().unwrap(),
                curl: self.curl.to_str().unwrap(),
                asset,
                progress,
            }
        }

        fn install(&self, setup: &Setup) -> (Result<(), String>, Vec<String>) {
            let mut out = Vec::new();
            let result = self.tools.install_with(setup, &mut out);
            let text = String::from_utf8(out).unwrap();
            (result, text.lines().map(str::to_owned).collect())
        }

        fn curl_asked(&self) -> String {
            std::fs::read_to_string(format!("{}.asked", self.curl.display())).unwrap()
        }
    }

    // The lines from the one that starts `step` to the one before the next step.
    fn step(lines: &[String], step: usize) -> Vec<&str> {
        let mark = format!("[{step}/{STEPS}]");
        let from = lines.iter().position(|l| l.starts_with(&mark)).unwrap();
        lines[from..]
            .iter()
            .enumerate()
            .take_while(|(i, l)| *i == 0 || !l.starts_with('['))
            .map(|(_, l)| l.as_str())
            .collect()
    }

    fn pinned_in(dir: &Path) -> Vec<(String, String)> {
        let text = std::fs::read_to_string(dir.join("package.json")).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        json["dependencies"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(n, v)| (n.clone(), v.as_str().unwrap().to_owned()))
            .collect()
    }

    #[test]
    fn a_run_names_every_tool_with_its_version_and_ends_with_a_table_of_ok() {
        let rig = Rig::new();
        let asset = rig.release();
        let (result, lines) = rig.install(&rig.setup(Some(asset), false));
        result.unwrap();

        let packages = step(&lines, 1);
        let pinned = pinned_in(rig.tools.dir());
        assert_eq!(pinned.len(), 1, "{pinned:?}");
        for (name, version) in &pinned {
            let named = format!("{name} {version}");
            assert!(packages[0].contains(&named), "{named} not in {packages:?}");
        }
        assert!(
            packages.last().unwrap().trim().starts_with("done in "),
            "{packages:?}"
        );

        let browser = step(&lines, 2);
        assert_eq!(browser.last().unwrap().trim(), "done: chromium-1200");

        let graph = step(&lines, 3);
        assert!(graph[0].contains(CODE_GRAPH), "{graph:?}");
        assert!(graph[0].contains(graph::RELEASE), "{graph:?}");
        assert!(
            graph[0].contains("codebase-memory-mcp-test-1.tar.gz"),
            "{graph:?}"
        );
        assert!(
            graph.last().unwrap().contains("sha256 ok, unpacked to"),
            "{graph:?}"
        );

        let table = step(&lines, 4);
        assert!(table[0].contains("summary"), "{table:?}");
        for (name, version) in &pinned {
            let row = table.iter().find(|l| l.contains(name.as_str())).unwrap();
            assert!(
                row.contains(version.as_str()) && row.ends_with("ok"),
                "{row}"
            );
        }
        for tool in ["Chromium", CODE_GRAPH] {
            let row = table.iter().find(|l| l.contains(tool)).unwrap();
            assert!(row.ends_with("ok"), "{row}");
        }
        let row = table.iter().find(|l| l.contains(CODE_GRAPH)).unwrap();
        assert!(row.contains(graph::RELEASE), "{row}");
        let row = table.iter().find(|l| l.contains("chromium-1200")).unwrap();
        assert!(row.contains("Chromium"), "{row}");
        assert!(rig.tools.has_code_graph());
    }

    #[test]
    fn a_machine_the_release_has_no_build_for_says_so_and_still_succeeds() {
        let rig = Rig::new();
        let (result, lines) = rig.install(&rig.setup(None, false));
        result.unwrap();

        let graph = step(&lines, 3);
        assert!(graph.last().unwrap().contains("skipped"), "{graph:?}");
        let table = step(&lines, 4);
        let row = table.iter().find(|l| l.contains(CODE_GRAPH)).unwrap();
        assert!(row.ends_with("skipped, no build for this machine"), "{row}");
        assert!(!rig.tools.has_code_graph());
    }

    #[test]
    fn a_failed_npm_names_its_step_and_marks_every_later_tool_not_run() {
        let rig = Rig::with_npm("#!/bin/sh\nexit 1\n");
        let (result, lines) = rig.install(&rig.setup(None, false));

        let why = result.unwrap_err();
        assert!(
            why.contains("step 1 of 4") && why.contains("npm packages"),
            "{why}"
        );
        assert!(
            step(&lines, 1).last().unwrap().contains("failed"),
            "{lines:?}"
        );
        let table = step(&lines, 4);
        for (name, _) in pinned_in(rig.tools.dir()) {
            let row = table.iter().find(|l| l.contains(name.as_str())).unwrap();
            assert!(row.ends_with("failed"), "{row}");
        }
        for tool in ["Chromium", CODE_GRAPH] {
            let row = table.iter().find(|l| l.contains(tool)).unwrap();
            assert!(row.ends_with("not run"), "{row}");
        }
    }

    #[test]
    fn a_download_that_is_not_the_pinned_one_fails_its_step_and_keeps_the_tools_before_it_ok() {
        let rig = Rig::new();
        let asset = Asset {
            name: "codebase-memory-mcp-test-1.tar.gz",
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
        };
        let (result, lines) = rig.install(&rig.setup(Some(asset), false));

        let why = result.unwrap_err();
        assert!(
            why.contains("step 3 of 4") && why.contains("code graph server"),
            "{why}"
        );
        let table = step(&lines, 4);
        let row = table.iter().find(|l| l.contains("Chromium")).unwrap();
        assert!(row.ends_with("ok"), "{row}");
        let row = table.iter().find(|l| l.contains(CODE_GRAPH)).unwrap();
        assert!(row.ends_with("failed"), "{row}");
    }

    #[test]
    fn the_code_graph_download_shows_a_bar_in_a_terminal_and_none_in_a_pipe() {
        let asset = Asset {
            name: "codebase-memory-mcp-test-1.tar.gz",
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
        };
        for (progress, bar) in [(true, true), (false, false)] {
            let rig = Rig::new();
            rig.install(&rig.setup(Some(asset), progress))
                .0
                .unwrap_err();
            let asked = rig.curl_asked();
            assert_eq!(asked.contains("--progress-bar"), bar, "{asked}");
            assert_eq!(asked.contains("--silent"), !bar, "{asked}");
        }
    }
}
