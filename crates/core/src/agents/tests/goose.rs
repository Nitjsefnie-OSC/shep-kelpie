//! goose in an agent file: a reviewer and an implementer on a model server

use super::{folder, limit, model, name, refused};
use crate::agents::{Agents, Role};
use crate::settings::{
    AgentHarness, ContextSize, GatewayName, Harness, Limit, ModelHost, ModelServer,
};

const REVIEWER: &str = "---\nrole: reviewer\nharness: goose\nmodel: qwen3.8:27b\neffort: low\n\
                        gateway: paddock\ncontext: 65536\n---\nReview.\n";

#[test]
fn goose_without_a_model_server_or_a_context_is_refused_naming_both_keys() {
    for text in [
        REVIEWER.replace("gateway: paddock\n", ""),
        REVIEWER.replace("context: 65536\n", ""),
        REVIEWER.replace("gateway: paddock\ncontext: 65536\n", ""),
    ] {
        let err = refused(&[("r.md", &text)]);
        for key in ["runs on goose", "`url`", "`gateway`", "`context`"] {
            assert!(err.contains(key), "{key}: {err}");
        }
    }
}

#[test]
fn goose_takes_no_tools_and_no_usage_but_none() {
    let implementer = REVIEWER.replace("reviewer", "implementer");
    let err = refused(&[(
        "t.md",
        &implementer.replace("---\nReview", "tools: Bash\n---\nReview"),
    )]);
    assert!(
        err.contains("runs on goose, which takes no `tools`"),
        "{err}"
    );

    for usage in ["claude", "codex"] {
        let text = REVIEWER.replace("---\nReview", &format!("usage: {usage}\n---\nReview"));
        let err = refused(&[("u.md", &text)]);
        assert!(
            err.contains("runs on goose, whose usage is read with `none`")
                && err.contains(&format!("`usage: {usage}`")),
            "{err}"
        );
    }
    let none = REVIEWER.replace("---\nReview", "usage: none\n---\nReview");
    let dir = folder(&[("n.md", &none)]);
    assert!(Agents::load(dir.path()).is_ok());
}

#[test]
fn an_implementer_on_goose_loads() {
    let implementer = REVIEWER.replace("reviewer", "implementer");
    let dir = folder(&[("i.md", &implementer)]);
    let agents = Agents::load(dir.path()).unwrap();
    let worker = agents.get(&name("i")).unwrap();
    assert_eq!(worker.role, Role::Implementer);
    assert_eq!(model(worker).harness.harness(), Harness::Goose);
}

#[test]
fn a_goose_reviewer_on_a_gateway_with_a_context_loads_and_reads_no_usage() {
    let dir = folder(&[("r.md", REVIEWER)]);
    let agents = Agents::load(dir.path()).unwrap();
    let reviewer = agents.get(&name("r")).unwrap();
    let paddock = GatewayName::try_from("paddock".to_owned()).unwrap();
    assert_eq!(reviewer.role, Role::Reviewer);
    assert_eq!(
        model(reviewer).harness,
        AgentHarness::Goose(ModelServer {
            host: ModelHost::Gateway(paddock.clone()),
            context: ContextSize::try_from(65_536).unwrap(),
        })
    );
    assert_eq!(model(reviewer).harness.harness(), Harness::Goose);
    assert_eq!(*limit(reviewer), Limit::Gateway);
    assert_eq!(
        reviewer.runs.gateway(),
        Some((&paddock, "qwen3.8:27b")),
        "the pacer and the gateway checks must see a goose agent's gateway"
    );

    let on_a_url = REVIEWER.replace("gateway: paddock", "url: http://box:11434/v1");
    let dir = folder(&[("u.md", &on_a_url)]);
    let agents = Agents::load(dir.path()).unwrap();
    assert_eq!(*limit(agents.get(&name("u")).unwrap()), Limit::Unqueued);
}

#[test]
fn a_pm_still_runs_on_claude_code_alone() {
    let text = "---\nrole: pm\nharness: goose\nmodel: qwen3.8:27b\neffort: low\n---\nGo.\n";
    let err = refused(&[("g.md", text)]);
    assert!(err.contains("runs on claude-code, not goose"), "{err}");
}
