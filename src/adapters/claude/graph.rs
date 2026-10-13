//! The code graph server in a call's MCP config

use std::path::Path;

use serde_json::{Value, json};

use crate::ports::CodeGraph;

/// The name Claude Code knows the server by, which its tools are named after
pub(crate) const SERVER: &str = "codebase-memory-mcp";

/// The code graph server as Claude Code's `mcpServers` entry: it starts over `graph`'s index, held to the call's folder
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::adapters::claude::mcp::mcp_file;
    use crate::adapters::claude::{ClaudeCli, argv, sandbox};
    use crate::ports::{AgentCall, Agents, Reach, Role, Session, SessionId, Tools};
    use crate::settings::{AgentHarness, Effort};

    fn graph() -> CodeGraph {
        CodeGraph {
            binary: "/kelpie/tools/bin/codebase-memory-mcp".into(),
            index: "/kelpie/code-graphs/7".into(),
        }
    }

    fn call(role: Role, tools: Tools, settings: &Path, code_graph: Option<CodeGraph>) -> AgentCall {
        AgentCall {
            issue: 7,
            settings: settings.to_owned(),
            reach: Reach {
                code_graph,
                ..Reach::default()
            },
            ..AgentCall::new(
                role,
                tools,
                AgentHarness::ClaudeCode,
                "claude-sonnet-5",
                Effort::Medium,
                Session::New(SessionId("7e812e8a-3bf1-42a7-bddf-0ab283b7372a".into())),
                PathBuf::from("/work/7"),
                "go",
            )
        }
    }

    fn flags(call: &AgentCall) -> Vec<String> {
        argv(call)
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect()
    }

    #[test]
    fn a_worker_and_a_reviewer_with_a_graph_load_the_server_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        for (role, tools) in [(Role::Worker, Tools::Work), (Role::Reviewer, Tools::Review)] {
            let settings = dir.path().join(format!("{role:?}.json"));
            let call = call(role, tools, &settings, Some(graph()));
            let flags = flags(&call);
            let at = flags.iter().position(|a| a == "--mcp-config").unwrap();
            assert_eq!(flags[at + 1], mcp_file(&settings).display().to_string());
            assert!(
                flags.contains(&"--strict-mcp-config".to_owned()),
                "{flags:?}"
            );
            assert!(
                at < flags
                    .iter()
                    .position(|a| a == "--strict-mcp-config")
                    .unwrap()
            );
        }
    }

    #[test]
    fn a_call_without_a_graph_names_no_mcp_config() {
        let call = call(Role::Worker, Tools::Work, Path::new("/k/s.json"), None);
        assert!(!flags(&call).contains(&"--mcp-config".to_owned()));
    }

    #[test]
    fn preparing_a_call_with_a_graph_writes_its_config_and_allows_its_tools() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("worker/review-settings.json");
        let call = call(Role::Reviewer, Tools::Review, &settings, Some(graph()));
        ClaudeCli::default().prepare(&call).unwrap();

        let read = |path: &Path| -> Value {
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
        };
        let written = read(&settings);
        assert_eq!(
            written["permissions"]["allow"],
            json!(["mcp__codebase-memory-mcp"])
        );
        let config = read(&mcp_file(&settings));
        assert_eq!(
            config["mcpServers"]["codebase-memory-mcp"]["env"]["CBM_CACHE_DIR"],
            "/kelpie/code-graphs/7"
        );
    }

    #[test]
    fn preparing_a_call_without_a_graph_writes_no_config_and_allows_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("worker/settings.json");
        let call = call(Role::Reviewer, Tools::Review, &settings, None);
        ClaudeCli::default().prepare(&call).unwrap();

        assert!(!mcp_file(&settings).exists());
        let written = std::fs::read_to_string(&settings).unwrap();
        assert!(!written.contains("mcp__"), "{written}");
    }

    #[test]
    fn the_sandbox_lets_the_server_read_its_binary_and_write_only_its_index() {
        let settings = Path::new("/k/worker/settings.json");
        let with = call(Role::Worker, Tools::Work, settings, Some(graph()));
        let policy = sandbox::policy(&with, Path::new("/home"), &[]).unwrap();
        let graph = graph();
        for path in [&graph.binary, &graph.index, &mcp_file(settings)] {
            assert!(policy.read.contains(path), "{path:?}");
        }
        assert!(policy.write.contains(&graph.index));
        assert!(!policy.write.contains(&graph.binary));

        let without = call(Role::Worker, Tools::Work, settings, None);
        let policy = sandbox::policy(&without, Path::new("/home"), &[]).unwrap();
        assert!(!policy.read.contains(&graph.binary));
        assert!(!policy.write.contains(&graph.index));
    }

    #[test]
    fn the_entry_starts_the_server_on_the_calls_index_and_folder() {
        let server = entry(&graph(), Path::new("/work/7"));
        assert_eq!(server["command"], "/kelpie/tools/bin/codebase-memory-mcp");
        assert_eq!(server["env"]["CBM_CACHE_DIR"], "/kelpie/code-graphs/7");
        assert_eq!(server["env"]["CBM_ALLOWED_ROOT"], "/work/7");
    }
}
