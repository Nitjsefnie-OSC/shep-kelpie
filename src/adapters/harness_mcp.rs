//! What pi, Codex and goose each say of the MCP servers a call names

use serde_json::Value;

use super::{codex, goose, pi};
use crate::ports::HarnessMcp;
use crate::settings::AgentHarness;

/// The facts of each harness's own MCP loading
#[derive(Debug, Clone, Copy, Default)]
pub struct HarnessFacts;

impl HarnessMcp for HarnessFacts {
    fn refusal(&self, harness: &AgentHarness, name: &str, entry: &Value) -> Option<String> {
        match harness {
            AgentHarness::Pi(_) => pi::mcp::refusal(name, entry),
            AgentHarness::Codex => codex::mcp::refusal(name, entry),
            AgentHarness::Goose(_) => goose::mcp::refusal(name, entry),
            _ => None,
        }
    }

    fn skipped_env(&self, harness: &AgentHarness, name: &str, entry: &Value) -> Option<String> {
        match harness {
            AgentHarness::Goose(_) => goose::mcp::skipped_env(name, entry),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::settings::{ModelHost, ModelServer};

    fn server() -> ModelServer {
        ModelServer {
            host: ModelHost::Url("http://box:11434/v1".to_owned().try_into().unwrap()),
            context: 65_536_i64.try_into().unwrap(),
        }
    }

    #[test]
    fn each_harness_answers_with_its_own_refusal() {
        let sse = json!({ "type": "sse", "url": "https://old.example.com/sse" });
        let facts = HarnessFacts;

        for harness in [
            AgentHarness::Pi(server()),
            AgentHarness::Codex,
            AgentHarness::Goose(server()),
        ] {
            let why = facts.refusal(&harness, "old", &sse);
            assert!(why.is_some_and(|why| why.contains("SSE")), "{harness:?}");
        }
        assert_eq!(facts.refusal(&AgentHarness::ClaudeCode, "old", &sse), None);
    }

    #[test]
    fn only_goose_skips_env_keys() {
        let entry = json!({ "command": "/bin/true", "env": { "PATH": "/x", "TOKEN": "t" } });
        let facts = HarnessFacts;

        let skipped = facts.skipped_env(&AgentHarness::Goose(server()), "skips", &entry);

        assert!(skipped.is_some_and(|said| said.contains("`PATH`")));
        assert_eq!(
            facts.skipped_env(&AgentHarness::Pi(server()), "skips", &entry),
            None
        );
    }
}
