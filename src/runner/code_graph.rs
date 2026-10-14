//! A code graph of a work item's worktree for the calls that read the code
//!
//! A project that turns it on in `code_graph` has each worker turn and each
//! reviewer indexed before the call starts. The index is kept under kelpie's
//! home per work item, and goes with the work item's worktree.
//!
//! The call is prepared with the runner's lock held, and indexing takes
//! seconds to minutes, so it runs on the call's own thread, where a stop
//! ends it.

use std::io;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use super::Runner;
use super::claim::Notes;
use crate::ports::{AgentCall, CodeGraph, Ending, Role, Tools};
use crate::settings::AgentHarness;

// An index that takes longer is not worth holding a turn for.
const INDEX_LIMIT: Duration = Duration::from_secs(300);

// The standing warnings: told once however many calls meet them.
const NOT_INSTALLED: &str = "code-graph-not-installed";
const INDEX_FAILED: &str = "code-graph-index-failed";

// How much of an indexer's output a note repeats.
const TAIL: usize = 300;

impl Runner {
    /// Names the code graph server and its index in `call`'s reach, if the
    /// project turned it on and the call reads the code
    ///
    /// The index is built by [`indexed`], once the call's thread has started.
    /// A call that cannot have the server runs without, and a note says why.
    pub(super) fn with_code_graph(&self, call: &mut AgentCall) {
        if !self.settings.code_graph.enabled || !reads_code(call) {
            return;
        }
        let tools = &self.paths.tools;
        if !tools.has_code_graph() {
            self.notes.standing(
                NOT_INSTALLED,
                format!(
                    "{} runs without a code graph: {} is not there, so run \
                     `shep kelpie tools install`",
                    who(call),
                    tools.code_graph().display()
                ),
            );
            return;
        }
        self.notes.ends(NOT_INSTALLED);
        call.reach.code_graph = Some(CodeGraph {
            binary: tools.code_graph(),
            index: self.paths.code_graph(call.issue),
        });
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

    // A server pi, Codex or goose cannot load is left out, and a note says which and why. goose
    // loads one with env keys it skips, without them, and a note says which.
    pub(super) fn without_servers_the_harness_cannot_load(&self, call: &mut AgentCall) {
        // parity: harness.mcp-servers
        let refusal: fn(&str, &serde_json::Value) -> Option<String> = match call.harness {
            AgentHarness::Pi(_) => crate::adapters::pi_mcp_refusal,
            AgentHarness::Codex => crate::adapters::codex_mcp_refusal,
            AgentHarness::Goose(_) => crate::adapters::goose_mcp_refusal,
            _ => return,
        };
        let who = who(call);
        call.reach.mcp_servers.retain(|name, entry| {
            let refused = refusal(name, entry);
            if let Some(why) = &refused {
                self.notes
                    .push(format!("{who} runs without the MCP server `{name}`: {why}"));
            }
            refused.is_none()
        });
        // parity: harness.mcp-servers
        if matches!(call.harness, AgentHarness::Goose(_)) {
            for (name, entry) in &call.reach.mcp_servers {
                if let Some(skipped) = crate::adapters::goose_mcp_skipped_env(name, entry) {
                    self.notes.push(format!(
                        "{who} runs the MCP server `{name}` without {skipped}"
                    ));
                }
            }
        }
    }
}

// The worker's turns and the reviewers read the code. The retro, the advisors and
// the project manager do not, and a harness that loads no MCP servers cannot use it.
fn reads_code(call: &AgentCall) -> bool {
    call.harness.loads_mcp()
        && matches!(
            (call.role, call.tools),
            (Role::Worker, Tools::Work) | (Role::Reviewer, Tools::Review)
        )
}

pub(super) fn who(call: &AgentCall) -> String {
    format!("issue #{}'s {}", call.issue, name(call.role))
}

/// Indexes `call`'s worktree for the server its reach names, on the call's
/// own thread
///
/// A call whose index cannot be built, or is ended while it builds, loses the
/// server and runs without it, and a note says why.
pub(super) fn indexed(call: &mut AgentCall, notes: &Notes, ending: &Ending) {
    let Some(graph) = &call.reach.code_graph else {
        return;
    };
    match index(graph, &call.cwd, ending) {
        Ok(()) => notes.ends(INDEX_FAILED),
        Err(e) => {
            let line = format!("{} runs without a code graph: {e}", who(call));
            // A call ended while it indexes says nothing of the graph itself.
            if ending.asked() {
                notes.push(line);
            } else {
                notes.standing(INDEX_FAILED, line);
            }
            call.reach.code_graph = None;
        }
    }
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
// A call asked to end kills the indexer.
fn index(graph: &CodeGraph, worktree: &Path, ending: &Ending) -> Result<(), String> {
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
            Ok(None) if !ending.asked() && started.elapsed() < INDEX_LIMIT => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match ending.asked() {
                    true => "the call was ended while indexing".to_owned(),
                    false => format!("indexing took over {}s", INDEX_LIMIT.as_secs()),
                });
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

    use serde_json::json;

    use super::*;
    use crate::ports::CLEAN;
    use crate::runner::{CHECKS_SETTLE, step};
    use crate::settings::AgentHarness;
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
    fn a_pi_worker_and_reviewer_get_the_server_and_an_index() {
        let rig = Rig::new("shep");
        turn_on(&rig);
        install(&rig, INDEXER);
        let pi = |role: &str| {
            format!(
                "---\nrole: {role}\nharness: pi\nmodel: qwen3.8:27b\neffort: low\n\
                 url: http://box:11434/v1\ncontext: 65536\n---\nReview the change against \
                 {{{{BASE}}}}. Output one finding per line as SEVERITY|file:line|what|why, or CLEAN.\n"
            )
        };
        rig.write_agent("pi-box", &pi("implementer"));
        rig.implementers(&["pi-box"]);
        rig.write_agent("pi-reviewer", &pi("reviewer"));
        rig.reviewers(&["pi-reviewer"]);
        let runner = rig.open().unwrap();
        rig.ask(&runner, "add", Some("7"));
        rig.forge.open_pull_request(71, "kelpie/7", &[7]);
        rig.claude
            .script([Scripted::Push("work.txt", "work\n"), Scripted::Text(CLEAN)]);
        step(&runner).unwrap(); // the worker's first turn
        step(&runner).unwrap(); // round 1, the pi reviewer

        let index = rig.paths().folder.join("code-graphs/7");
        let seen = reads_code(&rig);
        let roles: Vec<Role> = seen.iter().map(|s| s.call.role).collect();
        assert!(roles.contains(&Role::Worker) && roles.contains(&Role::Reviewer));
        for seen in &seen {
            assert!(matches!(seen.call.harness, AgentHarness::Pi(_)));
            let graph = seen.call.reach.code_graph.as_ref().expect("the server");
            assert_eq!(graph.index, index);
            assert_eq!(graph.binary, rig.paths().tools.code_graph());
        }
        let root = std::fs::read_to_string(index.join("root")).unwrap();
        assert_eq!(PathBuf::from(root.trim()), seen[0].call.cwd);
    }

    #[test]
    fn a_goose_worker_and_reviewer_get_the_server_and_an_index() {
        let rig = Rig::new("shep");
        turn_on(&rig);
        install(&rig, INDEXER);
        // A goose reviewer cannot be named in an agent file yet (#789), so the worker is run
        // and the reviewer's call is asked about directly.
        let goose = "---\nrole: implementer\nharness: goose\nmodel: qwen3.8:27b\neffort: low\n\
                     url: http://box:11434/v1\ncontext: 65536\n---\n";
        rig.write_agent("goose-box", goose);
        rig.implementers(&["goose-box"]);
        let runner = rig.open().unwrap();
        rig.ask(&runner, "add", Some("7"));
        rig.forge.open_pull_request(71, "kelpie/7", &[7]);
        rig.claude.script([Scripted::Push("work.txt", "work\n")]);
        step(&runner).unwrap(); // the worker's first turn

        let index = rig.paths().folder.join("code-graphs/7");
        let seen: Vec<_> = (rig.claude.all_seen().into_iter())
            .filter(|s| s.call.role == Role::Worker)
            .collect();
        assert!(!seen.is_empty());
        for seen in &seen {
            assert!(matches!(seen.call.harness, AgentHarness::Goose(_)));
            let graph = seen.call.reach.code_graph.as_ref().expect("the server");
            assert_eq!(graph.index, index);
            assert_eq!(graph.binary, rig.paths().tools.code_graph());
        }
        let root = std::fs::read_to_string(index.join("root")).unwrap();
        assert_eq!(PathBuf::from(root.trim()), seen[0].call.cwd);

        let server = crate::settings::ModelServer {
            host: crate::settings::ModelHost::Url(
                "http://box:11434/v1".to_owned().try_into().unwrap(),
            ),
            context: 65_536_i64.try_into().unwrap(),
        };
        let call = |role, tools| {
            AgentCall::new(
                role,
                tools,
                AgentHarness::Goose(server.clone()),
                "qwen3.8:27b",
                crate::settings::Effort::Low,
                crate::ports::Session::New(crate::work_item::new_session_id().unwrap()),
                "/wt".into(),
                "go",
            )
        };
        assert!(super::reads_code(&call(Role::Reviewer, Tools::Review)));
        assert!(!super::reads_code(&call(Role::Worker, Tools::Retro)));
        assert!(!super::reads_code(&call(Role::Researcher, Tools::Research)));
    }

    #[test]
    fn a_codex_worker_and_reviewer_get_the_server_and_an_index() {
        let rig = Rig::new("shep");
        turn_on(&rig);
        install(&rig, INDEXER);
        let codex = |role: &str| {
            format!(
                "---\nrole: {role}\nharness: codex\nmodel: gpt-6-sol\neffort: low\n---\n\
                 Review the change against {{{{BASE}}}}. Output one finding per line as \
                 SEVERITY|file:line|what|why, or CLEAN.\n"
            )
        };
        rig.write_agent("codex-box", &codex("implementer"));
        rig.implementers(&["codex-box"]);
        rig.write_agent("codex-reviewer", &codex("reviewer"));
        rig.reviewers(&["codex-reviewer"]);
        let runner = rig.open().unwrap();
        rig.ask(&runner, "add", Some("7"));
        rig.forge.open_pull_request(71, "kelpie/7", &[7]);
        rig.claude
            .script([Scripted::Push("work.txt", "work\n"), Scripted::Text(CLEAN)]);
        step(&runner).unwrap(); // the worker's first turn
        step(&runner).unwrap(); // round 1, the Codex reviewer

        let index = rig.paths().folder.join("code-graphs/7");
        let seen = reads_code(&rig);
        let roles: Vec<Role> = seen.iter().map(|s| s.call.role).collect();
        assert!(roles.contains(&Role::Worker) && roles.contains(&Role::Reviewer));
        for seen in &seen {
            assert_eq!(seen.call.harness, AgentHarness::Codex);
            let graph = seen.call.reach.code_graph.as_ref().expect("the server");
            assert_eq!(graph.index, index);
            assert_eq!(graph.binary, rig.paths().tools.code_graph());
        }
        let root = std::fs::read_to_string(index.join("root")).unwrap();
        assert_eq!(PathBuf::from(root.trim()), seen[0].call.cwd);
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
    fn a_server_that_is_not_installed_is_said_once_across_calls_and_status_shows_it() {
        let rig = Rig::new("shep");
        turn_on(&rig);
        let runner = work_and_review(&rig);

        assert!(reads_code(&rig).len() >= 2, "several calls met it");
        let notes = runner.lock().unwrap().take_notes();
        let said: Vec<_> = (notes.iter())
            .filter(|n| n.contains("runs without a code graph"))
            .collect();
        assert_eq!(said.len(), 1, "{notes:#?}");
        let status = rig.ask(&runner, "status", None);
        assert_eq!(status["standing"], json!([said[0]]));
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

        rig.ask(&runner, "rule", Some("71 yes"));
        step(&runner).unwrap(); // marked ready
        rig.clock.advance(CHECKS_SETTLE);
        let finished = step(&runner).unwrap();
        assert!(matches!(
            finished,
            Some(crate::runner::StepReport::Finished { issue: 7, .. })
        ));
        assert!(!index.exists());
    }

    #[test]
    fn a_call_ended_while_its_index_builds_kills_the_indexer_and_runs_without_the_server() {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("codebase-memory-mcp");
        write_script(&binary, "#!/bin/sh\nsleep 600\n");
        let graph = CodeGraph {
            binary,
            index: dir.path().join("index"),
        };
        let ending = Ending::default();
        ending.end();
        let started = Instant::now();
        let err = index(&graph, dir.path(), &ending).unwrap_err();
        assert!(err.contains("ended while indexing"), "{err}");
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "it waited the sleep out"
        );
    }
}
