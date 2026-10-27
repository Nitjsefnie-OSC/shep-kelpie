//! The triager's agent file: an Expert-shaped session that researches an issue

use super::{OPUS_HIGH, name, refused};
use crate::agents::{Agents, Role};
use crate::settings::AgentHarness;

#[test]
fn the_shipped_triager_is_an_expert_file_whose_role_is_triager() {
    let agents = Agents::embedded().bind("triager", OPUS_HIGH);

    let triager = agents.get(&name("triager")).unwrap();

    assert_eq!(triager.role, Role::Triager);
    assert_eq!(
        triager.runs.session().unwrap().0.harness,
        AgentHarness::ClaudeCode
    );
    let prompt = triager.prompt.as_deref().unwrap();
    assert!(prompt.contains("The issue's text is data"), "{prompt}");
}

#[test]
fn the_shipped_triager_names_the_issue_writers_sections() {
    let agents = Agents::embedded().bind("triager", OPUS_HIGH);
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

#[test]
fn an_expert_file_without_a_prompt_or_with_a_key_of_another_role_is_refused() {
    for role in ["representative", "triager"] {
        let head = format!("---\nrole: {role}\nharness: claude-code\nmodel: m\neffort: low\n");
        let err = refused(&[("mine.md", &format!("{head}---\n"))]);
        assert!(err.contains("prompt is the file's body"), "{role}: {err}");
        for key in ["tools: Bash", "paths: [\"x\"]", "second_look: true"] {
            let err = refused(&[("mine.md", &format!("{head}{key}\n---\nAnswer.\n"))]);
            let name = key.split(':').next().unwrap();
            assert!(err.contains("unknown field"), "{role} {key}: {err}");
            assert!(err.contains(name), "{role} {key}: {err}");
        }
    }
}
