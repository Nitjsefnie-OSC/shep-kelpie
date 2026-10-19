// goose calls as a handover takes them: which are carried, and with what.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::gateway_tests::{KEY, gateways, on_gateway};
use super::tests::{FRESH, FRESH_ID, URL, World, id};
use super::*;
use crate::adapters::process::record::Record;
use crate::ports::{KeptServer, Role};
use crate::test::{OpenSandbox, StandInEndpoint};

// goose as a script that waits for a `go` file before it prints `FRESH`.
fn waiting_goose(world: &World, gateways: Gateways) -> GooseCli {
    std::fs::write(world.path("stdout.jsonl"), FRESH).unwrap();
    let script = format!(
        "#!/bin/sh\ntouch {}\nwhile [ ! -f {} ]; do sleep 0.02; done\ncat {}\n",
        world.path("started").display(),
        world.path("go").display(),
        world.path("stdout.jsonl").display()
    );
    crate::test::write_script(&world.path("goose"), &script);
    ClaudeCli::default()
        .sandboxed(Arc::new(OpenSandbox::default()), world.path("home"))
        .goose()
        .with_program(world.path("goose"))
        .with_gateways(gateways)
}

// What the carry of `call` is while it runs, then lets it end.
fn carried_while_running(
    goose: &GooseCli,
    world: &World,
    call: &AgentCall,
) -> Result<Adopted, String> {
    let carried = std::thread::scope(|scope| {
        let running = scope.spawn(|| goose.run(call, &Ending::default()));
        let until = Instant::now() + Duration::from_secs(10);
        let carried = loop {
            let carried = goose.adoption(call);
            if carried.is_ok() || Instant::now() > until {
                break carried;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        std::fs::write(world.path("go"), "").unwrap();
        running.join().unwrap().unwrap();
        carried
    });
    let gone = goose.adoption(call).unwrap_err();
    assert!(
        gone.contains("cannot be read"),
        "a call that has ended is carried no more: {gone}"
    );
    carried
}

#[test]
fn a_call_that_is_not_a_workers_refuses_naming_why() {
    let w = World::new();
    let goose = waiting_goose(&w, Gateways::default());
    let call = w.call(URL, Role::Researcher, Session::New(id(FRESH_ID)));
    assert_eq!(
        goose.adoption(&call).unwrap_err(),
        "only a worker's call is carried"
    );
}

#[test]
fn a_workers_call_with_no_record_refuses_naming_the_record() {
    let w = World::new();
    let goose = waiting_goose(&w, Gateways::default());
    let call = w.call(URL, Role::Worker, Session::New(id(FRESH_ID)));
    let why = goose.adoption(&call).unwrap_err();
    assert!(
        why.starts_with("the call's record ") && why.contains("cannot be read"),
        "{why}"
    );
}

#[test]
fn a_call_with_no_note_refuses_naming_why() {
    let w = World::new();
    let goose = waiting_goose(&w, Gateways::default());
    let call = w.call(URL, Role::Worker, Session::New(id(FRESH_ID)));
    // A record another image wrote, for a call this image never noted.
    let files = CallFiles::of(&call);
    let record = Record {
        pid: std::process::id(),
        pgid: std::process::id(),
        role: Role::Worker,
        issue: crate::ticket::TicketId::from_issue(7),
        stdout: files.stdout,
        stderr: files.stderr,
        ending: false,
        flying: None,
    };
    std::fs::write(&files.record, serde_json::to_string(&record).unwrap()).unwrap();

    assert_eq!(
        goose.adoption(&call).unwrap_err(),
        "this image holds no forwarder for the call"
    );
}

#[test]
fn a_workers_turn_on_a_gateway_is_carried_with_its_forwarder_and_lease_and_not_the_key() {
    let server = StandInEndpoint::start([]).like_paddock(KEY, &["qwen3.8:27b"]);
    let w = World::new();
    let goose = waiting_goose(
        &w,
        gateways(server.host(), crate::settings::GatewayKind::Paddock),
    );
    let call = on_gateway(&w, Role::Worker);
    let adopted = carried_while_running(&goose, &w, &call).unwrap();

    let kept = adopted.kept.as_ref().expect("the forwarder is carried");
    assert_eq!(kept.forwarder.parent(), call.settings.parent());
    assert_eq!(kept.forwarder.extension().unwrap(), "sock");
    assert_eq!(kept.server, KeptServer::Gateway("paddock".into()));
    assert_eq!(kept.session.0, FRESH_ID);
    assert_eq!(kept.lease.as_deref(), Some("L1"));
    assert_eq!(adopted.harness, Harness::Goose);
    let wire = serde_json::to_string(&adopted).unwrap();
    assert!(!wire.contains(KEY), "{wire}");
}

#[test]
fn a_workers_turn_on_a_model_with_no_gateway_is_carried_with_its_forwarder_alone() {
    let w = World::new();
    let goose = waiting_goose(&w, Gateways::default());
    let call = w.call(URL, Role::Worker, Session::New(id(FRESH_ID)));
    let adopted = carried_while_running(&goose, &w, &call).unwrap();

    let kept = adopted.kept.as_ref().expect("the forwarder is carried");
    assert_eq!(kept.server, KeptServer::Url(URL.into()));
    assert_eq!(kept.lease, None);
    assert_eq!(adopted.harness, Harness::Goose);
}
