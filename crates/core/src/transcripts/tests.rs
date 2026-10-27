use std::path::Path;

use super::*;

fn write(path: &Path, lines: &[String]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, lines.join("\n")).unwrap();
}

#[test]
fn a_codex_rollout_is_found_by_its_thread_at_any_depth_under_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let sessions = dir.path().join("sessions");
    write(
        &sessions.join("2026/10/21/rollout-2026-10-21T15-40-45-01a0f7fd.jsonl"),
        &[],
    );
    write(
        &sessions.join("2026/10/21/rollout-2026-10-21T15-41-00-other.jsonl"),
        &[],
    );

    let found = codex_rollout(&sessions, "01a0f7fd").unwrap();
    assert!(found.ends_with("rollout-2026-10-21T15-40-45-01a0f7fd.jsonl"));
    assert_eq!(codex_rollout(&sessions, "absent"), None);
}

#[test]
fn a_codex_session_s_rollout_is_the_one_its_thread_record_names() {
    let dir = tempfile::tempdir().unwrap();
    let sessions = dir.path().join("sessions");
    let threads = dir.path().join("threads");
    write(
        &sessions.join("2026/10/21/rollout-2026-10-21T15-40-45-aaa.jsonl"),
        &[],
    );
    write(
        &sessions.join("2026/10/21/rollout-2026-10-21T15-41-00-bbb.jsonl"),
        &[],
    );
    write(&threads.join("w7.json"), &[r#"{"id":"aaa"}"#.to_owned()]);

    let found = find(
        Format::Codex,
        "w7",
        &sessions,
        Some(&threads),
        jiff::Timestamp::now(),
    );
    assert!(
        found
            .unwrap()
            .ends_with("rollout-2026-10-21T15-40-45-aaa.jsonl")
    );
}
