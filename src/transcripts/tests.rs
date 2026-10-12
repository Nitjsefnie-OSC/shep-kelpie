use std::path::Path;

use super::*;

fn write(path: &Path, lines: &[String]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, lines.join("\n")).unwrap();
}

fn claude(cwd: &str, asked: &str, said: &str) -> Vec<String> {
    vec![
        format!(
            r#"{{"type":"user","cwd":"{cwd}","message":{{"role":"user","content":"{asked}"}},"timestamp":"2026-10-10T12:00:01.000Z"}}"#
        ),
        format!(
            r#"{{"type":"assistant","cwd":"{cwd}","message":{{"role":"assistant","content":[{{"type":"text","text":"{said}"}},{{"type":"tool_use","name":"Bash","input":{{"command":"ls"}}}}]}},"timestamp":"2026-10-10T12:00:02.000Z"}}"#
        ),
    ]
}

fn codex(cwd: &str, asked: &str) -> Vec<String> {
    vec![
        format!(
            r#"{{"timestamp":"2026-10-09T09:00:00.000Z","type":"session_meta","payload":{{"id":"t1","cwd":"{cwd}"}}}}"#
        ),
        format!(
            r#"{{"timestamp":"2026-10-09T09:00:01.000Z","type":"response_item","payload":{{"type":"message","role":"user","content":[{{"type":"input_text","text":"{asked}"}}]}}}}"#
        ),
    ]
}

fn pi(cwd: &str, asked: &str) -> Vec<String> {
    vec![
        format!(
            r#"{{"type":"session","version":3,"id":"s1","timestamp":"2026-10-08T08:00:00.000Z","cwd":"{cwd}"}}"#
        ),
        format!(
            r#"{{"type":"message","timestamp":"2026-10-08T08:00:01.000Z","message":{{"role":"user","content":[{{"type":"text","text":"{asked}"}}]}}}}"#
        ),
    ]
}

#[test]
fn each_harnesss_sessions_for_the_project_are_read_through_the_one_table() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let project = "/work/koji";
    write(
        &home.join(".claude/projects/-work-koji/a.jsonl"),
        &claude(project, "Should merges wait for CI?", "Yes, always."),
    );
    write(
        &home.join(".codex/sessions/2026/10/09/rollout-1-t1.jsonl"),
        &codex(project, "Rename the flag"),
    );
    write(
        &home.join(".pi/agent/sessions/--work-koji--/1_s1.jsonl"),
        &pi(project, "Use ollama here"),
    );

    let found = sessions_in(
        &[
            home.join(".claude/projects"),
            home.join(".codex/sessions"),
            home.join(".pi/agent/sessions"),
        ],
        Path::new(project),
    );

    let by_harness: Vec<&str> = found.iter().map(|f| f.harness).collect();
    assert_eq!(
        by_harness
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        ["claude-code", "codex", "pi"].into()
    );
    let all: String = found.iter().map(|f| f.text.as_str()).collect();
    assert!(all.contains("asked: Should merges wait for CI?"), "{all}");
    assert!(all.contains("said: Yes, always."), "{all}");
    assert!(all.contains("asked: Rename the flag"), "{all}");
    assert!(all.contains("asked: Use ollama here"), "{all}");
    assert!(!all.contains("called"), "a tool call is not kept: {all}");
}

#[test]
fn a_session_that_ran_in_another_folder_is_left_out() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("mine.jsonl"),
        &claude("/work/koji/worktrees/7", "in a worktree", "ok"),
    );
    write(
        &dir.path().join("theirs.jsonl"),
        &claude("/work/kojima", "elsewhere", "ok"),
    );
    write(&dir.path().join("none.jsonl"), &["not json".to_owned()]);

    let found = sessions_in(&[dir.path().to_owned()], Path::new("/work/koji"));

    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(found[0].text.contains("in a worktree"));
}

#[test]
fn a_folder_that_is_not_there_holds_no_sessions() {
    let found = sessions_in(&[PathBuf::from("/nonexistent/sessions")], Path::new("/w"));
    assert!(found.is_empty());
}

#[test]
fn the_newest_sessions_come_first_and_only_so_many_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    for n in 0..(MOST_SESSIONS + 3) {
        let path = dir.path().join(format!("s{n}.jsonl"));
        write(&path, &claude("/w", &format!("session {n}"), "ok"));
        let age = std::time::Duration::from_secs(1_000 - n as u64);
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(SystemTime::UNIX_EPOCH + age).unwrap();
    }

    let found = sessions_in(&[dir.path().to_owned()], Path::new("/w"));

    assert_eq!(found.len(), MOST_SESSIONS);
    assert!(found[0].text.contains("session 0"), "{}", found[0].text);
}

#[test]
fn a_long_session_keeps_its_end_from_a_whole_message() {
    let lines: Vec<String> = (0..2_000)
        .map(|n| {
            format!(
                r#"{{"type":"user","cwd":"/w","message":{{"role":"user","content":"message number {n} {}"}},"timestamp":"2026-10-10T12:00:01.000Z"}}"#,
                "x".repeat(100)
            )
        })
        .collect();

    let text = render(Format::Claude, &lines.join("\n"), Keep::Talk, 4_000);

    assert!(
        text.starts_with(
            "(earlier messages left out)\n\n[2026-10-10 12:00] asked: message number "
        )
    );
    assert!(text.contains("message number 1999"), "the end is kept");
    assert!(!text.contains("message number 0 "), "the start is not");
    assert!(text.len() < 4_200, "{}", text.len());
}

#[test]
fn a_work_rendering_adds_the_tools_a_session_called() {
    let lines = claude("/w", "go", "running it").join("\n");
    let talk = render(Format::Claude, &lines, Keep::Talk, 10_000);
    let work = render(Format::Claude, &lines, Keep::Work, 10_000);
    assert!(!talk.contains("called"));
    assert!(work.contains("called: Bash  ls"), "{work}");
}
