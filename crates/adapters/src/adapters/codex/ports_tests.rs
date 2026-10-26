//! The ports a Codex call opens: a browser's socket, and the code graph's index

use std::path::PathBuf;

use super::tests::{WORKER_ID, World, id, strings};
use super::*;
use crate::ports::{Browser, CodeGraph};
use crate::test::NO_PROGRAM;

#[test]
fn a_call_with_a_browser_loads_playwright_and_the_sandbox_connects_only_to_its_socket() {
    let w = World::new();
    let browser = Browser {
        kelpie: PathBuf::from("/k/kelpie"),
        socket: w.path("0123456789abcdef0123456789abcdef.sock"),
    };
    let mut call = w.fenced(Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));
    let login = w.path("login");
    let without = policy(&call, &login, &Files::of(&call), &NO_PROGRAM, &[]);
    let argv_without = strings(&call, None, None);
    call.reach.browser = Some(Box::new(browser.clone()));
    let with = policy(&call, &login, &Files::of(&call), &NO_PROGRAM, &[]);
    let argv_with = strings(&call, None, None);

    assert!(!without.sockets.contains(&browser.socket));
    assert_eq!(
        with.sockets
            .iter()
            .filter(|s| !without.sockets.contains(s))
            .collect::<Vec<_>>(),
        [&browser.socket]
    );
    assert!(
        !argv_without
            .iter()
            .any(|a| a.starts_with("mcp_servers.playwright")),
        "{argv_without:?}"
    );
    let at = argv_with
        .iter()
        .position(|a| a.starts_with("mcp_servers.playwright="))
        .expect("the override");
    assert_eq!(argv_with[at - 1], "-c");
    // Read back the way Codex reads `-c`: the server must carry the socket the sandbox allows.
    let line: toml::Table = toml::from_str(&argv_with[at]).unwrap();
    let server = &line["mcp_servers"]["playwright"];
    assert_eq!(server["command"].as_str(), Some("/k/kelpie"));
    let args: Vec<_> = server["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    assert_eq!(args, ["mcp-connect", browser.socket.to_str().unwrap()]);
}

#[test]
fn the_sandbox_reads_the_code_graph_binary_and_writes_only_its_index_and_daemon_folder() {
    let w = World::new();
    let graph = CodeGraph {
        binary: PathBuf::from("/k/tools/codebase-memory-mcp"),
        index: PathBuf::from("/k/graphs/7"),
        runtime: PathBuf::from("/k/cbm/7"),
    };
    let mut call = w.fenced(Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));
    let login = w.path("login");
    let without = policy(&call, &login, &Files::of(&call), &NO_PROGRAM, &[]);
    call.reach.code_graph = Some(graph.clone());
    let with = policy(&call, &login, &Files::of(&call), &NO_PROGRAM, &[]);

    assert!(with.read.contains(&graph.binary) && with.read.contains(&graph.index));
    assert!(with.write.contains(&graph.index));
    assert!(!with.write.contains(&graph.binary));
    assert!(!without.read.contains(&graph.binary) && !without.write.contains(&graph.index));
    let granted: Vec<_> = with
        .write
        .iter()
        .filter(|p| !without.write.contains(p))
        .collect();
    assert_eq!(granted, [&graph.index, &graph.runtime]);
    let sockets: Vec<_> = (with.sockets.iter())
        .filter(|s| !without.sockets.contains(s))
        .collect();
    assert_eq!(
        sockets,
        [&graph.runtime],
        "its daemon's socket, and no other"
    );
}
