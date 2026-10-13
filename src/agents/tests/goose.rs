//! goose in an agent file: a researcher on a model server for now, and no
//! other role until the slice that opens it

use super::{folder, limit, model, name, refused};
use crate::agents::{Agents, Role};
use crate::settings::{
    AgentHarness, ContextSize, GatewayName, Harness, Limit, ModelHost, ModelServer,
};

const RESEARCHER: &str = "---\nrole: researcher\nharness: goose\nmodel: qwen3.8:27b\neffort: low\n\
                          gateway: paddock\ncontext: 65536\n---\nVote.\n";

#[test]
fn a_goose_researcher_on_a_gateway_with_a_context_loads_and_reads_no_usage() {
    let dir = folder(&[("r.md", RESEARCHER)]);
    let agents = Agents::load(dir.path()).unwrap();
    let researcher = agents.get(&name("r")).unwrap();
    let paddock = GatewayName::try_from("paddock".to_owned()).unwrap();
    assert_eq!(researcher.role, Role::Researcher);
    assert_eq!(
        model(researcher).harness,
        AgentHarness::Goose(ModelServer {
            host: ModelHost::Gateway(paddock.clone()),
            context: ContextSize::try_from(65_536).unwrap(),
        })
    );
    assert_eq!(model(researcher).harness.harness(), Harness::Goose);
    assert_eq!(*limit(researcher), Limit::Gateway);
    assert_eq!(
        researcher.runs.gateway(),
        Some((&paddock, "qwen3.8:27b")),
        "the pacer and the gateway checks must see a goose agent's gateway"
    );

    let on_a_url = RESEARCHER.replace("gateway: paddock", "url: http://box:11434/v1");
    let dir = folder(&[("u.md", &on_a_url)]);
    let agents = Agents::load(dir.path()).unwrap();
    assert_eq!(*limit(agents.get(&name("u")).unwrap()), Limit::Unqueued);
}

#[test]
fn goose_without_a_model_server_or_a_context_is_refused_naming_both_keys() {
    for text in [
        RESEARCHER.replace("gateway: paddock\n", ""),
        RESEARCHER.replace("context: 65536\n", ""),
        RESEARCHER.replace("gateway: paddock\ncontext: 65536\n", ""),
    ] {
        let err = refused(&[("r.md", &text)]);
        for key in ["runs on goose", "`url`", "`gateway`", "`context`"] {
            assert!(err.contains(key), "{key}: {err}");
        }
    }
}

#[test]
fn goose_takes_no_tools_and_no_usage_but_none() {
    let implementer = RESEARCHER.replace("researcher", "implementer");
    let err = refused(&[(
        "t.md",
        &implementer.replace("---\nVote", "tools: Bash\n---\nVote"),
    )]);
    assert!(
        err.contains("runs on goose, which takes no `tools`"),
        "{err}"
    );

    for usage in ["claude", "codex"] {
        let text = RESEARCHER.replace("---\nVote", &format!("usage: {usage}\n---\nVote"));
        let err = refused(&[("u.md", &text)]);
        assert!(
            err.contains("runs on goose, whose usage is read with `none`")
                && err.contains(&format!("`usage: {usage}`")),
            "{err}"
        );
    }
    let none = RESEARCHER.replace("---\nVote", "usage: none\n---\nVote");
    let dir = folder(&[("n.md", &none)]);
    assert!(Agents::load(dir.path()).is_ok());
}

#[test]
fn an_implementer_a_reviewer_or_an_advisor_on_goose_is_refused_naming_the_slice_that_opens_it() {
    let reviewer = RESEARCHER.replace("researcher", "reviewer");
    for (role, text, slice) in [
        (
            "an implementer",
            RESEARCHER.replace("researcher", "implementer"),
            "#788",
        ),
        ("a reviewer", reviewer, "#789"),
        (
            "an advisor",
            RESEARCHER.replace("researcher", "advisor"),
            "#789",
        ),
    ] {
        let err = refused(&[("g.md", &text)]);
        assert!(
            err.contains(&format!("{role} cannot run on goose yet")) && err.contains(slice),
            "{role}: {err}"
        );
    }
    for role in ["issue-writer", "pm"] {
        let text = format!(
            "---\nrole: {role}\nharness: goose\nmodel: qwen3.8:27b\neffort: low\n---\nGo.\n"
        );
        let err = refused(&[("g.md", &text)]);
        assert!(
            err.contains("runs on claude-code, not goose"),
            "{role}: {err}"
        );
    }
}
