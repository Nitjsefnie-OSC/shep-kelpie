use std::process::{Child, Command, Stdio};

use crate::runner::{Pass, advance};
use crate::test::{CALL_PATIENCE, Hold, Rig, Scripted};

const PI: &str = "---\nrole: implementer\nharness: pi\nmodel: qwen3.8:27b\neffort: low\n\
                  url: http://box:11434/v1\ncontext: 65536\n---\n";

const GOOSE: &str = "---\nrole: implementer\nharness: goose\nmodel: qwen3.8:27b\neffort: low\n\
                     url: http://box:11434/v1\ncontext: 65536\n---\n";

const CODEX: &str = "---\nrole: implementer\nharness: codex\nmodel: gpt-6-sol\neffort: low\n---\n";

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
fn a_handover_waits_while_a_codex_session_is_attached() {
    let (rig, runner, _) = Rig::with_pull_request_set("rotom", |rig| {
        rig.write_agent("codex-box", CODEX);
        rig.implementers(&["codex-box"]);
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
    assert!(why.contains("codex"), "{why}");

    rig.ask(&runner, "detach", Some(&format!("7 {pid}")));
    assert!(runner.lock().unwrap().carry_calls().unwrap().is_empty());
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
        serde_json::json!([{"issue": "7", "role": "pi_session"}])
    );

    rig.ask(&runner, "detach", Some(&format!("7 {pid}")));
    let draining = rig.ask(&runner, "drain", None)["draining"]["calls"].clone();
    assert_eq!(draining, serde_json::json!([]));
}

#[test]
fn a_drain_waits_while_a_goose_session_is_attached() {
    let (rig, runner, _) = Rig::with_pull_request_set("rotom", |rig| {
        rig.write_agent("goose-box", GOOSE);
        rig.implementers(&["goose-box"]);
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
        serde_json::json!([{"issue": "7", "role": "goose_session"}])
    );

    rig.ask(&runner, "detach", Some(&format!("7 {pid}")));
    let draining = rig.ask(&runner, "drain", None)["draining"]["calls"].clone();
    assert_eq!(draining, serde_json::json!([]));
}

#[test]
fn a_drain_waits_while_a_codex_session_is_attached() {
    let (rig, runner, _) = Rig::with_pull_request_set("rotom", |rig| {
        rig.write_agent("codex-box", CODEX);
        rig.implementers(&["codex-box"]);
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
        serde_json::json!([{"issue": "7", "role": "codex_session"}])
    );

    rig.ask(&runner, "detach", Some(&format!("7 {pid}")));
    let draining = rig.ask(&runner, "drain", None)["draining"]["calls"].clone();
    assert_eq!(draining, serde_json::json!([]));
}

// The bridge's socket and the Chromium behind it live in this image, so the next could not take the turn.
#[test]
fn a_handover_waits_while_a_worker_turn_holds_a_browser() {
    let rig = Rig::new("rotom");
    let off = "[app.dogs.kelpie.browser]\nenabled = false";
    rig.edit_settings(|s| {
        assert!(s.contains(off), "the example's browser moved");
        s.replace(off, "[app.dogs.kelpie.browser]\nenabled = true")
    });
    let tools = rig.paths().tools;
    std::fs::create_dir_all(tools.playwright_mcp().parent().unwrap()).unwrap();
    std::fs::write(tools.playwright_mcp(), "").unwrap();
    std::fs::create_dir_all(tools.browsers().join("chromium-1200")).unwrap();
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    let hold = Hold::default();
    rig.claude.script([Scripted::Hold(hold.clone())]);
    assert!(matches!(advance(&runner).unwrap(), Pass::Started));
    assert!(hold.entered(CALL_PATIENCE), "the call never began");

    let why = runner.lock().unwrap().carry_calls().unwrap_err();
    assert!(why.starts_with("#7's turn drives a browser"), "{why}");

    hold.release();
    assert!(hold.answered(CALL_PATIENCE), "the call never answered");
}

// A turn the next image finishes is counted there, from where its transcript stood as it began.
#[test]
fn a_carried_turn_s_requests_are_counted_when_the_next_image_adopts_it() {
    let rig = Rig::new("rotom");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    let hold = Hold::default();
    let lines = include_str!("../../../../../../fixtures/claude-transcript-request-sizes.jsonl");
    rig.claude.script([Scripted::Transcribed(
        lines.to_owned(),
        Box::new(Scripted::Hold(hold.clone())),
    )]);
    assert!(matches!(advance(&runner).unwrap(), Pass::Started));
    assert!(hold.entered(CALL_PATIENCE), "the call never began");

    let turns = runner.lock().unwrap().carry_calls().unwrap();
    assert_eq!(turns.len(), 1);
    runner.lock().unwrap().adopt_calls(turns);

    let total = || {
        let labels = [
            ("harness", "claude-code"),
            ("le", "+Inf"),
            ("role", "worker"),
        ];
        rig.samples.value("kelpie_request_context_tokens", &labels)
    };
    let began = std::time::Instant::now();
    while total() != Some(6.0) && began.elapsed() < CALL_PATIENCE {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(total(), Some(6.0), "the six requests of the transcript");
    let labels = [
        ("after", "start"),
        ("harness", "claude-code"),
        ("role", "worker"),
    ];
    let start = rig
        .samples
        .value("kelpie_cache_write_tokens_total", &labels);
    assert!(start.is_some_and(|written| written > 0.0), "{start:?}");

    hold.release();
}
