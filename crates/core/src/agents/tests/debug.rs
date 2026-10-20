//! What a failing assert prints for the agents

use super::{folder, name};
use crate::agents::Agents;

#[test]
fn agents_debug_names_each_agent_and_leaves_out_its_prompt() {
    let marker = "PROMPT-BODY-MARKER";
    let body = format!("{marker} {}", "x".repeat(4000));
    let file = format!(
        "---\nrole: implementer\nharness: claude-code\nmodel: mine-model\neffort: low\n---\n{body}\n"
    );
    let dir = folder(&[("mine.md", &file)]);
    let agents = Agents::load(dir.path()).unwrap();

    let printed = format!("{agents:?}");

    for (agent, _) in agents.iter() {
        assert!(printed.contains(agent.as_str()), "{agent}");
    }
    assert!(printed.contains("claude-code"), "its harness");
    assert!(printed.contains("mine-model"), "its model");
    assert!(!printed.contains(marker), "no prompt text");
    let length = agents
        .get(&name("mine"))
        .unwrap()
        .prompt
        .as_deref()
        .unwrap()
        .len();
    assert!(printed.contains(&format!("{length} bytes")), "its length");
    assert!(printed.len() < 4000, "{} bytes", printed.len());
}
