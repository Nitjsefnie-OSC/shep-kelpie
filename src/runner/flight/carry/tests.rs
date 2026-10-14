use std::process::{Child, Command, Stdio};

use crate::test::Rig;

const PI: &str = "---\nrole: implementer\nharness: pi\nmodel: qwen3.8:27b\neffort: low\n\
                  url: http://box:11434/v1\ncontext: 65536\n---\n";

const GOOSE: &str = "---\nrole: implementer\nharness: goose\nmodel: qwen3.8:27b\neffort: low\n\
                     url: http://box:11434/v1\ncontext: 65536\n---\n";

// A process that lives until dropped, standing in for `shep kelpie attach`
struct Live(Child);

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn a_handover_waits_while_a_pi_session_is_attached() {
    let (rig, runner, _) = Rig::with_pull_request_set("rotom", |rig| {
        rig.write_agent("pi-box", PI);
        rig.implementers(&["pi-box"]);
    });
    assert!(runner.lock().unwrap().carry_calls().is_ok());
    let child = Command::new("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let me = Live(child);
    let pid = me.0.id();
    let attached = rig.ask(&runner, "attach", Some(&format!("7 {pid}")));
    assert_eq!(attached["attach"], "ready", "{attached}");

    let why = runner.lock().unwrap().carry_calls().unwrap_err();
    assert!(why.starts_with("#7 is attached"), "{why}");

    rig.ask(&runner, "detach", Some(&format!("7 {pid}")));
    assert!(runner.lock().unwrap().carry_calls().unwrap().is_empty());
    assert_eq!(
        rig.ask(&runner, "status", None)["work_item"].get("attached"),
        None
    );
}

#[test]
fn a_handover_waits_while_a_goose_session_is_attached() {
    let (rig, runner, _) = Rig::with_pull_request_set("rotom", |rig| {
        rig.write_agent("goose-box", GOOSE);
        rig.implementers(&["goose-box"]);
    });
    assert!(runner.lock().unwrap().carry_calls().is_ok());
    let child = Command::new("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let me = Live(child);
    let pid = me.0.id();
    let attached = rig.ask(&runner, "attach", Some(&format!("7 {pid}")));
    assert_eq!(attached["attach"], "ready", "{attached}");

    let why = runner.lock().unwrap().carry_calls().unwrap_err();
    assert!(why.starts_with("#7 is attached"), "{why}");
    assert!(why.contains("goose"), "{why}");

    rig.ask(&runner, "detach", Some(&format!("7 {pid}")));
    assert!(runner.lock().unwrap().carry_calls().unwrap().is_empty());
    assert_eq!(
        rig.ask(&runner, "status", None)["work_item"].get("attached"),
        None
    );
}

#[test]
fn a_drain_waits_while_a_pi_session_is_attached() {
    let (rig, runner, _) = Rig::with_pull_request_set("rotom", |rig| {
        rig.write_agent("pi-box", PI);
        rig.implementers(&["pi-box"]);
    });
    let child = Command::new("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let me = Live(child);
    let pid = me.0.id();
    rig.ask(&runner, "attach", Some(&format!("7 {pid}")));

    let draining = rig.ask(&runner, "drain", None)["draining"]["calls"].clone();
    assert_eq!(
        draining,
        serde_json::json!([{"issue": 7, "role": "pi_session"}])
    );

    rig.ask(&runner, "detach", Some(&format!("7 {pid}")));
    let draining = rig.ask(&runner, "drain", None)["draining"]["calls"].clone();
    assert_eq!(draining, serde_json::json!([]));
}
