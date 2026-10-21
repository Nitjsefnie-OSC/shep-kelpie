//! The MCP servers a goose call is given: the flags of a new session, the keys the hook lets
//! through, and what the sandbox leaves the code graph

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command as Process;

use serde_json::{Value, json};

use super::mcp::{derived_key, refusal, skipped_env};
use super::tests::{FRESH, FRESH_ID, OTHER_ID, URL, World, id, stand_in, strings};
use super::*;
use crate::guard::{Checkout, GOOSE_SERVER_FLAG, goose_server_flags, judge_goose};
use crate::hooks::Hooks;
use crate::local_paths::LocalPaths;
use crate::ports::{Browser, CodeGraph, Reach};

fn graph() -> CodeGraph {
    CodeGraph {
        binary: "/k/tools/codebase-memory-mcp".into(),
        index: "/k/graphs/7".into(),
        runtime: "/k/cbm/7".into(),
    }
}

fn servers(entries: Value) -> BTreeMap<String, Value> {
    entries
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, entry)| (name.clone(), entry.clone()))
        .collect()
}

// A worker behind the fence, given `mcp_servers` and, if asked, the code graph.
fn worker(w: &World, session: Session, mcp_servers: Value, graph: Option<CodeGraph>) -> AgentCall {
    let mut call = w.fenced(URL, Path::new("/k/kelpie"), session);
    call.reach.mcp_servers = servers(mcp_servers);
    call.reach.code_graph = graph;
    call
}

fn repo() -> Value {
    json!({
        "docs": { "type": "http", "url": "https://docs.example.com/mcp" },
        "plugin_style_search": { "command": "/k/search", "args": ["--stdio"] },
        // A repo server cannot take the code graph's place.
        "codebase-memory-mcp": { "command": "/repo/impostor" },
    })
}

// The flags of `argv` that add a server, each with its value.
fn server_flags(argv: &[String]) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for (at, arg) in argv.iter().enumerate() {
        if arg == "--with-extension" || arg == "--with-streamable-http-extension" {
            found.push((arg.clone(), argv[at + 1].clone()));
        }
    }
    found
}

// What goose makes of a `--with-extension` value: a copy of `split_extension_name_prefix`,
// `split_command_args` and `parse_stdio_extension` of goose 1.54.0, so a test reads the string back
// the way goose does. The name, the env pairs, the command and its arguments.
type Read = (Option<String>, Vec<(String, String)>, String, Vec<String>);

fn goose_reads(value: &str) -> Read {
    let named = value.split_once(':').filter(|(candidate, rest)| {
        candidate.chars().count() >= 2
            && candidate
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            && !rest.trim().is_empty()
            && !rest.starts_with("//")
    });
    let (name, command) = match named {
        Some((name, rest)) => (Some(name.to_lowercase()), rest),
        None => (None, value),
    };
    let mut parts = split_command_args(command);
    let mut envs = Vec::new();
    while parts.first().is_some_and(|part| part.contains('=')) {
        let (key, value) = parts
            .remove(0)
            .split_once('=')
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .unwrap();
        envs.push((key, value));
    }
    let cmd = parts.remove(0);
    (name, envs, cmd, parts)
}

fn split_command_args(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let (mut in_double, mut in_single) = (false, false);
    for c in input.chars() {
        match c {
            '"' if !in_single => in_double = !in_double,
            '\'' if !in_double && (in_single || current.is_empty()) => in_single = !in_single,
            c if c.is_whitespace() && !in_double && !in_single => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    assert!(!in_double && !in_single, "unmatched quote in {input}");
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

#[test]
fn a_new_session_gets_one_flag_per_server_in_order_with_the_graph_last_and_unreplaceable() {
    let w = World::new();
    let call = worker(&w, Session::New(id(FRESH_ID)), repo(), Some(graph()));
    let argv = strings(&call);

    let at = argv.iter().position(|a| a == "--with-builtin").unwrap();
    assert_eq!(argv[at + 1], "developer");
    // The flags follow it, and nothing else of the servers is in the call.
    assert_eq!(
        argv[at + 2..at + 8],
        [
            "--with-streamable-http-extension",
            "https://docs.example.com/mcp",
            "--with-extension",
            "plugin_style_search:/k/search --stdio",
            "--with-extension",
            &format!(
                "codebase-memory-mcp:CBM_ALLOWED_ROOT={} CBM_CACHE_DIR=/k/graphs/7 \
                 CBM_RUNTIME_DIR=/k/cbm/7 /k/tools/codebase-memory-mcp",
                w.path("wt").display()
            ),
        ]
    );
    assert_eq!(server_flags(&argv).len(), 3);
    assert!(!argv.join(" ").contains("impostor"));
    // The hook is handed the key of each, in the same order.
    assert_eq!(
        mcp::keys(&call),
        [
            "docs_example_com_mcp",
            "plugin_style_search",
            "codebase-memory-mcp"
        ]
    );
}

#[test]
fn stdio_entries_take_goose_s_command_string_with_the_server_s_name_and_env() {
    let w = World::new();
    let entry = json!({
        "command": "/k/my tool/bin",
        "args": ["--name", "it's a name", "'quoted'", "plain'mid", "a\tb"],
        // goose skips PATH and NODE_OPTIONS, in any case.
        "env": { "TOKEN": "two words", "EMPTY": "", "PATH": "/evil", "node_options": "--x", "K": "it's" },
    });
    let call = worker(
        &w,
        Session::New(id(FRESH_ID)),
        json!({ "Search-Tool": entry.clone() }),
        None,
    );
    let flags = server_flags(&strings(&call));
    assert_eq!(flags.len(), 1);
    let (flag, value) = &flags[0];
    assert_eq!(flag, "--with-extension");
    assert_eq!(
        value,
        "Search-Tool:EMPTY= K=it's \"TOKEN=two words\" \"/k/my tool/bin\" --name \
         \"it's a name\" \"'quoted'\" plain'mid \"a\tb\""
    );

    // Read back the way goose reads it, the name is the server's, in lower case, and each word
    // is the entry's own.
    let (name, envs, command, args) = goose_reads(value);
    assert_eq!(name.as_deref(), Some("search-tool"));
    let pair = |k: &str, v: &str| (k.to_owned(), v.to_owned());
    assert_eq!(
        envs,
        [
            pair("EMPTY", ""),
            pair("K", "it's"),
            pair("TOKEN", "two words")
        ]
    );
    assert_eq!(command, "/k/my tool/bin");
    assert_eq!(
        args,
        ["--name", "it's a name", "'quoted'", "plain'mid", "a\tb"]
    );
    assert_eq!(mcp::keys(&call), ["search-tool"]);

    // The skipped keys are the only thing about the entry the runner notes.
    let note = skipped_env("Search-Tool", &entry).unwrap();
    assert!(
        note.contains("`PATH`") && note.contains("`node_options`"),
        "{note}"
    );
    assert!(!note.contains("TOKEN"), "{note}");
    assert_eq!(skipped_env("plain", &json!({ "command": "/k/s" })), None);
}

#[test]
fn an_http_entry_without_headers_takes_the_streamable_http_flag_and_its_derived_key() {
    let w = World::new();
    let call = worker(
        &w,
        Session::New(id(FRESH_ID)),
        json!({
            "docs": { "type": "http", "url": "https://Docs.Example.com:8443/v1/mcp/" },
            "untyped": { "url": "http://localhost:80/mcp", "headers": {} },
            "streamable": { "type": "streamable-http", "url": "https://s.example.com" },
        }),
        None,
    );
    let flags = server_flags(&strings(&call));
    let flag = "--with-streamable-http-extension".to_owned();
    assert_eq!(
        flags,
        [
            (
                flag.clone(),
                "https://Docs.Example.com:8443/v1/mcp/".to_owned()
            ),
            (flag.clone(), "https://s.example.com".to_owned()),
            (flag, "http://localhost:80/mcp".to_owned()),
        ]
    );
    // goose names each from its URL, and the hook is handed that name, not Claude's.
    assert_eq!(
        mcp::keys(&call),
        [
            "docs_example_com_8443_v1_mcp",
            "s_example_com",
            "localhost_mcp"
        ]
    );
    assert_eq!(
        derived_key("http://127.0.0.1:3000/mcp").as_deref(),
        Some("127_0_0_1_3000_mcp")
    );
}

#[test]
fn a_resumed_session_passes_no_server_flag() {
    let w = World::new();
    let call = worker(&w, Session::Resume(id(FRESH_ID)), repo(), Some(graph()));
    let argv = strings(&call);
    assert!(server_flags(&argv).is_empty(), "{argv:?}");
    // The builtin is de-duplicated by goose, so a resumed call keeps it.
    assert!(argv.contains(&"developer".to_owned()));
    // The call still loads the servers its session holds, so the hook still lets them through.
    assert_eq!(mcp::keys(&call).len(), 3);
    let mut new = call.clone();
    new.session = Session::New(id(OTHER_ID));
    assert_eq!(server_flags(&strings(&new)).len(), 3);
}

#[test]
fn a_call_with_no_server_passes_no_server_flag() {
    let w = World::new();
    let call = worker(&w, Session::New(id(FRESH_ID)), json!({}), None);
    let argv = strings(&call);
    assert!(server_flags(&argv).is_empty(), "{argv:?}");
    assert!(mcp::keys(&call).is_empty());
    stand_in(&w, FRESH).prepare(&call).unwrap();
    let command = hook_command(&w);
    assert!(!command.contains(GOOSE_SERVER_FLAG), "{command}");
}

#[test]
fn an_entry_goose_cannot_load_says_why_and_stays_out_of_the_flags() {
    let header = json!({ "type": "http", "url": "https://h.example.com/mcp",
                         "headers": { "Authorization": "Bearer x" } });
    assert!(refusal("hdr", &header).unwrap().contains("`headers`"));
    let sse = json!({ "type": "sse", "url": "https://old.example.com/sse" });
    assert!(refusal("old", &sse).unwrap().contains("SSE"));
    let quoted = json!({ "command": "/k/s", "args": ["say \"hi\""] });
    let why = refusal("quote", &quoted).unwrap();
    assert!(
        why.contains("double quote") && why.contains("`args`"),
        "{why}"
    );
    let in_env = json!({ "command": "/k/s", "env": { "T": "a\"b" } });
    assert!(refusal("quote", &in_env).unwrap().contains("`env`"));
    for (name, entry, mentions) in [
        ("x", json!({ "command": "/k/s" }), "name"),
        ("has space", json!({ "command": "/k/s" }), "name"),
        ("developer", json!({ "command": "/k/s" }), "developer"),
        ("cwd", json!({ "command": "/k/s", "cwd": "/k" }), "folder"),
        (
            "var",
            json!({ "command": "/k/s", "args": ["${TOKEN}"] }),
            "VAR",
        ),
        ("eq", json!({ "command": "/k/a=b" }), "env pair"),
        ("empty", json!({ "command": "/k/s", "args": [""] }), "empty"),
        ("none", json!({ "args": [] }), "neither"),
        ("ws", json!({ "type": "ws", "url": "wss://x" }), "`ws`"),
        (
            "ip",
            json!({ "url": "http://[::1]:3000/mcp" }),
            "name goose would give",
        ),
        (
            "key",
            json!({ "command": "/k/s", "env": { "A B": "1" } }),
            "`A B`",
        ),
    ] {
        let why = refusal(name, &entry).unwrap_or_else(|| panic!("{name} was loaded"));
        assert!(why.contains(mentions), "{name}: {why}");
    }
    // A file path is fine as a command, and so is a skipped key: goose is told without it.
    assert_eq!(
        refusal(
            "fine",
            &json!({ "command": "/k/s", "env": { "PATH": "/x" } })
        ),
        None
    );

    let w = World::new();
    let mut entries = repo();
    entries["hdr"] = header;
    entries["old"] = sse;
    entries["quote"] = quoted;
    let call = worker(&w, Session::New(id(FRESH_ID)), entries, None);
    // With no graph, a repo server of the graph's name is an ordinary one, in name order.
    assert_eq!(
        mcp::keys(&call),
        [
            "codebase-memory-mcp",
            "docs_example_com_mcp",
            "plugin_style_search"
        ]
    );
}

#[test]
fn two_servers_goose_names_alike_load_once() {
    let w = World::new();
    let call = worker(
        &w,
        Session::New(id(FRESH_ID)),
        json!({
            "Docs": { "command": "/k/a" },
            "docs": { "command": "/k/b" },
            "docs_example_com_mcp": { "command": "/k/c" },
            "web": { "url": "https://docs.example.com/mcp" },
        }),
        None,
    );
    // A name goose keys twice ends the run, so the later of the two is left out.
    assert_eq!(mcp::keys(&call), ["docs", "docs_example_com_mcp"]);
}

fn hook_command(w: &World) -> String {
    let hooks = w.path("settings.goose/.agents/plugins/kelpie-fence/hooks/hooks.json");
    let text = std::fs::read_to_string(hooks).unwrap();
    let hooks: Value = serde_json::from_str(&text).unwrap();
    hooks["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .to_owned()
}

// The arguments the hook's command runs `kelpie` with, as a stand-in kelpie keeps them.
fn hook_arguments(w: &World, call: &mut AgentCall) -> Vec<String> {
    let kelpie = w.path("kelpie-bin");
    let seen = w.path("seen");
    let script = format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\n", seen.display());
    let _ = std::fs::remove_file(&kelpie);
    crate::test::write_script(&kelpie, &script);
    call.reach.fence.as_mut().unwrap().guard.kelpie = kelpie;
    stand_in(w, FRESH).prepare(call).unwrap();
    let ran = Process::new("sh")
        .args(["-c", &hook_command(w)])
        .status()
        .unwrap();
    assert!(ran.success());
    let seen = std::fs::read_to_string(seen).unwrap();
    seen.lines().map(str::to_owned).collect()
}

#[test]
fn the_hook_lets_each_loaded_server_s_tools_through_and_nothing_else_it_refused() {
    let w = World::new();
    let mut call = worker(&w, Session::New(id(FRESH_ID)), repo(), Some(graph()));
    let arguments = hook_arguments(&w, &mut call);
    let (servers, rest) = goose_server_flags(&arguments);
    assert_eq!(
        servers,
        [
            "docs_example_com_mcp",
            "plugin_style_search",
            "codebase-memory-mcp"
        ]
    );
    // The flags follow the build folder's, and nothing else changed in the command.
    assert!(rest.last().unwrap().starts_with("--goose="), "{rest:?}");

    let judge = |tool: &str| {
        let payload = json!({
            "event": "PreToolUse",
            "session_id": "20260101_1",
            "matcher_context": tool,
            "tool_name": tool,
            "tool_input": {},
            "working_dir": w.path("wt"),
        });
        let checkout = Checkout {
            git_common_dir: Path::new("/nowhere"),
            worktree: &w.path("wt"),
            base: "main",
        };
        judge_goose(
            payload.to_string().as_bytes(),
            Some(Path::new("/home/me")),
            LocalPaths::new([Path::new("/home/me")]),
            &Hooks::default(),
            checkout,
            &w.path("build"),
            &servers,
        )
    };
    use crate::confine::Verdict;
    for tool in [
        "plugin_style_search__lookup",
        "codebase-memory-mcp__search_graph",
        "docs_example_com_mcp__fetch",
    ] {
        assert_eq!(judge(tool), Verdict::Allow, "{tool}");
    }
    // The name Claude Code gives an HTTP server is not goose's, and neither is a look-alike.
    for tool in [
        "docs__fetch",
        "plugin_style_search",
        "plugin_style_search__",
        "plugin_style_searcher__lookup",
        "Plugin_Style_Search__lookup",
        "delegate",
        "load",
        "read_image",
        "developer__shell",
        "other__tool",
    ] {
        assert!(matches!(judge(tool), Verdict::Refuse(_)), "{tool}");
    }
}

#[test]
fn a_researcher_that_loads_a_server_runs_in_auto_behind_the_hook_and_one_that_loads_none_in_chat() {
    let w = World::new();
    let mut call = worker(&w, Session::New(id(FRESH_ID)), repo(), Some(graph()));
    call.role = Role::Researcher;
    call.tools = Tools::Research;
    let vars = |call: &AgentCall| {
        let goose = GooseCli::on(&Base::default());
        let server = server(call).unwrap();
        let upstream = goose.upstream(server).unwrap();
        let files = Files::of(&call.settings);
        let vars = env_args(call, server, &files, &upstream, None);
        vars.into_iter()
            .map(|v| v.into_string().unwrap())
            .collect::<Vec<_>>()
    };
    assert!(vars(&call).contains(&"GOOSE_MODE=auto".to_owned()));
    let argv = strings(&call);
    // A researcher is offered no builtin but `skills`, and the servers.
    assert_eq!(
        argv.iter().filter(|a| *a == "--with-builtin").count(),
        1,
        "{argv:?}"
    );
    assert!(argv.contains(&"skills".to_owned()), "{argv:?}");
    assert_eq!(server_flags(&argv).len(), 3);
    let arguments = hook_arguments(&w, &mut call);
    assert_eq!(goose_server_flags(&arguments).0.len(), 3);

    let mut none = call.clone();
    none.reach.mcp_servers.clear();
    none.reach.code_graph = None;
    assert!(vars(&none).contains(&"GOOSE_MODE=chat".to_owned()));
    assert!(server_flags(&strings(&none)).is_empty());
}

#[test]
fn the_sandbox_reads_the_code_graph_binary_and_writes_only_its_index_and_daemon_folder() {
    let w = World::new();
    let policy_of = |call: &AgentCall| {
        let goose = GooseCli::on(&Base::default());
        let upstream = goose.upstream(server(call).unwrap()).unwrap();
        let files = Files::of(&call.settings);
        policy(call, &files, &upstream, &w.path("forwarder.sock"), &[])
    };
    let with = policy_of(&worker(
        &w,
        Session::New(id(FRESH_ID)),
        json!({}),
        Some(graph()),
    ));
    let without = policy_of(&worker(&w, Session::New(id(FRESH_ID)), json!({}), None));
    let binary = Path::new("/k/tools/codebase-memory-mcp");
    let index = Path::new("/k/graphs/7");
    let runtime = Path::new("/k/cbm/7");
    assert!(with.read.iter().any(|p| p == binary));
    assert!(with.read.iter().any(|p| p == index));
    assert!(with.write.iter().any(|p| p == index));
    assert!(with.write.iter().any(|p| p == runtime));
    assert!(with.sockets.iter().any(|p| p == runtime));
    assert!(!with.write.iter().any(|p| p == binary));
    // Nothing else changed, and no host was added for the server.
    assert_eq!(with.write.len(), without.write.len() + 2);
    assert_eq!(with.read.len(), without.read.len() + 3);
    assert_eq!(with.sockets.len(), without.sockets.len() + 1);
    assert_eq!(with.hosts, without.hosts);
    assert!(!without.read.iter().any(|p| p == binary || p == index));

    // A call whose servers are the repo's alone is given no folder besides.
    let repo_only = Reach {
        mcp_servers: servers(repo()),
        ..Reach::default()
    };
    let mut call = worker(&w, Session::New(id(FRESH_ID)), json!({}), None);
    call.reach.mcp_servers = repo_only.mcp_servers;
    let plain = policy_of(&call);
    assert_eq!(plain.write, without.write);
    assert_eq!(plain.read, without.read);
}

#[test]
fn a_new_session_with_a_browser_loads_playwright_and_the_sandbox_connects_only_to_its_socket() {
    let w = World::new();
    let socket = w.path("0123456789abcdef0123456789abcdef.sock");
    let policy_of = |call: &AgentCall| {
        let goose = GooseCli::on(&Base::default());
        let upstream = goose.upstream(server(call).unwrap()).unwrap();
        let files = Files::of(&call.settings);
        policy(call, &files, &upstream, &w.path("forwarder.sock"), &[])
    };
    let mut call = worker(&w, Session::New(id(FRESH_ID)), json!({}), None);
    let without = policy_of(&call);
    let argv_without = strings(&call);
    call.reach.browser = Some(Box::new(Browser {
        kelpie: "/k/kelpie".into(),
        socket: socket.clone(),
    }));
    let with = policy_of(&call);
    let argv_with = strings(&call);

    assert!(!without.sockets.contains(&socket));
    assert!(server_flags(&argv_without).is_empty(), "{argv_without:?}");
    let new: Vec<_> = with
        .sockets
        .iter()
        .filter(|s| !without.sockets.contains(s))
        .collect();
    assert_eq!(new, [&socket]);
    // Read back the way goose reads it, the server carries the socket the sandbox allows.
    let flags = server_flags(&argv_with);
    assert_eq!(flags.len(), 1, "{argv_with:?}");
    assert_eq!(flags[0].0, "--with-extension");
    let (name, _, command, args) = goose_reads(&flags[0].1);
    assert_eq!(name.as_deref(), Some("playwright"));
    assert_eq!(command, "/k/kelpie");
    assert_eq!(args, ["mcp-connect", socket.to_str().unwrap()]);

    // A resumed session keeps the set it began with.
    call.session = Session::Resume(id(FRESH_ID));
    assert!(server_flags(&strings(&call)).is_empty());
}

#[test]
fn the_browser_wins_a_key_a_repo_server_of_another_name_shares() {
    let w = World::new();
    let mut call = worker(
        &w,
        Session::New(id(FRESH_ID)),
        json!({
            "Playwright": { "command": "/repo/impostor" },
            "docs": { "command": "/k/docs" },
        }),
        None,
    );
    call.reach.browser = Some(Box::new(Browser {
        kelpie: "/k/kelpie".into(),
        socket: w.path("0123456789abcdef0123456789abcdef.sock"),
    }));
    assert_eq!(mcp::keys(&call), ["docs", "playwright"]);
    let argv = strings(&call).join(" ");
    assert!(
        argv.contains("/k/kelpie") && !argv.contains("impostor"),
        "{argv}"
    );
}

#[test]
fn the_code_graph_wins_a_key_a_repo_server_of_another_name_shares() {
    let w = World::new();
    // `Codebase-memory-mcp` lowercases to the graph's key, and the URL derives to it.
    let call = worker(
        &w,
        Session::New(id(FRESH_ID)),
        json!({
            "Codebase-memory-mcp": { "command": "/repo/impostor" },
            "viaurl": { "url": "http://codebase-memory-mcp/" },
            "docs": { "command": "/k/docs" },
        }),
        Some(graph()),
    );
    assert_eq!(mcp::keys(&call), ["docs", "codebase-memory-mcp"]);
    let argv = strings(&call).join(" ");
    assert!(argv.contains("/k/tools/codebase-memory-mcp"), "{argv}");
    assert!(
        !argv.contains("impostor") && !argv.contains("http://codebase"),
        "{argv}"
    );
}

#[test]
fn an_http_server_goose_would_key_developer_is_refused_beside_the_builtin() {
    let entry = json!({ "type": "http", "url": "https://developer/mcp" });
    assert_eq!(
        derived_key("http://developer/").as_deref(),
        Some("developer")
    );
    let why = refusal("dev", &json!({ "url": "http://developer/" })).unwrap();
    assert!(why.contains("developer"), "{why}");
    // The key is the URL's host and path, so a path after it is a different key.
    assert_eq!(refusal("dev", &entry), None);
    let w = World::new();
    let call = worker(
        &w,
        Session::New(id(FRESH_ID)),
        json!({ "dev": { "url": "http://developer/" } }),
        None,
    );
    assert!(mcp::keys(&call).is_empty());
    assert!(server_flags(&strings(&call)).is_empty());
}

#[test]
fn a_resumed_session_s_hook_and_mode_follow_the_set_it_was_created_with() {
    let w = World::new();
    let mode = |call: &AgentCall| {
        let goose = GooseCli::on(&Base::default());
        let server = server(call).unwrap();
        let upstream = goose.upstream(server).unwrap();
        let files = Files::of(&call.settings);
        let vars = env_args(call, server, &files, &upstream, None);
        vars.into_iter()
            .map(|v| v.into_string().unwrap())
            .find(|v| v.starts_with("GOOSE_MODE="))
            .unwrap()
    };
    let researcher = |session, servers| {
        let mut call = worker(&w, session, servers, None);
        call.role = Role::Researcher;
        call.tools = Tools::Research;
        call
    };
    let mut first = researcher(Session::New(id(FRESH_ID)), repo());
    let created = [
        "codebase-memory-mcp",
        "docs_example_com_mcp",
        "plugin_style_search",
    ];
    assert_eq!(
        goose_server_flags(&hook_arguments(&w, &mut first)).0,
        created
    );

    // The repo's set has since changed: the resumed session still holds the first one.
    let changed = json!({ "renamed": { "command": "/k/renamed" } });
    let mut later = researcher(Session::Resume(id(FRESH_ID)), changed.clone());
    assert_eq!(
        goose_server_flags(&hook_arguments(&w, &mut later)).0,
        created
    );
    assert!(server_flags(&strings(&later)).is_empty());
    assert_eq!(mode(&later), "GOOSE_MODE=auto");

    // A set that is empty now does not switch a session that holds servers to chat.
    let mut emptied = researcher(Session::Resume(id(FRESH_ID)), json!({}));
    assert_eq!(
        goose_server_flags(&hook_arguments(&w, &mut emptied)).0,
        created
    );
    assert_eq!(mode(&emptied), "GOOSE_MODE=auto");

    // A session created with none holds none, whatever the repo has since gained. (A fresh
    // folder, since a call that loads no server leaves an earlier hook file as it was.)
    let w = World::new();
    let held = |session, servers| {
        let mut call = worker(&w, session, servers, None);
        call.role = Role::Researcher;
        call.tools = Tools::Research;
        stand_in(&w, FRESH).prepare(&call).unwrap();
        (
            mcp::held(&call, &Files::of(&call.settings)).unwrap(),
            mode(&call),
        )
    };
    assert_eq!(
        held(Session::New(id(OTHER_ID)), json!({})),
        (Vec::<String>::new(), "GOOSE_MODE=chat".to_owned())
    );
    assert_eq!(
        held(Session::Resume(id(OTHER_ID)), changed),
        (Vec::<String>::new(), "GOOSE_MODE=chat".to_owned())
    );
}

#[test]
fn a_reviewer_s_read_only_hook_names_the_servers_it_loads() {
    let w = World::new();
    let mut call = worker(&w, Session::New(id(FRESH_ID)), repo(), Some(graph()));
    call.role = Role::Reviewer;
    call.tools = Tools::Review;
    call.reach.fence.as_mut().unwrap().write.clear();
    let arguments = hook_arguments(&w, &mut call);
    assert!(
        arguments.contains(&"--read-only".to_owned()),
        "{arguments:?}"
    );
    let (keys, _) = goose_server_flags(&arguments);
    assert_eq!(keys, mcp::keys(&call));
    assert_eq!(keys.len(), 3);
}
