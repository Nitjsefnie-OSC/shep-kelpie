//! The MCP servers in a pi call's own home
//!
//! pi's built-in MCP support reads `mcp.json` from its agent folder, which is the call's own home.
//! A call that loads a server gets that file and `-e builtin:mcp`, since `--no-extensions` turns
//! the built-in off. Any other call gets `--no-mcp`, so no other config can start one. The
//! worktree's `.pi/mcp.json` is a project file, and `--no-approve` keeps it unread.

use std::path::PathBuf;

use serde_json::{Map, Value, json};

use super::Files;
use crate::ports::mcp::{servers, tool_prefix};
use crate::ports::{AgentCall, AgentError};

/// The built-in extension that connects the servers
pub(super) const EXTENSION: &str = "builtin:mcp";

/// Where the call's servers go in its home
pub(super) fn file(files: &Files) -> PathBuf {
    files.home.join("mcp.json")
}

/// The call's `mcp.json`, where it loads a server pi can load
pub(super) fn config(call: &AgentCall) -> Option<Value> {
    let loaded: Map<String, Value> = servers(call)
        .into_iter()
        .filter(|(name, entry)| refusal(name, entry).is_none())
        .filter_map(|(name, entry)| Some((name, converted(&entry).ok()?)))
        .collect();
    (!loaded.is_empty()).then(|| json!({ "mcpServers": loaded }))
}

/// Why pi cannot load a Claude-shaped `mcpServers` entry, if it cannot
pub(crate) fn refusal(name: &str, entry: &Value) -> Option<String> {
    let named = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !named {
        return Some("pi takes only letters, digits, `_` and `-` in a server's name".to_owned());
    }
    converted(entry).err()
}

/// The `--tools` entries that keep the call's servers' tools
///
/// Once an entry starts with `mcp__`, `--tools` filters MCP tools too, so each server is named.
pub(super) fn tool_names(call: &AgentCall) -> Vec<String> {
    let loaded = config(call);
    let names = loaded
        .iter()
        .filter_map(|config| config["mcpServers"].as_object())
        .flat_map(Map::keys);
    names
        .map(|name| format!("mcp__{}__*", tool_prefix(name)))
        .collect()
}

/// Writes the call's `mcp.json`, and removes the one an earlier call left when it loads none
///
/// # Errors
///
/// [`AgentError::Setup`] when the file cannot be written or removed.
pub(super) fn prepare(config: Option<Value>, files: &Files) -> Result<(), AgentError> {
    let path = file(files);
    let setup = |e: std::io::Error| {
        AgentError::Setup(format!("cannot write {}: {}", path.display(), e.kind()))
    };
    match config {
        Some(config) => {
            let text = serde_json::to_string_pretty(&config).expect("a config is JSON");
            std::fs::write(&path, text).map_err(setup)
        }
        None => match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(setup(e)),
            _ => Ok(()),
        },
    }
}

// pi's entry for a Claude-shaped one. The tools are declared to the model, as Claude Code's are:
// pi's default has them reachable only through a codemode script.
fn converted(entry: &Value) -> Result<Value, String> {
    let has = |field: &str| entry.get(field).is_some();
    let kind = match (entry["type"].as_str(), has("command"), has("url")) {
        (Some("stdio"), ..) | (None, true, _) => "stdio",
        (Some("http" | "streamable-http"), ..) | (None, false, true) => "http",
        (Some("sse"), ..) => {
            return Err(
                "pi has no SSE transport: the server may offer streamable HTTP, \
                        commonly at `/mcp`"
                    .to_owned(),
            );
        }
        (Some(other), ..) => return Err(format!("pi has no `{other}` transport")),
        (None, false, false) => return Err("it names neither a `command` nor a `url`".to_owned()),
    };
    let (required, copied): (&str, &[&str]) = match kind {
        "stdio" => ("command", &["command", "args", "env", "cwd"]),
        _ => ("url", &["url", "headers"]),
    };
    if !entry[required].is_string() {
        return Err(format!("its `{required}` is not a string"));
    }
    let mut out = Map::from_iter([("type".to_owned(), json!(kind))]);
    for field in copied {
        if let Some(value) = entry.get(*field) {
            out.insert((*field).to_owned(), value.clone());
        }
    }
    // pi expands `${VAR}` in `env` and `headers` only, and Claude Code also in these.
    let texts = [("command", entry["command"].as_str())]
        .into_iter()
        .chain([("url", entry["url"].as_str())])
        .chain([("cwd", entry["cwd"].as_str())])
        .chain(
            entry["args"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|arg| ("args", arg.as_str())),
        );
    for (field, text) in texts {
        if text.is_some_and(|text| text.contains("${")) {
            return Err(format!(
                "its `{field}` holds a `${{VAR}}`, which pi expands only in `env` and `headers`"
            ));
        }
    }
    // pi runs a value of `env` or `headers` that starts with `!` as a command.
    for field in ["env", "headers"] {
        let mut values = entry[field].as_object().into_iter().flatten();
        if let Some((key, _)) = values.find(|(_, v)| v.as_str().is_some_and(|v| v.starts_with('!')))
        {
            return Err(format!(
                "its `{field}` value `{key}` starts with `!`, which pi runs as a command"
            ));
        }
    }
    out.insert("exposure".to_owned(), json!("direct"));
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::pi::tests::{World, id, stand_in, strings};
    use crate::ports::{Agents, Browser, CodeGraph, Reach, Role, Session};

    const URL: &str = "http://model.kelpie.test/v1";
    const SESSION: &str = "0b7f1c52-3b64-4f8e-9a52-0f6a2f1d9c11";

    fn reach() -> Reach {
        Reach {
            mcp_servers: [
                (
                    "docs".to_owned(),
                    json!({ "type": "http", "url": "https://docs.example.com/mcp",
                            "headers": { "Authorization": "Bearer ${DOCS_TOKEN}" } }),
                ),
                (
                    "plugin_style_search".to_owned(),
                    json!({ "command": "/k/search", "args": ["--stdio"],
                            "env": { "INDEX": "/k/index" }, "cwd": "/k/tools" }),
                ),
            ]
            .into(),
            code_graph: Some(CodeGraph {
                binary: "/k/tools/codebase-memory-mcp".into(),
                index: "/k/graphs/7".into(),
                runtime: "/k/cbm/7".into(),
            }),
            ..Reach::default()
        }
    }

    fn with_reach(w: &World, reach: Reach) -> AgentCall {
        let mut call = w.call(URL, Role::Worker, Session::New(id(SESSION)));
        call.tools = crate::ports::Tools::Work;
        call.reach = reach;
        call
    }

    #[test]
    fn stdio_and_http_entries_take_pis_shape() {
        let w = World::new();
        let config = config(&with_reach(&w, reach())).unwrap();
        assert_eq!(
            config["mcpServers"]["plugin_style_search"],
            json!({
                "type": "stdio",
                "command": "/k/search",
                "args": ["--stdio"],
                "env": { "INDEX": "/k/index" },
                "cwd": "/k/tools",
                "exposure": "direct",
            })
        );
        assert_eq!(
            config["mcpServers"]["docs"],
            json!({
                "type": "http",
                "url": "https://docs.example.com/mcp",
                "headers": { "Authorization": "Bearer ${DOCS_TOKEN}" },
                "exposure": "direct",
            })
        );
        let mut untyped = Reach::default();
        untyped
            .mcp_servers
            .insert("local".into(), json!({ "command": "/k/local" }));
        untyped
            .mcp_servers
            .insert("remote".into(), json!({ "url": "https://r.example.com" }));
        let config = self::config(&with_reach(&w, untyped)).unwrap();
        assert_eq!(config["mcpServers"]["local"]["type"], "stdio");
        assert_eq!(config["mcpServers"]["remote"]["type"], "http");
    }

    #[test]
    fn a_call_with_servers_gets_an_mcp_file_in_pis_home_with_each_server_and_the_graph() {
        let w = World::new();
        let call = with_reach(&w, reach());
        stand_in(&w, "").prepare(&call).unwrap();

        let path = w.path("settings.pi/mcp.json");
        let written: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let names: Vec<_> = written["mcpServers"].as_object().unwrap().keys().collect();
        assert_eq!(
            names,
            ["codebase-memory-mcp", "docs", "plugin_style_search"]
        );
        assert_eq!(
            written["mcpServers"]["codebase-memory-mcp"],
            json!({
                "type": "stdio",
                "command": "/k/tools/codebase-memory-mcp",
                "args": [],
                "env": {
                    "CBM_CACHE_DIR": "/k/graphs/7",
                    "CBM_RUNTIME_DIR": "/k/cbm/7",
                    "CBM_ALLOWED_ROOT": w.path("wt"),
                },
                "exposure": "direct",
            })
        );
        let argv = strings(&call);
        let at = argv.iter().position(|a| a == "--extension").unwrap();
        assert_eq!(argv[at + 1], "builtin:mcp");
        assert!(!argv.contains(&"--no-mcp".to_owned()));
    }

    #[test]
    fn a_call_with_no_server_gets_no_mcp_file_and_no_mcp() {
        let w = World::new();
        let pi = stand_in(&w, "");
        pi.prepare(&with_reach(&w, reach())).unwrap();
        let path = w.path("settings.pi/mcp.json");
        assert!(path.is_file());

        let call = with_reach(&w, Reach::default());
        pi.prepare(&call).unwrap();
        assert!(!path.exists(), "the last call's file stayed");
        let argv = strings(&call);
        assert!(argv.contains(&"--no-mcp".to_owned()), "{argv:?}");
        assert!(!argv.contains(&"builtin:mcp".to_owned()), "{argv:?}");
    }

    #[test]
    fn the_tools_of_each_server_are_named_in_tools() {
        let w = World::new();
        let argv = strings(&with_reach(&w, reach()));
        let at = argv.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(
            argv[at + 1],
            "read,bash,edit,write,grep,find,ls,\
             mcp__codebase_memory_mcp__*,mcp__docs__*,mcp__plugin_style_search__*"
        );
    }

    #[test]
    fn an_entry_pi_cannot_load_says_why_and_stays_out_of_the_file() {
        let sse = json!({ "type": "sse", "url": "https://old.example.com/sse" });
        assert!(refusal("old", &sse).unwrap().contains("SSE"));
        let bang = json!({ "command": "/k/s", "env": { "TOKEN": "!gh auth token" } });
        let why = refusal("bang", &bang).unwrap();
        assert!(why.contains("`TOKEN`") && why.contains('!'), "{why}");
        assert!(refusal("a name", &json!({ "command": "/k/s" })).is_some());
        assert!(refusal("none", &json!({ "args": [] })).is_some());
        assert_eq!(refusal("fine", &json!({ "command": "/k/s" })), None);
        for entry in [
            json!({ "command": "/k/s", "args": ["--token", "${DOCS_TOKEN}"] }),
            json!({ "url": "${DOCS_URL}/mcp" }),
            json!({ "command": "${HOME}/s" }),
        ] {
            let why = refusal("var", &entry).unwrap();
            assert!(why.contains("${VAR}"), "{why}");
        }
        let in_env = json!({ "command": "/k/s", "env": { "T": "${DOCS_TOKEN}" } });
        assert_eq!(refusal("var", &in_env), None);

        let w = World::new();
        let mut loaded = Reach::default();
        loaded.mcp_servers.insert("old".into(), sse);
        loaded
            .mcp_servers
            .insert("fine".into(), json!({ "command": "/k/s" }));
        loaded
            .mcp_servers
            .insert("a name".into(), json!({ "command": "/k/s" }));
        let call = with_reach(&w, loaded);
        let config = config(&call).unwrap();
        let names: Vec<_> = config["mcpServers"].as_object().unwrap().keys().collect();
        assert_eq!(names, ["fine"]);
        assert_eq!(tool_names(&call), ["mcp__fine__*"]);
    }

    #[test]
    fn a_call_with_a_browser_gets_playwright_in_the_mcp_file_and_its_tools_in_tools() {
        let w = World::new();
        let reach = Reach {
            browser: Some(Box::new(Browser {
                kelpie: "/k/bin/shep-kelpie".into(),
                socket: "/k/worker/0123456789abcdef0123456789abcdef.sock".into(),
            })),
            ..Reach::default()
        };
        let call = with_reach(&w, reach);
        stand_in(&w, "").prepare(&call).unwrap();

        let path = w.path("settings.pi/mcp.json");
        let written: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(
            written["mcpServers"]["playwright"],
            json!({
                "type": "stdio",
                "command": "/k/bin/shep-kelpie",
                "args": ["mcp-connect", "/k/worker/0123456789abcdef0123456789abcdef.sock"],
                "exposure": "direct",
            })
        );
        let argv = strings(&call);
        let at = argv.iter().position(|a| a == "--tools").unwrap();
        assert!(argv[at + 1].ends_with(",mcp__playwright__*"), "{argv:?}");
    }
}
