//! What kelpie writes for the advisor and the researcher to read, and the
//! rulings it keeps for the researcher

use std::path::Path;

use super::{parked_with, script_all, seen, settle, state_of};
use crate::ports::Role;
use crate::runner::step;
use crate::test::{Rig, ticket, write_in};

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Where the rig's test puts the maintainer's sessions
fn sessions(rig: &Rig) -> std::path::PathBuf {
    rig.paths().kelpie_home.join("my-sessions")
}

#[test]
fn the_researcher_is_given_the_history_the_threads_the_workers_session_and_the_maintainers() {
    let (rig, runner) = parked_with("koji", false, |rig| {
        let folder = sessions(rig);
        rig.edit_settings(|s| {
            s + &format!(
                "\n[app.dogs.kelpie.researcher]\ntranscripts = [\"{}\"]\n",
                folder.display()
            )
        });
    });
    let paths = rig.paths();
    // An earlier ruling and its answer, as kelpie kept them when it was answered.
    write_in(
        &paths.folder,
        "rulings.jsonl",
        "{\"id\":\"30\",\"at\":1790000000,\"issue\":\"3\",\"pull_request\":30,\
         \"asked\":\"Merge this pull request at abc1234 into main?\",\"answer\":\"yes\"}\n\
         {\"id\":\"40\",\"at\":1790000100,\"issue\":\"4\",\"pull_request\":40,\
         \"asked\":\"Retry the failed turn?\",\"answer\":\"yes\",\"by_pm\":true}\n",
    );
    rig.writes_on(71, "reviewer", "THREAD-MARK please add a test");
    // The maintainer's own session in the checkout, and in another project.
    let repo = rig.repo().display().to_string();
    let session = |cwd: &str, said: &str| {
        format!(
            "{{\"type\":\"user\",\"cwd\":\"{cwd}\",\"message\":{{\"role\":\"user\",\"content\":\"{said}\"}},\
             \"timestamp\":\"2026-10-10T12:00:01.000Z\"}}\n"
        )
    };
    write_in(
        &sessions(&rig),
        "mine.jsonl",
        &session(&repo, "MAINT-MARK always merge on green"),
    );
    write_in(
        &sessions(&rig),
        "other.jsonl",
        &session("/elsewhere", "OTHER-MARK"),
    );
    // The worker's own session, where the stand-in says its transcripts are.
    let state = state_of(&rig);
    let id = state["work_items"][0]["session"]
        .as_str()
        .unwrap()
        .to_owned();
    let folder = paths.worker.join("vote-settings-7.transcripts");
    write_in(
        &folder,
        &format!("{id}.jsonl"),
        &session(&repo, "WORKER-SESSION-MARK start on the hook"),
    );
    script_all(&rig, false);

    settle(&rig, &runner);

    let researcher = &paths.researcher;
    assert!(read(&researcher.join("ruling.md")).contains("Merge this pull request"));
    let history = read(&researcher.join("rulings.md"));
    assert!(
        history.contains("Ruling 30 (issue #3, pull request #30)"),
        "{history}"
    );
    assert!(
        history.contains("Answered by the maintainer: yes"),
        "{history}"
    );
    assert!(
        history.contains("Answered by the project manager, not the maintainer: yes"),
        "{history}"
    );
    let pull_request = read(&researcher.join("threads/pull-request-71.md"));
    assert!(
        pull_request.contains("THREAD-MARK please add a test"),
        "{pull_request}"
    );
    assert!(read(&researcher.join("threads/issue-7.md")).contains("Title of #7"));
    let worker = read(&researcher.join("worker.md"));
    assert!(
        worker.contains("WORKER-SESSION-MARK start on the hook"),
        "{worker}"
    );
    let theirs = read(&researcher.join("maintainer/01-claude-code.md"));
    assert!(
        theirs.contains("MAINT-MARK always merge on green"),
        "{theirs}"
    );
    assert!(!researcher.join("maintainer/02-claude-code.md").exists());
    let prompt = &seen(&rig, Role::Researcher)[0].call.prompt;
    for name in ["`threads/`", "`worker.md`", "`maintainer/`", "`rulings.md`"] {
        assert!(prompt.contains(name), "{name}: {prompt}");
    }
}

#[test]
fn the_advisor_is_given_the_ruling_the_branchs_diff_and_where_to_read() {
    let (rig, runner) = parked_with("rotom", false, |_| {});
    script_all(&rig, false);

    settle(&rig, &runner);

    let advisor = &rig.paths().advisor;
    assert!(read(&advisor.join("ruling.md")).contains("| **A** | Merge |"));
    assert!(read(&advisor.join("diff.patch")).contains("+++ b/work.txt"));
    let places = read(&advisor.join("where.md"));
    let worktree = rig.paths().worktree(ticket(7)).display().to_string();
    assert!(
        places.contains(&format!("`{worktree}`, on branch `kelpie/7`")),
        "{places}"
    );
    assert!(
        places.contains(&rig.repo().display().to_string()),
        "{places}"
    );
    let prompt = &seen(&rig, Role::Advisor)[0].call.prompt;
    assert!(prompt.contains(&worktree), "{prompt}");
}

#[test]
fn an_answered_ruling_is_kept_for_the_researcher_with_its_answer() {
    let (rig, runner) = parked_with("pikachu", false, |_| {});
    rig.ask(&runner, "rule", Some("71 no add a test first"));
    step(&runner).unwrap();

    let kept = read(&rig.paths().rulings);
    let record: serde_json::Value = serde_json::from_str(kept.lines().next().unwrap()).unwrap();
    assert_eq!(record["id"], "71");
    assert_eq!(record["issue"], "7");
    assert_eq!(record["pull_request"], 71);
    assert_eq!(record["answer"], "no: add a test first");
    assert!(
        record["asked"]
            .as_str()
            .unwrap()
            .starts_with("Merge this pull request at "),
        "{record}"
    );
}
