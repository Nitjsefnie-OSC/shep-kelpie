//! The MCP servers in a call's MCP config
//!
//! A call that loads any server gets one config file beside its settings file,
//! named with `--mcp-config`. `--strict-mcp-config` keeps every other source
//! out, so the servers here are all it has: the code graph, and those of the
//! repo's plugins and its `.mcp.json`. Headless, a tool nothing allows is
//! refused, and `--tools` names built-in tools only, so each server is also
//! allowed in the settings file.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::ports::AgentCall;
use crate::ports::mcp::{loads, servers};

/// Where the call's MCP config goes, beside its settings file
pub(crate) fn mcp_file(settings: &Path) -> PathBuf {
    settings.with_extension("mcp.json")
}

/// The call's MCP config, where it loads a server
pub(crate) fn config(call: &AgentCall) -> Option<Value> {
    loads(call).then(|| json!({ "mcpServers": servers(call) }))
}

/// Adds each server's tools to the settings' allow-list
pub(crate) fn allow(settings: &mut Value, call: &AgentCall) {
    let rules: Vec<Value> = servers(call)
        .keys()
        .map(|name| Value::from(format!("mcp__{name}")))
        .collect();
    if rules.is_empty() {
        return;
    }
    match settings["permissions"]["allow"].as_array_mut() {
        Some(allow) => allow.extend(rules),
        None => settings["permissions"]["allow"] = Value::Array(rules),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::adapters::claude::{ClaudeCli, argv, sandbox};
    use crate::ports::{Agents, Browser, CodeGraph, Reach, Role, Session, SessionId, Tools};
    use crate::settings::{AgentHarness, Effort};

    fn call(role: Role, tools: Tools, settings: &Path, reach: Reach) -> AgentCall {
        AgentCall {
            issue: crate::ticket::TicketId::from_issue(7),
            settings: settings.to_owned(),
            reach,
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

    fn reach() -> Reach {
        Reach {
            mcp_servers: [
                (
                    "docs".to_owned(),
                    json!({ "type": "http", "url": "https://docs.example.com" }),
                ),
                (
                    "plugin_style_search".to_owned(),
                    json!({ "command": "/k/search" }),
                ),
            ]
            .into(),
            ..Reach::default()
        }
    }

    fn flags(call: &AgentCall) -> Vec<String> {
        argv(call)
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect()
    }

    #[test]
    fn every_kind_of_call_with_a_server_names_the_config_and_an_allow_listed_one_stays_strict() {
        let settings = Path::new("/k/worker/settings.json");
        for (role, tools, strict) in [
            (Role::Worker, Tools::Work, true),
            (Role::Researcher, Tools::Research, true),
            (Role::IssueWriter, Tools::Issues, false),
        ] {
            let flags = flags(&call(role, tools, settings, reach()));
            let at = flags.iter().position(|a| a == "--mcp-config").unwrap();
            assert_eq!(flags[at + 1], mcp_file(settings).display().to_string());
            assert_eq!(flags.contains(&"--strict-mcp-config".to_owned()), strict);
        }
    }

    #[test]
    fn the_issue_writer_with_a_server_keeps_the_servers_it_loads_and_names_its_allow_list() {
        let settings = Path::new("/k/issues/settings.json");
        let flags = flags(&call(Role::IssueWriter, Tools::Issues, settings, reach()));
        let at = flags.iter().position(|a| a == "--mcp-config").unwrap();
        assert_eq!(flags[at + 1], mcp_file(settings).display().to_string());
        assert!(!flags.contains(&"--strict-mcp-config".to_owned()));
        let at = flags.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(flags[at + 1], "Bash,Read,Glob,Grep,Skill");
    }

    #[test]
    fn a_call_with_no_server_names_no_config() {
        let settings = Path::new("/k/worker/settings.json");
        let call = call(Role::Worker, Tools::Work, settings, Reach::default());
        assert!(!flags(&call).contains(&"--mcp-config".to_owned()));
    }

    #[test]
    fn preparing_a_call_writes_each_server_and_the_code_graph_and_allows_their_tools() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("worker/settings.json");
        let mut reach = reach();
        reach.code_graph = Some(CodeGraph {
            binary: "/k/tools/codebase-memory-mcp".into(),
            index: "/k/graphs/7".into(),
            runtime: "/k/cbm/7".into(),
        });
        let call = call(Role::Worker, Tools::Work, &settings, reach);
        ClaudeCli::default().prepare(&call).unwrap();

        let read = |path: &Path| -> Value {
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
        };
        let config = read(&mcp_file(&settings));
        let names: Vec<_> = config["mcpServers"].as_object().unwrap().keys().collect();
        assert_eq!(
            names,
            ["codebase-memory-mcp", "docs", "plugin_style_search"]
        );
        assert_eq!(
            config["mcpServers"]["docs"]["url"],
            "https://docs.example.com"
        );
        assert_eq!(
            read(&settings)["permissions"]["allow"],
            json!([
                "mcp__codebase-memory-mcp",
                "mcp__docs",
                "mcp__plugin_style_search"
            ])
        );
    }

    #[test]
    fn the_sandbox_lets_a_call_read_the_config_that_names_its_servers() {
        let settings = Path::new("/k/worker/settings.json");
        let with = call(Role::Worker, Tools::Work, settings, reach());
        let policy = sandbox::policy(&with, Path::new("/home"), &[]).unwrap();
        assert!(policy.read.contains(&mcp_file(settings)));

        let without = call(Role::Worker, Tools::Work, settings, Reach::default());
        let policy = sandbox::policy(&without, Path::new("/home"), &[]).unwrap();
        assert!(!policy.read.contains(&mcp_file(settings)));
    }

    fn browser() -> Browser {
        Browser {
            kelpie: "/k/bin/shep-kelpie".into(),
            socket: "/k/worker/0123456789abcdef0123456789abcdef.sock".into(),
        }
    }

    #[test]
    fn a_call_with_a_browser_loads_playwright_allows_it_and_connects_the_sandbox_to_its_socket() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("worker/settings.json");
        let reach = Reach {
            browser: Some(Box::new(browser())),
            ..Reach::default()
        };
        let with = call(Role::Worker, Tools::Work, &settings, reach);
        ClaudeCli::default().prepare(&with).unwrap();

        let read = |path: &Path| -> Value {
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
        };
        assert_eq!(
            read(&mcp_file(&settings))["mcpServers"]["playwright"],
            json!({
                "type": "stdio",
                "command": "/k/bin/shep-kelpie",
                "args": ["mcp-connect", "/k/worker/0123456789abcdef0123456789abcdef.sock"],
            })
        );
        assert_eq!(
            read(&settings)["permissions"]["allow"],
            json!(["mcp__playwright"])
        );
        let policy = sandbox::policy(&with, Path::new("/home"), &[]).unwrap();
        assert_eq!(policy.sockets, [browser().socket]);
    }

    #[test]
    fn a_call_without_a_browser_has_no_playwright_no_allowance_and_no_socket() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("worker/settings.json");
        let without = call(Role::Worker, Tools::Work, &settings, reach());
        ClaudeCli::default().prepare(&without).unwrap();

        let read = |path: &Path| -> Value {
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
        };
        assert!(read(&mcp_file(&settings))["mcpServers"]["playwright"].is_null());
        let allowed = read(&settings)["permissions"]["allow"].to_string();
        assert!(!allowed.contains("playwright"), "{allowed}");
        let policy = sandbox::policy(&without, Path::new("/home"), &[]).unwrap();
        assert!(policy.sockets.is_empty());
    }
}
