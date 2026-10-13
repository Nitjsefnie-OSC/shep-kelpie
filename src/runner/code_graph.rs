//! A code graph of a work item's worktree for the calls that read the code
//!
//! A project that turns it on in `code_graph` has each worker turn and each
//! reviewer indexed before the call starts. The index is kept under kelpie's
//! home per work item, and goes with the work item's worktree.

use std::io;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use super::Runner;
use crate::ports::{AgentCall, CodeGraph, Role, Tools};
use crate::settings::AgentHarness;

// An index that takes longer is not worth holding a turn for.
const INDEX_LIMIT: Duration = Duration::from_secs(300);

// How much of an indexer's output a note repeats.
const TAIL: usize = 300;

impl Runner {
    /// Gives `call` the code graph server over its worktree's index, if the
    /// project turned it on and the call reads the code
    ///
    /// A call that cannot have it runs without, and a note says why.
    pub(super) fn with_code_graph(&self, call: &mut AgentCall) {
        if !self.settings.code_graph.enabled || !reads_code(call) {
            return;
        }
        let tools = &self.paths.tools;
        let who = format!("issue #{}'s {}", call.issue, name(call.role));
        if !tools.has_code_graph() {
            self.notes.push(format!(
                "{who} runs without a code graph: {} is not there, so run \
                 `shep kelpie tools install`",
                tools.code_graph().display()
            ));
            return;
        }
        let graph = CodeGraph {
            binary: tools.code_graph(),
            index: self.paths.code_graph(call.issue),
        };
        match index(&graph, &call.cwd) {
            Ok(()) => call.reach.code_graph = Some(graph),
            Err(e) => self
                .notes
                .push(format!("{who} runs without a code graph: {e}")),
        }
    }

    /// Removes the index of work item `issue`'s worktree
    ///
    /// # Errors
    ///
    /// The folder that could not be removed, and why.
    pub(super) fn remove_code_graph(&self, issue: u64) -> Result<(), String> {
        let folder = self.paths.code_graph(issue);
        match std::fs::remove_dir_all(&folder) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                Err(format!("cannot remove {}: {e}", folder.display()))
            }
            _ => Ok(()),
        }
    }
}

// The worker's turns and the reviewers read the code. The retro, the advisors and
// the project manager do not, and a harness that loads no MCP config cannot use it.
fn reads_code(call: &AgentCall) -> bool {
    let harness = match call.harness {
        AgentHarness::ClaudeCode => true,
        AgentHarness::Pi(_) | AgentHarness::Codex => false,
        #[cfg(test)]
        AgentHarness::StandIn => true,
    };
    harness
        && matches!(
            (call.role, call.tools),
            (Role::Worker, Tools::Work) | (Role::Reviewer, Tools::Review)
        )
}

fn name(role: Role) -> &'static str {
    match role {
        Role::Worker => "worker",
        Role::Reviewer => "reviewer",
        Role::IssueWriter => "issue writer",
        Role::Pm => "project manager",
        Role::Advisor => "advisor",
        Role::Researcher => "researcher",
    }
}

// Indexes `worktree` into the graph's folder, which the server reads back.
// Run again for each call, since the server only watches what it is running over.
fn index(graph: &CodeGraph, worktree: &Path) -> Result<(), String> {
    std::fs::create_dir_all(&graph.index)
        .map_err(|e| format!("cannot make {}: {e}", graph.index.display()))?;
    let log = graph.index.join("kelpie-index.log");
    let output =
        std::fs::File::create(&log).map_err(|e| format!("cannot write {}: {e}", log.display()))?;
    let more = output
        .try_clone()
        .map_err(|e| format!("cannot write {}: {e}", log.display()))?;
    let mut child = crate::spawn::command(&graph.binary)
        .args(["cli", "index_repository", "--repo-path"])
        .arg(worktree)
        .env("CBM_CACHE_DIR", &graph.index)
        .env("CBM_ALLOWED_ROOT", worktree)
        .current_dir(worktree)
        .stdin(Stdio::null())
        .stdout(output)
        .stderr(more)
        .spawn()
        .map_err(|e| format!("cannot run {}: {e}", graph.binary.display()))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < INDEX_LIMIT => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("indexing took over {}s", INDEX_LIMIT.as_secs()));
            }
            Err(e) => return Err(format!("cannot wait for the indexer: {e}")),
        }
    };
    if status.success() {
        return Ok(());
    }
    let said = std::fs::read_to_string(&log).unwrap_or_default();
    let said = said.trim();
    let from = said.len().saturating_sub(TAIL);
    let tail = said.get(from..).unwrap_or(said);
    Err(format!("indexing failed with {status}: {tail}"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex;

    use super::*;
    use crate::ports::CLEAN;
    use crate::runner::{CHECKS_SETTLE, step};
    use crate::test::{Rig, Scripted, write_script};

    // A server that indexes by writing a file into its cache folder.
    const INDEXER: &str = "#!/bin/sh\n\
        echo \"$@\" > \"$CBM_CACHE_DIR/indexed-with\"\n\
        echo \"$CBM_ALLOWED_ROOT\" > \"$CBM_CACHE_DIR/root\"\n";

    fn turn_on(rig: &Rig) {
        let off = "[app.dogs.kelpie.code_graph]\nenabled = false";
        rig.edit_settings(|s| {
            assert!(s.contains(off), "the example's code graph moved");
            s.replace(off, "[app.dogs.kelpie.code_graph]\nenabled = true")
        });
    }

    fn install(rig: &Rig, script: &str) {
        let binary = rig.paths().tools.code_graph();
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        write_script(&binary, script);
    }

    // Issue 7's worker turn and a Claude reviewer's round after it.
    fn work_and_review(rig: &Rig) -> Mutex<Runner> {
        let runner = rig.open().unwrap();
        rig.ask(&runner, "add", Some("7"));
        rig.forge.open_pull_request(71, "kelpie/7", &[7]);
        rig.claude
            .script([Scripted::Push("work.txt", "work\n"), Scripted::Text(CLEAN)]);
        step(&runner).unwrap(); // the worker's first turn
        step(&runner).unwrap(); // round 1, qwen: clean by default
        step(&runner).unwrap(); // round 2, claude: scripted clean above
        runner
    }

    fn reads_code(rig: &Rig) -> Vec<crate::test::Seen> {
        (rig.claude.all_seen().into_iter())
            .filter(|s| matches!(s.call.role, Role::Worker | Role::Reviewer))
            .collect()
    }

    fn mcp_file(seen: &crate::test::Seen) -> PathBuf {
        seen.call.settings.with_extension("mcp.json")
    }

    #[test]
    fn a_project_with_the_graph_on_gives_its_worker_and_reviewer_the_server_and_an_index_under_kelpies_home()
     {
        let rig = Rig::new("shep");
        turn_on(&rig);
        install(&rig, INDEXER);
        work_and_review(&rig);

        let index = rig.paths().folder.join("code-graphs/7");
        let seen = reads_code(&rig);
        let roles: Vec<Role> = seen.iter().map(|s| s.call.role).collect();
        assert!(roles.contains(&Role::Worker) && roles.contains(&Role::Reviewer));
        for seen in &seen {
            let graph = seen.call.reach.code_graph.as_ref().expect("the server");
            assert_eq!(graph.index, index);
            assert_eq!(graph.binary, rig.paths().tools.code_graph());
            let config: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(mcp_file(seen)).unwrap()).unwrap();
            let server = &config["mcpServers"]["codebase-memory-mcp"];
            assert_eq!(server["env"]["CBM_CACHE_DIR"], index.to_str().unwrap());
            assert_eq!(
                seen.settings["permissions"]["allow"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|a| *a == "mcp__codebase-memory-mcp")
                    .count(),
                1,
                "{:?}",
                seen.call.role
            );
        }
        assert!(index.starts_with(&rig.paths().kelpie_home));
        let indexed = std::fs::read_to_string(index.join("indexed-with")).unwrap();
        assert!(
            indexed.starts_with("cli index_repository --repo-path "),
            "{indexed}"
        );
        let root = std::fs::read_to_string(index.join("root")).unwrap();
        assert_eq!(
            PathBuf::from(root.trim()),
            seen[0].call.cwd,
            "the server is held to the worktree"
        );
    }

    #[test]
    fn a_project_with_the_graph_off_gets_no_server_and_no_index() {
        let rig = Rig::new("shep");
        install(&rig, INDEXER);
        work_and_review(&rig);

        let seen = reads_code(&rig);
        assert!(seen.len() >= 2);
        for seen in &seen {
            assert_eq!(seen.call.reach.code_graph, None);
            assert!(!mcp_file(seen).exists());
            assert!(!seen.settings.to_string().contains("mcp__"));
        }
        assert!(!rig.paths().folder.join("code-graphs").exists());
    }

    #[test]
    fn a_server_that_is_not_installed_is_a_logged_skip_and_the_calls_run() {
        let rig = Rig::new("shep");
        turn_on(&rig);
        let runner = work_and_review(&rig);

        let seen = reads_code(&rig);
        assert!(seen.len() >= 2, "the turns ran");
        assert!(seen.iter().all(|s| s.call.reach.code_graph.is_none()));
        let notes = runner.lock().unwrap().take_notes();
        let skip = notes
            .iter()
            .find(|n| n.contains("runs without a code graph"))
            .expect("a note");
        assert!(skip.contains("tools install"), "{skip}");
    }

    #[test]
    fn an_indexer_that_fails_is_a_logged_skip_naming_why() {
        let rig = Rig::new("shep");
        turn_on(&rig);
        install(
            &rig,
            "#!/bin/sh\necho 'no room for the index' >&2\nexit 3\n",
        );
        let runner = work_and_review(&rig);

        assert!(
            reads_code(&rig)
                .iter()
                .all(|s| s.call.reach.code_graph.is_none())
        );
        let notes = runner.lock().unwrap().take_notes();
        let skip = notes
            .iter()
            .find(|n| n.contains("runs without a code graph"))
            .expect("a note");
        assert!(skip.contains("no room for the index"), "{skip}");
    }

    #[test]
    fn the_index_goes_when_the_work_item_finishes_and_its_worktree_goes() {
        let (rig, runner, _) = Rig::parked_set("shep", |rig| {
            turn_on(rig);
            install(rig, INDEXER);
        });
        let index = rig.paths().folder.join("code-graphs/7");
        assert!(
            index.join("indexed-with").is_file(),
            "the worker's turn indexed"
        );

        rig.ask(&runner, "rule", Some("1 yes"));
        step(&runner).unwrap(); // marked ready
        rig.clock.advance(CHECKS_SETTLE);
        let finished = step(&runner).unwrap();
        assert!(matches!(
            finished,
            Some(crate::runner::StepReport::Finished { issue: 7, .. })
        ));
        assert!(!index.exists());
    }
}
