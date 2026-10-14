//! What goose's stdout file says of a call's activity

use std::fs::File;
use std::time::{Duration, UNIX_EPOCH};

use super::tests::{FRESH_ID, World, id};
use super::*;
use crate::ports::{CallActivity, Role, Timestamp};

const URL: &str = "http://192.0.2.9:11434/v1";
const WRITTEN: u64 = 1_790_825_500;

// What goose prints before its model has said anything.
const BANNER: &str = "\n    __( O)>  ● new session · openai qwen3.8:27b\n   \\____)    20261013_1 · /tmp/kelpie-goose\n     L L     goose is ready\n";

const MESSAGE: &str = r#"{"type":"message","message":{"role":"assistant","created":1791931400,"content":[{"type":"text","text":"ok"}]}}"#;

fn call(world: &World) -> AgentCall {
    world.call(URL, Role::Researcher, Session::New(id(FRESH_ID)))
}

fn goose() -> GooseCli {
    ClaudeCli::default().goose()
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
fn a_call_with_no_event_yet_has_shown_nothing() {
    let w = World::new();
    let call = call(&w);
    stdout_holds(&call, BANNER);
    assert_eq!(goose().last_active(&call), CallActivity::Nothing);
    // A `complete` event is the call's end, not the model's doing.
    stdout_holds(&call, format!("{BANNER}{{\"type\":\"complete\"}}\n"));
    assert_eq!(goose().last_active(&call), CallActivity::Nothing);
}

#[test]
fn a_message_or_a_notification_makes_the_stdout_file_s_write_time_the_call_s_last_activity() {
    let w = World::new();
    let call = call(&w);
    for event in [MESSAGE, r#"{"type":"notification","extension_id":"x"}"#] {
        stdout_holds(&call, format!("{BANNER}{event}\n"));
        assert_eq!(
            goose().last_active(&call),
            CallActivity::At(Timestamp(WRITTEN)),
            "{event}"
        );
    }
}

#[test]
fn an_error_counts_as_activity_too() {
    let w = World::new();
    let call = call(&w);
    stdout_holds(
        &call,
        format!("{BANNER}{{\"type\":\"error\",\"error\":\"x\"}}\n"),
    );
    assert_eq!(
        goose().last_active(&call),
        CallActivity::At(Timestamp(WRITTEN))
    );
}

#[test]
fn a_stdout_file_not_made_yet_is_nothing() {
    let w = World::new();
    assert_eq!(goose().last_active(&call(&w)), CallActivity::Nothing);
}

#[test]
fn a_half_written_last_line_is_not_read_as_an_event() {
    let w = World::new();
    let call = call(&w);
    stdout_holds(&call, format!("{BANNER}{MESSAGE}"));
    assert_eq!(goose().last_active(&call), CallActivity::Nothing);
    stdout_holds(&call, format!("{BANNER}{MESSAGE}\n"));
    assert_eq!(
        goose().last_active(&call),
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
        goose().last_active(&call),
        CallActivity::At(Timestamp(WRITTEN))
    );
}

// `shep kelpie watch` finds the stdout file as the root folder's sibling.
#[test]
fn the_transcript_folder_is_the_root_the_stdout_file_is_named_beside() {
    let w = World::new();
    let call = call(&w);
    let folder = goose().transcript(&call).unwrap().folder;
    assert_eq!(
        folder.with_extension("stdout.jsonl"),
        CallFiles::of(&call).stdout
    );
}
