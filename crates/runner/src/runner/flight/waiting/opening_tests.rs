//! Opening labelled work items against paddock's shared model turns

use serde_json::{Value, json};
use std::sync::Arc;

use super::{BEHIND_PADDOCK, agent_labels, events, item};
use crate::ports::{LocalLeases, PaddockHolder};
use crate::test::{Rig, StandInEndpoint};
use shep_kelpie_core::forwarder::Upstream;

fn status() -> Value {
    json!({
        "models": [{"model": "coder", "turns": {"lease": 2, "request": 1}}],
        "leases": [
            {"id": "L1", "model": "coder", "client": "other-runner", "revoked": null, "reclaimable": false},
            {"id": "L2", "model": "coder", "client": "maintainer", "revoked": null, "reclaimable": false}
        ],
        "waiters": [{"model": "coder", "kind": "lease", "client": "third-runner"}]
    })
}

// A real HTTP boundary keeps the runner's selection and the adapter's status reading together.
fn rig(server: &StandInEndpoint, listed: &[&str]) -> Rig {
    at(server.host(), listed)
}

fn at(url: &str, listed: &[&str]) -> Rig {
    let rig = Rig::new("acme");
    rig.write_agent("mine", BEHIND_PADDOCK);
    rig.implementers(listed);
    rig.edit_settings(|s| {
        s.replace(
            "\nimplementers = ",
            "\nfallback_after = 15\nimplementers = ",
        )
    });
    let settings = std::fs::read_to_string(rig.kelpie_settings_file()).unwrap();
    let gateway = format!(
        "[gateways.paddock]\nurl = {:?}\nkey_env = \"CARGO_PKG_NAME\"\n",
        url
    );
    rig.set_kelpie_settings(&format!("{settings}{gateway}"));
    rig
}

struct Queued(bool);

impl LocalLeases for Queued {
    fn paddock_holder(&self, _: &Upstream, _: &str) -> Option<PaddockHolder> {
        None
    }

    fn paddock_queued(&self, _: &Upstream, model: &str) -> bool {
        self.0 && model == "coder"
    }
}

#[test]
fn a_queued_gateway_moves_a_new_item_before_any_worker_turn_starts() {
    let rig = at("http://127.0.0.1:9", &["mine", "sonnet-high"]);
    let runner = rig.open().unwrap();
    runner.lock().unwrap().ports.local_leases = Arc::new(Queued(true));
    rig.forge.label(7, "agent:mine");

    rig.ask(&runner, "add", Some("7"));

    assert_eq!(item(&rig, &runner, 7)["agent"], "sonnet-high");
    assert_eq!(agent_labels(&rig, 7), ["agent:sonnet-high"]);
    assert_eq!(
        events(&runner)
            .into_iter()
            .filter(|event| event.contains("moved"))
            .collect::<Vec<_>>(),
        [
            "#7: its `agent:mine` label moved to `agent:sonnet-high`, since mine's model already has a wait"
        ]
    );
}

#[test]
fn a_free_gateway_turn_keeps_the_labelled_implementer() {
    let rig = at("http://127.0.0.1:9", &["mine", "sonnet-high"]);
    let runner = rig.open().unwrap();
    runner.lock().unwrap().ports.local_leases = Arc::new(Queued(false));
    rig.forge.label(7, "agent:mine");

    rig.ask(&runner, "add", Some("7"));

    assert_eq!(item(&rig, &runner, 7)["agent"], "mine");
    assert_eq!(agent_labels(&rig, 7), ["agent:mine"]);
    assert!(events(&runner).iter().all(|event| !event.contains("moved")));
}

#[test]
fn a_pinned_label_stays_on_a_queued_gateway() {
    let rig = at("http://127.0.0.1:9", &["mine", "sonnet-high"]);
    let runner = rig.open().unwrap();
    runner.lock().unwrap().ports.local_leases = Arc::new(Queued(true));
    rig.forge.label(7, "agent:mine!");

    rig.ask(&runner, "add", Some("7"));

    assert_eq!(item(&rig, &runner, 7)["agent"], "mine");
    assert_eq!(agent_labels(&rig, 7), ["agent:mine!"]);
    assert!(events(&runner).iter().all(|event| !event.contains("moved")));
}

#[test]
fn a_queued_gateway_with_no_free_implementer_keeps_its_label() {
    let rig = at("http://127.0.0.1:9", &["mine"]);
    let runner = rig.open().unwrap();
    runner.lock().unwrap().ports.local_leases = Arc::new(Queued(true));
    rig.forge.label(7, "agent:mine");

    rig.ask(&runner, "add", Some("7"));

    assert_eq!(item(&rig, &runner, 7)["agent"], "mine");
    assert_eq!(agent_labels(&rig, 7), ["agent:mine"]);
}

#[test]
fn another_implementer_on_the_same_queued_model_is_skipped() {
    let rig = at("http://127.0.0.1:9", &["mine", "also-mine", "sonnet-high"]);
    rig.write_agent("also-mine", BEHIND_PADDOCK);
    let runner = rig.open().unwrap();
    runner.lock().unwrap().ports.local_leases = Arc::new(Queued(true));
    rig.forge.label(7, "agent:mine");

    rig.ask(&runner, "add", Some("7"));

    assert_eq!(item(&rig, &runner, 7)["agent"], "sonnet-high");
    assert_eq!(agent_labels(&rig, 7), ["agent:sonnet-high"]);
}

#[test]
fn a_failed_gateway_status_read_keeps_the_labelled_implementer() {
    let rig = at("http://127.0.0.1:9", &["mine", "sonnet-high"]);
    let runner = rig.open().unwrap();
    rig.forge.label(7, "agent:mine");

    rig.ask(&runner, "add", Some("7"));

    assert_eq!(item(&rig, &runner, 7)["agent"], "mine");
    assert_eq!(agent_labels(&rig, 7), ["agent:mine"]);
}

#[test]
fn a_queued_gateway_does_not_override_fallback_after_off() {
    let rig = at("http://127.0.0.1:9", &["mine", "sonnet-high"]);
    rig.edit_settings(|s| s.replace("fallback_after = 15\n", ""));
    let runner = rig.open().unwrap();
    runner.lock().unwrap().ports.local_leases = Arc::new(Queued(true));
    rig.forge.label(7, "agent:mine");

    rig.ask(&runner, "add", Some("7"));

    assert_eq!(item(&rig, &runner, 7)["agent"], "mine");
    assert_eq!(agent_labels(&rig, 7), ["agent:mine"]);
}

#[test]
fn an_openai_gateway_has_no_paddock_turn_limit() {
    let rig = at("http://127.0.0.1:9", &["mine", "sonnet-high"]);
    let settings = std::fs::read_to_string(rig.kelpie_settings_file()).unwrap();
    rig.set_kelpie_settings(&settings.replace(
        "[gateways.paddock]\n",
        "[gateways.paddock]\nkind = \"openai\"\n",
    ));
    let runner = rig.open().unwrap();
    runner.lock().unwrap().ports.local_leases = Arc::new(Queued(true));
    rig.forge.label(7, "agent:mine");

    rig.ask(&runner, "add", Some("7"));

    assert_eq!(item(&rig, &runner, 7)["agent"], "mine");
    assert_eq!(agent_labels(&rig, 7), ["agent:mine"]);
}

#[test]
fn gateway_status_with_a_free_lease_turn_keeps_the_labelled_implementer() {
    let mut status = status();
    status["leases"].as_array_mut().unwrap().pop();
    let server = StandInEndpoint::start([])
        .like_paddock(env!("CARGO_PKG_NAME"), &["coder"])
        .with_paddock_status(&status.to_string());
    let rig = rig(&server, &["mine", "sonnet-high"]);
    let runner = rig.open().unwrap();
    rig.forge.label(7, "agent:mine");

    rig.ask(&runner, "add", Some("7"));

    assert_eq!(item(&rig, &runner, 7)["agent"], "mine");
    assert_eq!(agent_labels(&rig, 7), ["agent:mine"]);
}

#[test]
fn a_model_full_of_other_clients_leases_moves_a_new_item_and_records_both_implementers() {
    let server = StandInEndpoint::start([])
        .like_paddock(env!("CARGO_PKG_NAME"), &["coder"])
        .with_paddock_status(&status().to_string());
    let rig = rig(&server, &["mine", "sonnet-high"]);
    let runner = rig.open().unwrap();
    rig.forge.label(7, "agent:mine");

    rig.ask(&runner, "add", Some("7"));

    assert_eq!(item(&rig, &runner, 7)["agent"], "sonnet-high");
    assert_eq!(agent_labels(&rig, 7), ["agent:sonnet-high"]);
    let moved: Vec<_> = events(&runner)
        .into_iter()
        .filter(|event| event.contains("moved"))
        .collect();
    assert_eq!(moved.len(), 1, "{moved:?}");
    assert!(moved[0].contains("agent:mine"), "{}", moved[0]);
    assert!(moved[0].contains("agent:sonnet-high"), "{}", moved[0]);
}
