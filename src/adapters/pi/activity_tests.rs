//! What pi's stdout file says of a call's activity

use std::fs::File;
use std::time::{Duration, UNIX_EPOCH};

use super::tests::{FRESH_ID, World, id};
use super::*;
use crate::ports::{Role, Timestamp};

const URL: &str = "http://192.0.2.9:11434/v1";
const WRITTEN: u64 = 1_790_825_500;

// The lines pi writes at once, before its model has said anything.
const OPENING: &str = concat!(
    r#"{"type":"session","version":3,"id":"0b6f3c1e-7d2a-4f4e-9a51-3c8d2e6f1a07","cwd":"/wt"}"#,
    "\n",
    r#"{"type":"agent_start"}"#,
    "\n",
    r#"{"type":"turn_start"}"#,
    "\n",
    r#"{"type":"message_start","message":{"role":"user","content":[{"type":"text","text":"Say ok."}]}}"#,
    "\n",
    r#"{"type":"message_end","message":{"role":"user","content":[{"type":"text","text":"Say ok."}]}}"#,
    "\n",
    r#"{"type":"message_start","message":{"role":"assistant","content":[]}}"#,
    "\n",
    r#"{"type":"message_end","message":{"role":"assistant","content":[]}}"#,
    "\n",
);

fn call(world: &World) -> AgentCall {
    world.call(URL, Role::Reviewer, Session::New(id(FRESH_ID)))
}

fn pi() -> PiCli {
    ClaudeCli::default().pi()
}

// Writes `text` as the call's stdout file, last written at `WRITTEN`.
fn stdout_holds(call: &AgentCall, text: impl AsRef<[u8]>) {
    let path = CallFiles::of(call).stdout;
    std::fs::write(&path, text).unwrap();
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(WRITTEN))
        .unwrap();
}

#[test]
fn a_call_with_only_its_opening_events_has_shown_nothing() {
    let w = World::new();
    let call = call(&w);
    stdout_holds(&call, OPENING);
    assert_eq!(pi().last_active(&call), CallActivity::Nothing);
}

#[test]
fn a_message_update_makes_the_stdout_file_s_write_time_the_call_s_last_activity() {
    let w = World::new();
    let call = call(&w);
    let update = r#"{"type":"message_update","assistantMessageEvent":{"type":"thinking_delta"}}"#;
    stdout_holds(&call, format!("{OPENING}{update}\n"));
    assert_eq!(
        pi().last_active(&call),
        CallActivity::At(Timestamp(WRITTEN))
    );
}

#[test]
fn a_tool_execution_update_a_compaction_and_a_retry_count_as_activity_too() {
    let w = World::new();
    let call = call(&w);
    for kind in [
        "tool_execution_start",
        "tool_execution_update",
        "tool_execution_end",
        "compaction_start",
        "compaction_end",
        "auto_retry_start",
        "auto_retry_end",
    ] {
        stdout_holds(&call, format!("{OPENING}{{\"type\":\"{kind}\"}}\n"));
        assert_eq!(
            pi().last_active(&call),
            CallActivity::At(Timestamp(WRITTEN)),
            "{kind}"
        );
    }
}

#[test]
fn an_assistant_message_that_ends_with_something_said_counts_as_activity() {
    let w = World::new();
    let call = call(&w);
    let said = r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#;
    stdout_holds(&call, format!("{OPENING}{said}\n"));
    assert_eq!(
        pi().last_active(&call),
        CallActivity::At(Timestamp(WRITTEN))
    );
}

#[test]
fn a_stdout_file_not_made_yet_is_nothing() {
    let w = World::new();
    assert_eq!(pi().last_active(&call(&w)), CallActivity::Nothing);
}

#[test]
fn a_session_file_written_while_the_stdout_file_is_empty_is_not_activity() {
    let w = World::new();
    let call = call(&w);
    let sessions = Files::of(&call).sessions();
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(
        sessions.join(format!("2026-10-01_{FRESH_ID}.jsonl")),
        "{}\n",
    )
    .unwrap();
    stdout_holds(&call, "");
    assert_eq!(pi().last_active(&call), CallActivity::Nothing);
}

#[test]
fn a_half_written_last_line_is_not_read_as_an_event() {
    let w = World::new();
    let call = call(&w);
    stdout_holds(&call, format!("{OPENING}{{\"type\":\"message_update\"}}"));
    assert_eq!(pi().last_active(&call), CallActivity::Nothing);
    stdout_holds(&call, format!("{OPENING}{{\"type\":\"message_update\"}}\n"));
    assert_eq!(
        pi().last_active(&call),
        CallActivity::At(Timestamp(WRITTEN))
    );
}

#[test]
fn a_stdout_file_past_a_megabyte_is_activity_without_being_read_through() {
    let w = World::new();
    let call = call(&w);
    // Not events at all, so only a file that goes unread can count.
    stdout_holds(&call, vec![b'x'; 1_048_577]);
    assert_eq!(
        pi().last_active(&call),
        CallActivity::At(Timestamp(WRITTEN))
    );
}
