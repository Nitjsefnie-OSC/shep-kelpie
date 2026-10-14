//! The MCP servers in a Codex call's `-c` overrides
//!
//! A call runs with `--ignore-user-config`, so it loads no server of its own. Each server it is
//! given is one `-c mcp_servers.<name>=<inline table>` pair, in the shape Codex's `config.toml`
//! takes (`docs/facts/codex-as-an-agent.md`).

use std::ffi::OsString;

use serde_json::Value;
use toml::Table;

use crate::adapters::mcp::servers;
use crate::ports::AgentCall;

/// The `-c` pairs that load the call's servers, one per server Codex can load
pub(super) fn overrides(call: &AgentCall) -> Vec<OsString> {
    servers(call)
        .into_iter()
        .filter(|(name, _)| named(name))
        .filter_map(|(name, entry)| Some((name, converted(&entry).ok()?)))
        .flat_map(|(name, table)| {
            [
                OsString::from("-c"),
                super::config(&format!("mcp_servers.{name}"), table),
            ]
        })
        .collect()
}

/// Why Codex cannot load a Claude-shaped `mcpServers` entry, if it cannot
pub(crate) fn refusal(name: &str, entry: &Value) -> Option<String> {
    if !named(name) {
        return Some("Codex takes only letters, digits, `_` and `-` in a server's name".to_owned());
    }
    converted(entry).err()
}

// A name that is a bare key of the `-c` path, which Codex also takes as a server's name.
fn named(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

// Codex's table for a Claude-shaped entry. Codex expands no `${VAR}` of its own, so an entry that
// holds one is refused, and a field Codex has no name for is left out.
fn converted(entry: &Value) -> Result<Table, String> {
    let has = |field: &str| entry.get(field).is_some();
    let (stdio, text) = match (entry["type"].as_str(), has("command"), has("url")) {
        (Some("stdio"), ..) | (None, true, _) => (true, "command"),
        (Some("http" | "streamable-http"), ..) | (None, false, true) => (false, "url"),
        (Some("sse"), ..) => {
            return Err(
                "Codex has no SSE transport: the server may offer streamable HTTP, \
                        commonly at `/mcp`"
                    .to_owned(),
            );
        }
        (Some(other), ..) => return Err(format!("Codex has no `{other}` transport")),
        (None, false, false) => return Err("it names neither a `command` nor a `url`".to_owned()),
    };
    let mut out = Table::new();
    out.insert(
        text.to_owned(),
        string(entry, text)?
            .ok_or_else(|| format!("its `{text}` is not a string"))?
            .into(),
    );
    if stdio {
        if let Some(args) = list(entry, "args")? {
            out.insert("args".to_owned(), args.into());
        }
        if let Some(env) = map(entry, "env")? {
            out.insert("env".to_owned(), env.into());
        }
        if let Some(cwd) = string(entry, "cwd")? {
            out.insert("cwd".to_owned(), cwd.into());
        }
    } else if let Some(headers) = map(entry, "headers")? {
        out.insert("http_headers".to_owned(), headers.into());
    }
    Ok(out)
}

// A string field, if the entry has it.
fn string(entry: &Value, field: &str) -> Result<Option<String>, String> {
    let Some(value) = entry.get(field) else {
        return Ok(None);
    };
    let text = value
        .as_str()
        .ok_or_else(|| format!("its `{field}` is not a string"))?;
    plain(field, text).map(|()| Some(text.to_owned()))
}

fn list(entry: &Value, field: &str) -> Result<Option<Vec<String>>, String> {
    let Some(value) = entry.get(field) else {
        return Ok(None);
    };
    let items = value
        .as_array()
        .ok_or_else(|| format!("its `{field}` is not a list"))?;
    let mut out = Vec::new();
    for item in items {
        let text = item
            .as_str()
            .ok_or_else(|| format!("its `{field}` holds a value that is not a string"))?;
        plain(field, text)?;
        out.push(text.to_owned());
    }
    Ok(Some(out))
}

fn map(entry: &Value, field: &str) -> Result<Option<Table>, String> {
    let Some(value) = entry.get(field) else {
        return Ok(None);
    };
    let pairs = value
        .as_object()
        .ok_or_else(|| format!("its `{field}` is not a table"))?;
    let mut out = Table::new();
    for (key, value) in pairs {
        let text = value
            .as_str()
            .ok_or_else(|| format!("its `{field}` value `{key}` is not a string"))?;
        plain(field, text)?;
        out.insert(key.clone(), text.to_owned().into());
    }
    Ok(Some(out))
}

fn plain(field: &str, text: &str) -> Result<(), String> {
    if text.contains("${") {
        return Err(format!(
            "its `{field}` holds a `${{VAR}}`, which Codex does not expand"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::adapters::codex::tests::{WORKER_ID, World, id, strings};
    use crate::ports::{CodeGraph, Reach, Role, Session, Tools};

    fn graph() -> CodeGraph {
        CodeGraph {
            binary: "/k/tools/codebase-memory-mcp".into(),
            index: "/k/graphs/7".into(),
        }
    }

    fn call(w: &World, reach: Reach) -> AgentCall {
        AgentCall {
            tools: Tools::Work,
            reach,
            ..w.call(Role::Worker, Session::New(id(WORKER_ID)))
        }
    }

    // The servers the overrides name, read back the way Codex reads `-c`: each pair as a TOML line.
    fn loaded(overrides: &[OsString]) -> toml::Table {
        let mut table = toml::Table::new();
        for pair in overrides.chunks(2) {
            assert_eq!(pair[0], "-c", "{overrides:?}");
            let line: Table = toml::from_str(pair[1].to_str().unwrap()).unwrap();
            let Some(toml::Value::Table(servers)) = line.get("mcp_servers") else {
                panic!("{pair:?} is not a server");
            };
            table.extend(servers.clone());
        }
        table
    }

    #[test]
    fn a_call_with_servers_gets_one_override_per_server_and_the_graph_cannot_be_replaced() {
        let w = World::new();
        let reach = Reach {
            mcp_servers: [
                (
                    "docs".to_owned(),
                    json!({ "type": "http", "url": "https://docs.example.com/mcp" }),
                ),
                (
                    "codebase-memory-mcp".to_owned(),
                    json!({ "command": "/repo/impostor" }),
                ),
                ("a name".to_owned(), json!({ "command": "/k/s" })),
            ]
            .into(),
            code_graph: Some(graph()),
            ..Reach::default()
        };
        let call = call(&w, reach);
        let overrides = overrides(&call);

        assert_eq!(overrides.len(), 4, "{overrides:?}");
        let servers = loaded(&overrides);
        let names: Vec<_> = servers.keys().collect();
        assert_eq!(names, ["codebase-memory-mcp", "docs"]);
        assert_eq!(
            servers["codebase-memory-mcp"]["command"].as_str(),
            Some("/k/tools/codebase-memory-mcp")
        );
        assert_eq!(
            servers["codebase-memory-mcp"]["env"]["CBM_CACHE_DIR"].as_str(),
            Some("/k/graphs/7")
        );
        assert_eq!(
            servers["codebase-memory-mcp"]["env"]["CBM_ALLOWED_ROOT"].as_str(),
            call.cwd.to_str()
        );
    }

    #[test]
    fn stdio_and_http_entries_take_codexs_shape() {
        let w = World::new();
        let reach = Reach {
            mcp_servers: [
                (
                    "docs".to_owned(),
                    json!({ "type": "http", "url": "https://docs.example.com/mcp",
                            "headers": { "X-Team": "kelpie" } }),
                ),
                (
                    "search".to_owned(),
                    json!({ "command": "/k/search", "args": ["--stdio"],
                            "env": { "INDEX": "/k/index" }, "cwd": "/k/tools" }),
                ),
                (
                    "remote".to_owned(),
                    json!({ "url": "https://r.example.com" }),
                ),
            ]
            .into(),
            ..Reach::default()
        };
        let servers = loaded(&overrides(&call(&w, reach)));

        let expected =
            |text: &str| -> toml::Value { toml::from_str::<Table>(text).unwrap().into() };
        assert_eq!(
            servers["search"],
            expected(
                r#"command = "/k/search"
                   args = ["--stdio"]
                   cwd = "/k/tools"
                   [env]
                   INDEX = "/k/index""#
            )
        );
        assert_eq!(
            servers["docs"],
            expected(
                r#"url = "https://docs.example.com/mcp"
                   [http_headers]
                   X-Team = "kelpie""#
            )
        );
        assert_eq!(
            servers["remote"],
            expected(r#"url = "https://r.example.com""#)
        );
    }

    #[test]
    fn a_call_with_no_server_adds_no_override() {
        let w = World::new();
        assert!(overrides(&call(&w, Reach::default())).is_empty());
    }

    #[test]
    fn an_entry_codex_cannot_load_says_why_and_gets_no_override() {
        let sse = json!({ "type": "sse", "url": "https://old.example.com/sse" });
        assert!(refusal("old", &sse).unwrap().contains("SSE"));
        assert!(refusal("a name", &json!({ "command": "/k/s" })).is_some());
        assert!(refusal("none", &json!({ "args": [] })).is_some());
        assert!(refusal("odd", &json!({ "command": "/k/s", "env": { "N": 1 } })).is_some());
        assert_eq!(refusal("fine", &json!({ "command": "/k/s" })), None);
        for entry in [
            json!({ "command": "/k/s", "args": ["--token", "${DOCS_TOKEN}"] }),
            json!({ "command": "/k/s", "env": { "T": "${DOCS_TOKEN}" } }),
            json!({ "url": "https://d.example.com", "headers": { "A": "Bearer ${T}" } }),
            json!({ "command": "${HOME}/s" }),
        ] {
            let why = refusal("var", &entry).unwrap();
            assert!(why.contains("${VAR}"), "{why}");
        }

        let w = World::new();
        let mut reach = Reach::default();
        reach.mcp_servers.insert("old".into(), sse);
        reach
            .mcp_servers
            .insert("fine".into(), json!({ "command": "/k/s" }));
        let overrides = overrides(&call(&w, reach));
        assert_eq!(loaded(&overrides).keys().collect::<Vec<_>>(), ["fine"]);
    }

    #[test]
    fn a_call_with_a_server_carries_its_override_and_keeps_the_features_off() {
        let w = World::new();
        let mut call = w.call(Role::Reviewer, Session::New(id(WORKER_ID)));
        call.reach.code_graph = Some(graph());
        let argv = strings(&call, None, None);

        let at = argv
            .iter()
            .position(|a| a.starts_with("mcp_servers.codebase-memory-mcp="))
            .expect("the override");
        assert_eq!(argv[at - 1], "-c");
        assert!(
            argv[at].contains("/k/tools/codebase-memory-mcp"),
            "{argv:?}"
        );
        assert!(argv.contains(&"--ignore-user-config".to_owned()));
        for feature in [
            "apps",
            "plugins",
            "skill_mcp_dependency_install",
            "shell_tool",
        ] {
            let at = argv.iter().position(|a| a == feature).expect(feature);
            assert_eq!(argv[at - 1], "--disable");
        }
    }
}
