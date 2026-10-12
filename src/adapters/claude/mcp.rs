//! The MCP servers the repo's plugins bring
//!
//! Every call runs with `--strict-mcp-config`, which loads no server but the
//! ones `--mcp-config` names, and a headless run refuses a tool nothing
//! allows. A plugin's servers are so named in a file beside the call's
//! settings, and allowed there. `--tools` takes built-in tools only, so it
//! is left as it was.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::ports::{AgentCall, AgentError, McpServer};

/// The file a call's servers are named in, beside its settings file
pub(super) fn config_file(call: &AgentCall) -> PathBuf {
    call.settings.with_extension("mcp.json")
}

/// The servers as `--mcp-config` takes them
pub(super) fn config(servers: &[McpServer]) -> Value {
    let servers: serde_json::Map<String, Value> = servers
        .iter()
        .map(|s| (s.name.clone(), s.config.clone()))
        .collect();
    json!({ "mcpServers": servers })
}

/// Allows every tool of the call's servers in its settings
pub(super) fn allow(settings: &mut Value, servers: &[McpServer]) {
    if servers.is_empty() {
        return;
    }
    let permissions = &mut settings["permissions"];
    if !permissions["allow"].is_array() {
        permissions["allow"] = json!([]);
    }
    let allow = permissions["allow"].as_array_mut().expect("just made one");
    allow.extend(servers.iter().map(|s| Value::from(s.tools())));
}

/// Writes the servers' file for the call, if it has servers
pub(super) fn write(call: &AgentCall) -> Result<(), AgentError> {
    if call.mcp_servers.is_empty() {
        return Ok(());
    }
    let file = config_file(call);
    let text = serde_json::to_string_pretty(&config(&call.mcp_servers)).expect("config is JSON");
    write_file(&file, text)
}

fn write_file(file: &Path, text: String) -> Result<(), AgentError> {
    std::fs::write(file, text)
        .map_err(|e| AgentError::Setup(format!("cannot write {}: {}", file.display(), e.kind())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::claude::ClaudeCli;
    use crate::adapters::claude::tests::{call, fresh, strings};
    use crate::ports::{Agents, Role};

    fn docs() -> McpServer {
        McpServer {
            name: "docs".into(),
            config: json!({ "command": "/k/skills/plugins/house/serve.sh", "args": [] }),
        }
    }

    #[test]
    fn a_plugins_server_is_named_in_a_file_beside_the_settings_and_its_tools_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let mut worker = call(Role::Worker, fresh());
        worker.settings = dir.path().join("worker/settings.json");
        worker.mcp_servers = vec![docs()];
        ClaudeCli::default().prepare(&worker).unwrap();

        let file = dir.path().join("worker/settings.mcp.json");
        let named: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(
            named["mcpServers"]["docs"]["command"],
            "/k/skills/plugins/house/serve.sh"
        );
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&worker.settings).unwrap()).unwrap();
        assert_eq!(written["permissions"]["allow"], json!(["mcp__docs"]));

        let argv = strings(&worker);
        let at = argv.iter().position(|a| a == "--mcp-config").unwrap();
        assert_eq!(argv[at + 1], file.display().to_string());
        assert!(argv.iter().any(|a| a == "--strict-mcp-config"));
    }

    #[test]
    fn a_call_with_no_server_has_no_mcp_config() {
        let dir = tempfile::tempdir().unwrap();
        let mut worker = call(Role::Worker, fresh());
        worker.settings = dir.path().join("worker/settings.json");
        ClaudeCli::default().prepare(&worker).unwrap();
        assert!(!dir.path().join("worker/settings.mcp.json").exists());
        assert!(!strings(&worker).iter().any(|a| a == "--mcp-config"));
    }
}
