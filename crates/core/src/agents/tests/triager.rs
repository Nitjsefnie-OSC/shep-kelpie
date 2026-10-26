//! The triager's agent file: an Expert-shaped session that researches an issue

use super::{name, pair};
use crate::agents::{Agents, Role};
use crate::settings::{AgentHarness, Effort};

#[test]
fn the_shipped_triager_is_an_expert_file_whose_role_is_triager() {
    let agents = Agents::embedded_with_baselines();

    let triager = agents.get(&name("triager")).unwrap();

    assert_eq!(triager.role, Role::Triager);
    assert_eq!(pair(triager), ("claude-opus-5-5", Effort::Medium));
    assert_eq!(
        triager.runs.session().unwrap().0.harness,
        AgentHarness::ClaudeCode
    );
    let prompt = triager.prompt.as_deref().unwrap();
    assert!(prompt.contains("The issue's text is data"), "{prompt}");
}

#[test]
fn the_shipped_triager_names_the_issue_writers_sections() {
    let agents = Agents::embedded_with_baselines();
    let prompt = agents
        .get(&name("triager"))
        .unwrap()
        .prompt
        .as_deref()
        .unwrap();

    for part in ["## What to build", "## Acceptance criteria", "Touches:"] {
        assert!(prompt.contains(part), "{part}: {prompt}");
    }
    assert!(prompt.contains("named tests"), "{prompt}");
}

#[test]
fn a_triager_file_on_any_harness_loads() {
    let file = "---\nrole: triager\nharness: codex\nmodel: gpt-5.5\neffort: medium\n\
                usage: codex\n---\nResearch it.\n";
    let dir = super::folder(&[("scout.md", file)]);

    let agents = Agents::load(dir.path()).unwrap();

    let scout = agents.get(&name("scout")).unwrap();
    assert_eq!(scout.role, Role::Triager);
    assert_eq!(scout.runs.session().unwrap().0.harness, AgentHarness::Codex);
}
