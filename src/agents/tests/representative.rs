//! The representative's agent file: a session on any harness, with the role,
//! model and effort a file gives it

use super::{folder, name, pair};
use crate::agents::{Agents, Role};
use crate::settings::{AgentHarness, Effort, Harness};

#[test]
fn a_representative_file_loads_with_its_role_harness_model_and_effort() {
    let file = "---\nrole: representative\nharness: codex\nmodel: gpt-5.5\neffort: low\n\
                usage: codex\n---\nWord it.\n";
    let dir = folder(&[("herald.md", file)]);

    let agents = Agents::load(dir.path()).unwrap();

    let herald = agents.get(&name("herald")).unwrap();
    assert_eq!(herald.role, Role::Representative);
    assert_eq!(pair(herald), ("gpt-5.5", Effort::Low));
    assert_eq!(
        herald.runs.session().unwrap().0.harness,
        AgentHarness::Codex
    );
    assert_eq!(herald.prompt.as_deref(), Some("Word it."));
}

#[test]
fn the_shipped_representative_is_opus_at_medium_effort_on_claude_code() {
    let agents = Agents::embedded();

    let representative = agents.get(&name("representative")).unwrap();

    assert_eq!(representative.role, Role::Representative);
    assert_eq!(pair(representative), ("claude-opus-5-5", Effort::Medium));
    assert_eq!(
        representative.runs.session().unwrap().0.harness,
        AgentHarness::ClaudeCode
    );
    let prompt = representative.prompt.as_deref().unwrap();
    assert!(
        prompt.contains("kelpie, the maintainer's sheepdog"),
        "{prompt}"
    );
    assert!(prompt.contains("you never promise one"), "{prompt}");
}

#[test]
fn a_representative_file_on_pi_loads() {
    let file = "---\nrole: representative\nharness: pi\nmodel: qwen3.8:27b\neffort: low\n\
                url: http://box:11434/v1\ncontext: 65536\nusage: none\n---\nWord it.\n";
    let dir = folder(&[("local-herald.md", file)]);

    let agents = Agents::load(dir.path()).unwrap();

    let herald = agents.get(&name("local-herald")).unwrap();
    assert_eq!(herald.role, Role::Representative);
    assert_eq!(
        herald.runs.session().unwrap().0.harness.harness(),
        Harness::Pi
    );
}
