//! A code graph of a work item's worktree for the calls that read the code
//!
//! A project that turns it on in `code_graph` has each reviewer indexed before
//! the call starts, and each worker turn too when the table names `worker`. The
//! index is kept under kelpie's home per work item, and goes with the work
//! item's worktree. A triage view's index goes when its call ends.
//!
//! The call is prepared with the runner's lock held, and indexing takes
//! seconds to minutes, so it runs on the call's own thread, where a stop
//! ends it.

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use super::Runner;
use super::claim::Notes;
use crate::metrics::{Feature, Recorder};
use crate::ports::{AgentCall, CodeGraph, Ending, Role, Tools};
use crate::settings::AgentHarness;
use crate::ticket::TicketId;

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
        let code_graph = &self.settings.code_graph;
        if !code_graph.enabled || !reads_code(call, code_graph.worker) {
            return;
        }
        let tools = &self.paths.tools;
        if !tools.has_code_graph() {
            self.metrics.feature_ran(Feature::CodeGraph, "skipped");
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
        // Only a call that works an issue reads code, and none numbers one zero.
        let Some(ticket) = TicketId::from_issue(call.issue) else {
            return;
        };
        // A work item of the same ticket would share the index of a triage view.
        let (index, runtime) = match call.role {
            Role::Triager => (
                self.paths.triage_graph(ticket),
                self.paths.triage_graph_runtime(ticket),
            ),
            _ => (
                self.paths.code_graph(ticket),
                self.paths.code_graph_runtime(ticket),
            ),
        };
        call.reach.code_graph = Some(CodeGraph {
            binary: tools.code_graph(),
            index,
            runtime,
        });
    }

    /// Removes the index of the worktree for `ticket`, and its daemon's folder
    ///
    /// # Errors
    ///
    /// The folder that could not be removed, and why.
    pub(super) fn remove_code_graph(&self, ticket: TicketId) -> Result<(), String> {
        remove(&[
            self.paths.code_graph(ticket),
            self.paths.code_graph_runtime(ticket),
        ])
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

// The reviewers and the triager read the code. The worker's turns do only when the project names
// `code_graph.worker`: no worker session recorded so far, on Claude Code or pi, has answered one of
// the server's tools, and its 17 definitions ride in every request of every turn
// (`docs/facts/mcp-servers.md`). The retro, the advisors and the project manager do not read the
// code at all, and a harness that loads no MCP servers cannot use the server.
fn reads_code(call: &AgentCall, worker: bool) -> bool {
    call.harness.loads_mcp()
        && match (call.role, call.tools) {
            (Role::Worker, Tools::Work) => worker,
            (Role::Reviewer | Role::Triager, Tools::Review) => true,
            _ => false,
        }
}

pub(super) fn who(call: &AgentCall) -> String {
    format!("issue #{}'s {}", call.issue, name(call.role))
}

// The note's owner for a worker's turn a previous image began, which carries its issue and
// nothing else of its call
pub(super) fn who_carried(issue: u64) -> String {
    format!("issue #{issue}'s {}", name(Role::Worker))
}

/// Indexes `call`'s worktree for the server its reach names, on the call's
/// own thread
///
/// A call whose index cannot be built, or is ended while it builds, loses the
/// server and runs without it, and a note says why.
pub(super) fn indexed(call: &mut AgentCall, notes: &Notes, metrics: &Recorder, ending: &Ending) {
    let Some(graph) = &call.reach.code_graph else {
        return;
    };
    match index(graph, &call.cwd, ending) {
        Ok(()) => {
            metrics.feature_ran(Feature::CodeGraph, "ok");
            notes.ends(INDEX_FAILED);
        }
        Err(e) => {
            let line = format!("{} runs without a code graph: {e}", who(call));
            // A call ended while it indexes says nothing of the graph itself.
            if ending.asked() {
                metrics.feature_ran(Feature::CodeGraph, "ended");
                notes.push(line);
            } else {
                metrics.feature_ran(Feature::CodeGraph, "failed");
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
        Role::Representative => "representative",
        Role::Triager => "triager",
    }
}

/// Removes the folders of a graph, which may be gone already
///
/// # Errors
///
/// The folder that could not be removed, and why.
pub(super) fn remove(folders: &[std::path::PathBuf]) -> Result<(), String> {
    for folder in folders {
        match std::fs::remove_dir_all(folder) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                return Err(format!("cannot remove {}: {e}", folder.display()));
            }
            _ => {}
        }
    }
    Ok(())
}

// Indexes `worktree` into the graph's folder, which the server reads back.
// Run again for each call, since the server only watches what it is running over.
// A call asked to end kills the indexer.
fn index(graph: &CodeGraph, worktree: &Path, ending: &Ending) -> Result<(), String> {
    for folder in [&graph.index, &graph.runtime] {
        std::fs::create_dir_all(folder)
            .map_err(|e| format!("cannot make {}: {e}", folder.display()))?;
    }
    // The daemon serves its graph over HTTP on a local port unless its index folder says not to.
    let ui_off = ["config", "set", "ui_enabled", "false"].map(OsStr::new);
    run(graph, worktree, "turning its web UI off", &ui_off, ending)?;
    let index = [
        OsStr::new("cli"),
        "index_repository".as_ref(),
        "--repo-path".as_ref(),
    ];
    run(
        graph,
        worktree,
        "indexing",
        &[&index[..], &[worktree.as_os_str()]].concat(),
        ending,
    )
    .map_err(|e| match too_long(&graph.runtime) {
        Some(why) => format!("{e}; {why}"),
        None => e,
    })
}

// Why the daemon's socket, `cbm-daemon-<uid>/cbm-<16 hex>.sock` under `runtime`, cannot bind
// there, when its path is too long: the daemon says only that its endpoint could not be made.
// The folder is this user's, so its owner is the uid.
fn too_long(runtime: &Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let uid = std::fs::metadata(runtime).ok()?.uid();
    let socket = runtime.join(format!("cbm-daemon-{uid}/cbm-{}.sock", "0".repeat(16)));
    crate::home::socket_fits(&socket).err()
}

// Runs the server's `args` over the graph's folders, saying `what` it was doing when it fails.
fn run(
    graph: &CodeGraph,
    worktree: &Path,
    what: &str,
    args: &[&OsStr],
    ending: &Ending,
) -> Result<(), String> {
    let log = graph.index.join("kelpie-index.log");
    let output =
        std::fs::File::create(&log).map_err(|e| format!("cannot write {}: {e}", log.display()))?;
    let more = output
        .try_clone()
        .map_err(|e| format!("cannot write {}: {e}", log.display()))?;
    let mut child = crate::spawn::command(&graph.binary)
        .args(args)
        .env("CBM_CACHE_DIR", &graph.index)
        .env("CBM_RUNTIME_DIR", &graph.runtime)
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
                    true => format!("the call was ended while {what}"),
                    false => format!("{what} took over {}s", INDEX_LIMIT.as_secs()),
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
    Err(format!("{what} failed with {status}: {tail}"))
}

#[cfg(test)]
mod tests {
    use crate::test::ticket;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;
    use crate::ports::CLEAN;
    use crate::runner::{CHECKS_SETTLE, step};
    use crate::settings::AgentHarness;
    use crate::test::{Rig, Scripted, write_script};

    // A server that indexes by writing a file into its cache folder, and keeps each run's
    // arguments and the folder its daemon would bind in.
    const INDEXER: &str = "#!/bin/sh\n\
        echo \"$@\" >> \"$CBM_CACHE_DIR/ran\"\n\
        echo \"$@\" > \"$CBM_CACHE_DIR/indexed-with\"\n\
        echo \"$CBM_RUNTIME_DIR\" > \"$CBM_CACHE_DIR/runtime\"\n\
        echo \"$CBM_ALLOWED_ROOT\" > \"$CBM_CACHE_DIR/root\"\n";

    // The example's code graph table, as `settings.example.toml` writes it
    const GRAPH_OFF: &str = "[app.dogs.kelpie.code_graph]\nenabled = false\nworker = false";

    // The graph on for the worker's turns and the reviewers alike
    fn turn_on(rig: &Rig) {
        rig.edit_settings(|s| {
            assert!(s.contains(GRAPH_OFF), "the example's code graph moved");
            s.replace(
                GRAPH_OFF,
                "[app.dogs.kelpie.code_graph]\nenabled = true\nworker = true",
            )
        });
    }

    // The graph on with no `worker` key, which is what `enabled` alone gives today
    fn turn_on_for_reviewers(rig: &Rig) {
        rig.edit_settings(|s| {
            assert!(s.contains(GRAPH_OFF), "the example's code graph moved");
            s.replace(GRAPH_OFF, "[app.dogs.kelpie.code_graph]\nenabled = true")
        });
    }

    fn install(rig: &Rig, script: &str) {
        let binary = rig.paths().tools.code_graph();
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        write_script(&binary, script);
    }

    // The rig's pi worker and reviewer, as their agent files and the project's lists
    fn pi_agents(rig: &Rig) {
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
    }

    // The rig's goose worker. A goose reviewer cannot be named in an agent file yet (#789).
    fn goose_agent(rig: &Rig) {
        let goose = "---\nrole: implementer\nharness: goose\nmodel: qwen3.8:27b\neffort: low\n\
                     url: http://box:11434/v1\ncontext: 65536\n---\n";
        rig.write_agent("goose-box", goose);
        rig.implementers(&["goose-box"]);
    }

    // The rig's Codex worker and reviewer, as their agent files and the project's lists
    fn codex_agents(rig: &Rig) {
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

    fn worker_turns(rig: &Rig) -> Vec<crate::test::Seen> {
        (rig.claude.all_seen().into_iter())
            .filter(|s| s.call.role == Role::Worker)
            .collect()
    }

    // Issue 7's worker's first turn on its own, with the graph's server installed
    fn worker_turn(rig: &Rig) -> Mutex<Runner> {
        let runner = rig.open().unwrap();
        rig.ask(&runner, "add", Some("7"));
        rig.forge.open_pull_request(71, "kelpie/7", &[7]);
        rig.claude.script([Scripted::Push("work.txt", "work\n")]);
        step(&runner).unwrap(); // the worker's first turn
        runner
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
        let runtime = rig.paths().folder.join("cbm/7");
        let seen = reads_code(&rig);
        let roles: Vec<Role> = seen.iter().map(|s| s.call.role).collect();
        assert!(roles.contains(&Role::Worker) && roles.contains(&Role::Reviewer));
        for seen in &seen {
            let graph = seen.call.reach.code_graph.as_ref().expect("the server");
            assert_eq!(graph.index, index);
            assert_eq!(graph.runtime, runtime);
            assert_eq!(graph.binary, rig.paths().tools.code_graph());
            let config: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(mcp_file(seen)).unwrap()).unwrap();
            let server = &config["mcpServers"]["codebase-memory-mcp"];
            assert_eq!(server["env"]["CBM_CACHE_DIR"], index.to_str().unwrap());
            assert_eq!(server["env"]["CBM_RUNTIME_DIR"], runtime.to_str().unwrap());
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
        let ran = std::fs::read_to_string(index.join("ran")).unwrap();
        let first: Vec<&str> = ran.lines().take(2).collect();
        assert_eq!(
            first[0], "config set ui_enabled false",
            "the web UI off first"
        );
        assert!(first[1].starts_with("cli index_repository"), "{ran}");
        let told = std::fs::read_to_string(index.join("runtime")).unwrap();
        assert_eq!(PathBuf::from(told.trim()), runtime);
        assert!(
            runtime.is_dir(),
            "made for the daemon, which does not make it"
        );
        let root = std::fs::read_to_string(index.join("root")).unwrap();
        assert_eq!(
            PathBuf::from(root.trim()),
            seen[0].call.cwd,
            "the server is held to the worktree"
        );
    }

    #[test]
    fn a_worker_gets_no_graph_unless_the_project_names_it() {
        // `enabled = true` with no `worker` key: the worker's turn carries no server, writes no
        // MCP file of its own, allows no `mcp__` tool, and indexes nothing.
        let rig = Rig::new("shep");
        turn_on_for_reviewers(&rig);
        install(&rig, INDEXER);
        worker_turn(&rig);

        let seen = worker_turns(&rig);
        assert!(!seen.is_empty(), "the worker's turn ran");
        for seen in &seen {
            assert_eq!(seen.call.reach.code_graph, None);
            assert!(!mcp_file(seen).exists());
            assert!(
                !seen.settings.to_string().contains("mcp__"),
                "{:?}",
                seen.settings
            );
        }
        assert!(
            !rig.paths().folder.join("code-graphs").exists(),
            "nothing is indexed before a reviewer's round"
        );

        // With `worker = true`, the same turn carries the server and builds the index it does today.
        let rig = Rig::new("shep");
        turn_on(&rig);
        install(&rig, INDEXER);
        worker_turn(&rig);

        let index = rig.paths().folder.join("code-graphs/7");
        let seen = worker_turns(&rig);
        assert!(!seen.is_empty(), "the worker's turn ran");
        for seen in &seen {
            let graph = seen.call.reach.code_graph.as_ref().expect("the server");
            assert_eq!(graph.index, index);
            assert_eq!(graph.binary, rig.paths().tools.code_graph());
            let config: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(mcp_file(seen)).unwrap()).unwrap();
            assert_eq!(
                config["mcpServers"]["codebase-memory-mcp"]["env"]["CBM_CACHE_DIR"],
                index.to_str().unwrap()
            );
            assert!(
                seen.settings["permissions"]["allow"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|a| *a == "mcp__codebase-memory-mcp"),
                "{:?}",
                seen.settings
            );
        }
        let indexed = std::fs::read_to_string(index.join("indexed-with")).unwrap();
        assert!(
            indexed.starts_with("cli index_repository --repo-path "),
            "{indexed}"
        );
    }

    #[test]
    fn a_worker_without_the_graph_still_gets_the_repo_s_own_servers() {
        // The graph is its own path through the call's reach, so a worker without it keeps what
        // `repo_setup` gives it.
        let rig = Rig::new("shep");
        turn_on_for_reviewers(&rig);
        install(&rig, INDEXER);
        let own = json!({"mcpServers": {"docs": {"type": "http", "url": "https://docs.example.com/mcp"}}});
        rig.land(".mcp.json", &own.to_string());
        worker_turn(&rig);

        let seen = worker_turns(&rig);
        assert_eq!(seen[0].call.reach.code_graph, None);
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(mcp_file(&seen[0])).unwrap()).unwrap();
        let given: Vec<String> = (config["mcpServers"].as_object().unwrap())
            .keys()
            .cloned()
            .collect();
        assert_eq!(given, vec!["docs".to_owned()]);
        assert!(
            seen[0].settings["permissions"]["allow"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| *a == "mcp__docs"),
            "{:?}",
            seen[0].settings
        );
    }

    #[test]
    fn a_reviewer_still_gets_the_graph_when_the_worker_does_not() {
        let rig = Rig::new("shep");
        turn_on_for_reviewers(&rig);
        install(&rig, INDEXER);
        work_and_review(&rig);

        let index = rig.paths().folder.join("code-graphs/7");
        let seen = reads_code(&rig);
        let roles: Vec<Role> = seen.iter().map(|s| s.call.role).collect();
        assert!(
            roles.contains(&Role::Worker) && roles.contains(&Role::Reviewer),
            "both calls ran: {roles:?}"
        );
        for seen in &seen {
            if seen.call.role == Role::Worker {
                assert_eq!(
                    seen.call.reach.code_graph, None,
                    "the worker's turn is not given it"
                );
                continue;
            }
            let graph = seen.call.reach.code_graph.as_ref().expect("the server");
            assert_eq!(graph.index, index);
            assert_eq!(graph.binary, rig.paths().tools.code_graph());
        }
        let indexed = std::fs::read_to_string(index.join("indexed-with")).unwrap();
        assert!(
            indexed.starts_with("cli index_repository --repo-path "),
            "the reviewer's round built the index: {indexed}"
        );
    }

    #[test]
    fn a_pi_worker_and_reviewer_get_the_server_and_an_index() {
        let rig = Rig::new("shep");
        turn_on(&rig);
        install(&rig, INDEXER);
        pi_agents(&rig);
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
    fn a_pi_worker_gets_no_graph_when_the_project_does_not_name_it() {
        let rig = Rig::new("shep");
        turn_on_for_reviewers(&rig);
        install(&rig, INDEXER);
        pi_agents(&rig);
        worker_turn(&rig);

        assert!(
            worker_turns(&rig)
                .iter()
                .all(|s| s.call.reach.code_graph.is_none()),
            "the pi worker carries no server"
        );
    }

    #[test]
    fn a_goose_worker_gets_no_graph_when_the_project_does_not_name_it() {
        let rig = Rig::new("shep");
        turn_on_for_reviewers(&rig);
        install(&rig, INDEXER);
        goose_agent(&rig);
        worker_turn(&rig);

        assert!(
            worker_turns(&rig)
                .iter()
                .all(|s| s.call.reach.code_graph.is_none()),
            "the goose worker carries no server"
        );
    }

    #[test]
    fn a_codex_worker_gets_no_graph_when_the_project_does_not_name_it() {
        let rig = Rig::new("shep");
        turn_on_for_reviewers(&rig);
        install(&rig, INDEXER);
        codex_agents(&rig);
        worker_turn(&rig);

        assert!(
            worker_turns(&rig)
                .iter()
                .all(|s| s.call.reach.code_graph.is_none()),
            "the Codex worker carries no server"
        );
    }

    #[test]
    fn a_goose_worker_and_reviewer_get_the_server_and_an_index() {
        let rig = Rig::new("shep");
        turn_on(&rig);
        install(&rig, INDEXER);
        // A goose reviewer cannot be named in an agent file yet (#789), so the worker is run
        // and the reviewer's call is asked about directly.
        goose_agent(&rig);
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
        assert!(super::reads_code(
            &call(Role::Reviewer, Tools::Review),
            false
        ));
        assert!(super::reads_code(
            &call(Role::Triager, Tools::Review),
            false
        ));
        assert!(super::reads_code(&call(Role::Worker, Tools::Work), true));
        assert!(!super::reads_code(&call(Role::Worker, Tools::Work), false));
        assert!(!super::reads_code(&call(Role::Worker, Tools::Retro), true));
        assert!(!super::reads_code(
            &call(Role::Researcher, Tools::Research),
            true
        ));
    }

    #[test]
    fn a_codex_worker_and_reviewer_get_the_server_and_an_index() {
        let rig = Rig::new("shep");
        turn_on(&rig);
        install(&rig, INDEXER);
        codex_agents(&rig);
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
        let runtime = rig.paths().folder.join("cbm/7");
        assert!(
            index.join("indexed-with").is_file(),
            "the worker's turn indexed"
        );
        assert!(runtime.is_dir());

        rig.ask(&runner, "rule", Some("71 yes"));
        step(&runner).unwrap(); // marked ready
        rig.clock.advance(CHECKS_SETTLE);
        let finished = step(&runner).unwrap();
        assert!(matches!(
           finished,
           Some(crate::runner::StepReport::Finished { issue, .. }) if issue == ticket(7)
        ));
        assert!(!index.exists());
        assert!(!runtime.exists(), "the daemon's folder goes with it");
    }

    #[test]
    fn a_call_ended_while_its_index_builds_kills_the_indexer_and_runs_without_the_server() {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("codebase-memory-mcp");
        write_script(&binary, "#!/bin/sh\ncase \"$1\" in cli) sleep 600;; esac\n");
        let graph = CodeGraph {
            binary,
            index: dir.path().join("index"),
            runtime: dir.path().join("cbm"),
        };
        let ending = Ending::default();
        ending.end();
        let started = Instant::now();
        let err = index(&graph, dir.path(), &ending).unwrap_err();
        assert!(err.contains("the call was ended while"), "{err}");
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "it waited the sleep out"
        );
    }

    #[test]
    fn an_index_that_fails_under_a_daemon_folder_too_long_for_its_socket_says_so() {
        let dir = crate::test::short_tempdir();
        let binary = dir.path().join("codebase-memory-mcp");
        let failing = "#!/bin/sh\ncase \"$1\" in cli) echo 'endpoint could not be created' >&2; exit 1;; esac\n";
        write_script(&binary, failing);
        let graph = |runtime: &str| CodeGraph {
            binary: binary.clone(),
            index: dir.path().join("index"),
            runtime: dir.path().join(runtime),
        };
        let long = "x".repeat(80);
        let err = index(&graph(&long), dir.path(), &Ending::default()).unwrap_err();
        assert!(err.contains("endpoint could not be created"), "{err}");
        assert!(err.contains(&long) && err.contains("socket"), "{err}");

        let err = index(&graph("cbm"), dir.path(), &Ending::default()).unwrap_err();
        assert!(
            !err.contains("socket"),
            "a path that fits is not blamed: {err}"
        );
    }
}
