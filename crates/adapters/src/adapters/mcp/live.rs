//! The code graph server under the real runtime, in a sandbox that denies
//! reading the folder its index and daemon folder sit in, as every call's
//! sandbox denies the shepherd's home
//!
//! A test needs macOS and `shep kelpie tools install` with the code graph.
//! Without them it says why on stderr and passes: the server binds a Unix
//! socket, which srt refuses on Linux (#488).

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};

use super::*;
use crate::adapters::SandboxRuntime;
use crate::ports::mcp::entry;
use crate::ports::{CodeGraph, Reach, Role, Sandbox, Session, SessionId, Tools as Kind};
use crate::settings::{AgentHarness, Effort};
use crate::tools::Tools;

const SYMBOL: &str = "kelpie_probe_symbol";

// The installed code graph, with a built kelpie to run its calls, or why there are none.
fn installed() -> Result<Tools, String> {
    if !cfg!(target_os = "macos") {
        return Err("srt refuses a Unix socket off macOS".into());
    }
    let home = crate::home::kelpie_home().map_err(|e| e.to_string())?;
    let tools = Tools::under(&home);
    // A test binary has no `crabpot::run_from_args`, so a call runs through the built kelpie.
    crate::adapters::srt::built_kelpie()?;
    match tools.has_code_graph() {
        true => Ok(tools),
        false => Err(format!(
            "run `shep kelpie tools install` with the code graph for {}",
            home.display()
        )),
    }
}

// Runs the server's `args` outside any sandbox, as the runner indexes before a call.
fn outside(graph: &CodeGraph, worktree: &Path, args: &[&std::ffi::OsStr]) -> String {
    let out = Command::new(&graph.binary)
        .args(args)
        .env("CBM_CACHE_DIR", &graph.index)
        .env("CBM_RUNTIME_DIR", &graph.runtime)
        .env("CBM_ALLOWED_ROOT", worktree)
        .current_dir(worktree)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "{args:?}: {out:?}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn the_server_answers_a_code_search_in_a_sandbox_that_denies_the_folder_above_its_index() {
    let tools = match installed() {
        Ok(tools) => tools,
        Err(why) => return eprintln!("skipped: {why}"),
    };
    let dir = crate::test::short_tempdir();
    let root = dir.path().canonicalize().unwrap();
    // The shepherd's home in small, with the worktree and the graph's folders in it.
    let home = root.join("h");
    let worktree = home.join("w");
    let graph = CodeGraph {
        binary: tools.code_graph(),
        index: home.join("g"),
        runtime: home.join("c"),
    };
    for folder in [&graph.index, &graph.runtime, &worktree.join("src")] {
        std::fs::create_dir_all(folder).unwrap();
    }
    // The daemon's socket, as `runner::code_graph` checks it, is 41 bytes longer than the folder.
    let uid = std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(&root).unwrap());
    let socket = (graph.runtime).join(format!("cbm-daemon-{uid}/cbm-{}.sock", "0".repeat(16)));
    if let Err(why) = crate::home::socket_fits(&socket) {
        return eprintln!("skipped: set TMPDIR to a shorter folder: {why}");
    }
    std::fs::write(
        worktree.join("src/lib.rs"),
        format!("pub fn {SYMBOL}() {{}}\n"),
    )
    .unwrap();
    outside(
        &graph,
        &worktree,
        &["config", "set", "ui_enabled", "false"].map(AsRef::as_ref),
    );
    let indexed = outside(
        &graph,
        &worktree,
        &[
            "cli".as_ref(),
            "index_repository".as_ref(),
            "--repo-path".as_ref(),
            worktree.as_os_str(),
        ],
    );
    let indexed: Value = serde_json::from_str(indexed.lines().last().unwrap()).unwrap();
    let project = indexed["project"].as_str().unwrap().to_owned();

    let call = AgentCall {
        reach: Reach {
            code_graph: Some(graph.clone()),
            ..Reach::default()
        },
        ..AgentCall::new(
            Role::Worker,
            Kind::Work,
            AgentHarness::ClaudeCode,
            "claude-sonnet-5",
            Effort::Medium,
            Session::New(SessionId("s".into())),
            worktree.clone(),
            "go",
        )
    };
    let mut policy = Policy {
        no_read: vec![format!("{}/**", home.display())],
        read: vec![worktree.clone()],
        write: vec![worktree.clone()],
        ..Policy::default()
    };
    fence_the_graph(&mut policy, &call);
    let mut server = Command::new(&graph.binary);
    let env = entry(&graph, &worktree)["env"].clone();
    for (name, value) in env.as_object().unwrap() {
        server.env(name, value.as_str().unwrap());
    }
    server.current_dir(&worktree);
    let mut child = SandboxRuntime::new(tools)
        .run_by(crate::adapters::srt::built_kelpie().unwrap())
        .wrap(&policy, &root.join("srt.json"), &server)
        .unwrap()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let requests = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "kelpie-test", "version": "1"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
            "name": "search_code", "arguments": {"project": project, "pattern": SYMBOL}}}),
    ];
    let mut stdin = child.stdin.take().unwrap();
    for request in requests {
        writeln!(stdin, "{request}").unwrap();
    }
    let (lines, replies) = mpsc::channel();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    std::thread::spawn(move || {
        stdout
            .lines()
            .map_while(Result::ok)
            .try_for_each(|l| lines.send(l))
    });
    let search = std::iter::from_fn(|| replies.recv_timeout(Duration::from_secs(60)).ok())
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .find(|reply| reply["id"] == 2);
    drop(stdin);
    let _ = child.kill();
    let out = child.wait_with_output().unwrap();
    let why = String::from_utf8_lossy(&out.stderr);
    let search = search.unwrap_or_else(|| panic!("no answer to the search: {why}"));
    assert_ne!(search["result"]["isError"], true, "{search}\n{why}");
    assert!(search.to_string().contains(SYMBOL), "{search}\n{why}");

    // An opened folder shows its names and nothing in them, and `/tmp` takes the server's
    // scratch names alone, not another daemon's folder.
    std::fs::write(home.join("secret"), SYMBOL).unwrap();
    let daemon = format!(
        "/private/tmp/cbm-daemon-kelpie-probe-{}",
        std::process::id()
    );
    let mut shell = Command::new("/bin/sh");
    shell
        .args([
            "-c",
            &format!("ls {h}; cat {h}/secret; mkdir {daemon}", h = home.display()),
        ])
        .current_dir(&worktree);
    let probe = SandboxRuntime::new(installed().unwrap())
        .run_by(crate::adapters::srt::built_kelpie().unwrap())
        .wrap(&policy, &root.join("srt.json"), &shell)
        .unwrap()
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let made = std::fs::remove_dir(&daemon).is_ok();
    let listed = String::from_utf8_lossy(&probe.stdout);
    assert!(listed.contains("secret"), "{probe:?}");
    assert_eq!(listed.matches(SYMBOL).count(), 0, "{probe:?}");
    assert!(!made, "{probe:?}");
}
