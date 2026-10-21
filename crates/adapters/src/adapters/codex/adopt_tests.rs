// Codex calls as a handover takes them: which are carried, and with what.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::adopt::settings_of;
use super::tests::{FRESH, WORKER_ID, World, id};
use super::*;
use crate::ports::Role;
use crate::test::OpenSandbox;

// Codex as a script that waits for a `go` file before it prints `FRESH`.
fn waiting_codex(world: &World) -> CodexCli {
    std::fs::write(world.path("stdout.jsonl"), FRESH).unwrap();
    let script = format!(
        "#!/bin/sh\ntouch {}\n{}\ncat {}\n",
        world.path("started").display(),
        crate::test::waits_for(&world.path("go"), crate::test::CALL_PATIENCE),
        world.path("stdout.jsonl").display()
    );
    crate::test::write_script(&world.path("codex"), &script);
    CodexCli::on(
        &Base::default().sandboxed(Arc::new(OpenSandbox::default()), world.path("home")),
        world.path("login"),
    )
    .with_program(world.path("codex"))
}

#[test]
fn a_codex_call_that_is_not_a_workers_refuses_the_handover_naming_why() {
    let w = World::new();
    let codex = waiting_codex(&w);
    let call = w.call(Role::Reviewer, Session::New(id(WORKER_ID)));
    assert_eq!(
        codex.adoption(&call).unwrap_err(),
        "only a worker's call is carried"
    );
}

#[test]
fn a_workers_codex_call_with_no_record_refuses_the_handover_naming_the_record() {
    let w = World::new();
    let codex = waiting_codex(&w);
    let call = w.call(Role::Worker, Session::New(id(WORKER_ID)));
    let why = codex.adoption(&call).unwrap_err();
    assert!(
        why.starts_with("the call's record ") && why.contains("cannot be read"),
        "{why}"
    );
}

#[test]
fn a_workers_turn_is_carried_with_its_session_and_ends_when_the_call_does() {
    let w = World::new();
    let codex = waiting_codex(&w);
    let call = w.call(Role::Worker, Session::New(id(WORKER_ID)));
    codex.prepare(&call).unwrap();
    let carried = std::thread::scope(|scope| {
        let running = scope.spawn(|| codex.run(&call, &Ending::default()));
        let until = Instant::now() + Duration::from_secs(10);
        let carried = loop {
            let carried = codex.adoption(&call);
            if carried.is_ok() || Instant::now() > until {
                break carried;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        std::fs::write(w.path("go"), "").unwrap();
        running.join().unwrap().unwrap();
        carried
    });

    let adopted = carried.unwrap();
    assert_eq!(adopted.harness, Harness::Codex);
    assert_eq!(adopted.session, Some(id(WORKER_ID)));
    assert_eq!(adopted.kept, None);
    let gone = codex.adoption(&call).unwrap_err();
    assert!(
        gone.contains("cannot be read"),
        "a call that has ended is carried no more: {gone}"
    );
}

#[test]
fn the_files_of_a_carried_call_are_found_again_from_its_record() {
    let w = World::new();
    let call = w.call(Role::Worker, Session::New(id(WORKER_ID)));
    let record = CallFiles::of(&call).record;
    assert_eq!(settings_of(&record), call.settings);
    let (found, made) = (Files::at(&settings_of(&record)), Files::of(&call));
    assert_eq!(found.threads, made.threads);
    assert_eq!(found.home, made.home);
    assert_eq!(found.stdout, made.stdout);
}
