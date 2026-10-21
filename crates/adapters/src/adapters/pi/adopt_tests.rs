// pi calls as a handover takes them: which are carried, and with what.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::gateway_tests::{KEY, gateways, on_gateway};
use super::tests::{FRESH, FRESH_ID, World, id};
use super::*;
use crate::ports::{KeptServer, Role};
use crate::test::{OpenSandbox, StandInEndpoint};

// pi as a script that waits for a `go` file before it prints `FRESH`.
fn waiting_pi(world: &World, gateways: Gateways) -> PiCli {
    std::fs::write(world.path("stdout.jsonl"), FRESH).unwrap();
    let script = format!(
        "#!/bin/sh\ntouch {}\n{}\ncat {}\n",
        world.path("started").display(),
        crate::test::waits_for(&world.path("go"), crate::test::CALL_PATIENCE),
        world.path("stdout.jsonl").display()
    );
    crate::test::write_script(&world.path("pi"), &script);
    PiCli::on(&Base::default().sandboxed(Arc::new(OpenSandbox::default()), world.path("home")))
        .with_program(world.path("pi"))
        .with_gateways(gateways)
}

// What the carry of `call` is while it runs, then lets it end.
fn carried_while_running(pi: &PiCli, world: &World, call: &AgentCall) -> Result<Adopted, String> {
    pi.prepare(call).unwrap();
    let carried = std::thread::scope(|scope| {
        let running = scope.spawn(|| pi.run(call, &Ending::default()));
        let until = Instant::now() + Duration::from_secs(10);
        let carried = loop {
            let carried = pi.adoption(call);
            if carried.is_ok() || Instant::now() > until {
                break carried;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        std::fs::write(world.path("go"), "").unwrap();
        running.join().unwrap().unwrap();
        carried
    });
    let gone = pi.adoption(call).unwrap_err();
    assert!(
        gone.contains("cannot be read"),
        "a call that has ended is carried no more: {gone}"
    );
    carried
}

#[test]
fn a_pi_call_that_is_not_a_workers_refuses_the_handover_naming_why() {
    let w = World::new();
    let pi = waiting_pi(&w, Gateways::default());
    let call = w.call(
        "http://192.0.2.9:11434/v1",
        Role::Reviewer,
        Session::New(id(FRESH_ID)),
    );
    assert_eq!(
        pi.adoption(&call).unwrap_err(),
        "only a worker's call is carried"
    );
}

#[test]
fn a_workers_pi_call_with_no_record_refuses_the_handover_naming_the_record() {
    let w = World::new();
    let pi = waiting_pi(&w, Gateways::default());
    let call = w.call(
        "http://192.0.2.9:11434/v1",
        Role::Worker,
        Session::New(id(FRESH_ID)),
    );
    let why = pi.adoption(&call).unwrap_err();
    assert!(
        why.starts_with("the call's record ") && why.contains("cannot be read"),
        "{why}"
    );
}

#[test]
fn a_workers_turn_on_a_gateway_is_carried_with_its_forwarder_and_lease_and_not_the_key() {
    let server = StandInEndpoint::start([]).like_paddock(KEY, &["qwen3.8:27b"]);
    let w = World::new();
    let pi = waiting_pi(
        &w,
        gateways(server.host(), crate::settings::GatewayKind::Paddock),
    );
    let call = on_gateway(&w, Role::Worker);
    let adopted = carried_while_running(&pi, &w, &call).unwrap();

    let kept = adopted.kept.as_ref().expect("the forwarder is carried");
    assert_eq!(kept.forwarder.parent(), call.settings.parent());
    assert_eq!(kept.forwarder.extension().unwrap(), "sock");
    assert_eq!(kept.server, KeptServer::Gateway("paddock".into()));
    assert_eq!(kept.session.0, FRESH_ID);
    assert_eq!(kept.lease.as_deref(), Some("L1"));
    assert_eq!(adopted.harness, Harness::Pi);
    let wire = serde_json::to_string(&adopted).unwrap();
    assert!(!wire.contains(KEY), "{wire}");
}

#[test]
fn a_workers_turn_on_a_model_with_no_gateway_is_carried_with_its_forwarder_alone() {
    let w = World::new();
    let pi = waiting_pi(&w, Gateways::default());
    let call = w.call(
        "http://192.0.2.9:11434/v1",
        Role::Worker,
        Session::New(id(FRESH_ID)),
    );
    let adopted = carried_while_running(&pi, &w, &call).unwrap();

    let kept = adopted.kept.as_ref().expect("the forwarder is carried");
    assert_eq!(
        kept.server,
        KeptServer::Url("http://192.0.2.9:11434/v1".into())
    );
    assert_eq!(kept.lease, None);
    assert_eq!(adopted.harness, Harness::Pi);
}
