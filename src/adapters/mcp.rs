//! The MCP servers a call loads, whichever harness runs it
//!
//! A call loads the servers of the repo's `.mcp.json` and its enabled plugins, and the code graph
//! and the browser where the call has them. Each harness writes them in its own file and shape.

use std::path::Path;

use serde_json::{Value, json};

use crate::ports::{AgentCall, Browser, CodeGraph, Policy};

/// The name the code graph server is known by, which its tools are named after
pub(crate) const SERVER: &str = "codebase-memory-mcp";

/// The servers the call loads, by name: the repo's, then the code graph and the browser, which a
/// repo's server of the same name does not replace
pub(crate) fn servers(call: &AgentCall) -> serde_json::Map<String, Value> {
    let mut servers: serde_json::Map<String, Value> = (call.reach.mcp_servers.iter())
        .map(|(name, entry)| (name.clone(), entry.clone()))
        .collect();
    if let Some(code_graph) = &call.reach.code_graph {
        servers.insert(SERVER.to_owned(), entry(code_graph, &call.cwd));
    }
    if let Some(browser) = &call.reach.browser {
        servers.insert(crate::bridge::SERVER.to_owned(), browser_entry(browser));
    }
    servers
}

/// Whether the call loads any server
pub(crate) fn loads(call: &AgentCall) -> bool {
    let (graph, browser) = (
        call.reach.code_graph.is_some(),
        call.reach.browser.is_some(),
    );
    graph || browser || !call.reach.mcp_servers.is_empty()
}

/// The code graph server as a Claude-shaped `mcpServers` entry: it starts over `graph`'s index, held to the call's folder
pub(crate) fn entry(graph: &CodeGraph, cwd: &Path) -> Value {
    json!({
        "type": "stdio",
        "command": graph.binary,
        "args": [],
        "env": {
            "CBM_CACHE_DIR": graph.index,
            "CBM_ALLOWED_ROOT": cwd,
        },
    })
}

/// The browser as a Claude-shaped `mcpServers` entry: `kelpie mcp-connect` carries the session to
/// the call's socket
pub(crate) fn browser_entry(browser: &Browser) -> Value {
    json!({
        "type": "stdio",
        "command": browser.kelpie,
        "args": ["mcp-connect", browser.socket],
    })
}

/// Lets the sandbox connect to the browser's socket, where the call has one
pub(crate) fn fence_the_browser(policy: &mut Policy, call: &AgentCall) {
    if let Some(browser) = &call.reach.browser {
        policy.sockets.push(browser.socket.clone());
    }
}

/// Lets the sandbox read the code graph's binary and index and write its index, where the call has one
pub(crate) fn fence_the_graph(policy: &mut Policy, call: &AgentCall) {
    if let Some(graph) = &call.reach.code_graph {
        policy
            .read
            .extend([graph.binary.clone(), graph.index.clone()]);
        policy.write.push(graph.index.clone());
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::ports::{Reach, Role, Session, SessionId, Tools};
    use crate::settings::{AgentHarness, Effort};

    fn graph() -> CodeGraph {
        CodeGraph {
            binary: "/k/tools/codebase-memory-mcp".into(),
            index: "/k/graphs/7".into(),
        }
    }

    fn call(reach: Reach) -> AgentCall {
        AgentCall {
            issue: 7,
            reach,
            ..AgentCall::new(
                Role::Worker,
                Tools::Work,
                AgentHarness::ClaudeCode,
                "claude-sonnet-5",
                Effort::Medium,
                Session::New(SessionId("7e812e8a-3bf1-42a7-bddf-0ab283b7372a".into())),
                PathBuf::from("/work/7"),
                "go",
            )
        }
    }

    #[test]
    fn a_repo_server_cannot_take_the_code_graphs_place() {
        let reach = Reach {
            mcp_servers: [
                (
                    "docs".to_owned(),
                    json!({ "type": "http", "url": "https://docs.example.com" }),
                ),
                (
                    "codebase-memory-mcp".to_owned(),
                    json!({ "command": "/repo/impostor" }),
                ),
            ]
            .into(),
            code_graph: Some(graph()),
            ..Reach::default()
        };
        let servers = servers(&call(reach));
        assert_eq!(
            servers["codebase-memory-mcp"]["command"],
            "/k/tools/codebase-memory-mcp"
        );
        assert!(servers.contains_key("docs"));
    }

    #[test]
    fn the_entry_starts_the_server_on_the_calls_index_and_folder() {
        let server = entry(&graph(), Path::new("/work/7"));
        assert_eq!(server["command"], "/k/tools/codebase-memory-mcp");
        assert_eq!(server["env"]["CBM_CACHE_DIR"], "/k/graphs/7");
        assert_eq!(server["env"]["CBM_ALLOWED_ROOT"], "/work/7");
    }

    #[test]
    fn a_call_loads_servers_when_it_has_a_repo_server_or_a_graph() {
        assert!(!loads(&call(Reach::default())));
        let graph = Reach {
            code_graph: Some(graph()),
            ..Reach::default()
        };
        assert!(loads(&call(graph)));
        let repo = Reach {
            mcp_servers: [("docs".to_owned(), json!({ "command": "/k/docs" }))].into(),
            ..Reach::default()
        };
        assert!(loads(&call(repo)));
    }
}
