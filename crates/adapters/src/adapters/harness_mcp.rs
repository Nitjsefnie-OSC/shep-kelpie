//! What pi, Codex and goose each say of the MCP servers a call names

use serde_json::Value;

use super::{Cli, codex::Codex, goose::Goose, pi::Pi};
use crate::ports::HarnessMcp;
use crate::settings::{AgentHarness, Harness};

type Says = fn(&str, &Value) -> Option<String>;

// What one CLI says of a server entry
struct Rules {
    harness: Harness,
    refusal: Says,
    skipped_env: Says,
}

impl Rules {
    const fn of<C: Cli>() -> Self {
        Self {
            harness: C::HARNESS,
            refusal: C::mcp_refusal,
            skipped_env: C::mcp_skipped_env,
        }
    }
}

// A CLI that is not listed loads every server whole.
const RULES: [Rules; 3] = [
    Rules::of::<Pi>(),
    Rules::of::<Codex>(),
    Rules::of::<Goose>(),
];

fn rules(harness: &AgentHarness) -> Option<&'static Rules> {
    RULES
        .iter()
        .find(|rules| rules.harness == harness.harness())
}

/// What each harness's own MCP loading refuses and drops
#[derive(Debug, Clone, Copy, Default)]
pub struct McpRules;

impl HarnessMcp for McpRules {
    fn refusal(&self, harness: &AgentHarness, name: &str, entry: &Value) -> Option<String> {
        (rules(harness)?.refusal)(name, entry)
    }

    fn skipped_env(&self, harness: &AgentHarness, name: &str, entry: &Value) -> Option<String> {
        (rules(harness)?.skipped_env)(name, entry)
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
        let facts = McpRules;

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
        let facts = McpRules;

        let skipped = facts.skipped_env(&AgentHarness::Goose(server()), "skips", &entry);

        assert!(skipped.is_some_and(|said| said.contains("`PATH`")));
        assert_eq!(
            facts.skipped_env(&AgentHarness::Pi(server()), "skips", &entry),
            None
        );
    }
}
