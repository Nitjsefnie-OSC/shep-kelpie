//! An issue another open issue or pull request already covers, which kelpie
//! closes as a duplicate naming it, through the runner's stand-ins

use std::sync::Mutex;

use serde_json::{Value, json};

use super::comment::MARKER;
use super::pick_tests::{issue, list, rig_with};
use super::research_tests::{WORDS, calls, crash, facts, moves, reply, triage, untouched};
use crate::ports::{IssueCloseReason, Role};
use crate::runner::{Runner, step};
use crate::test::Scripted;

#[test]
fn the_pull_requests_and_issues_reach_the_call_fenced_and_without_the_issue_itself() {
    let (rig, runner) = untouched();
    for (number, title) in [(12, "Add retries"), (13, "Crash\non exit")] {
        let mut other = issue(&rig, number, "someone", 0);
        other.title = title.to_owned();
        list(&rig, other);
    }
    rig.forge
        .list_open_pull(41, "Fix the crash on start", &[30]);
    rig.forge.list_open_pull(42, "Rework the docs", &[]);
    rig.claude.script_for(Role::Triager, [reply(json!({}))]);

    assert_eq!(step(&runner).unwrap(), None);

    let asked = calls(&rig, Role::Triager);
    let prompt = &asked[0].call.prompt;
    let fenced_issues = "UNTRUSTED-0 begin\n#12 Add retries\n#13 Crash on exit\nUNTRUSTED-0 end";
    assert!(prompt.contains(fenced_issues), "{prompt}");
    let fenced_pulls = "UNTRUSTED-0 begin\n#41 Fix the crash on start (closes #30)\n\
                        #42 Rework the docs\nUNTRUSTED-0 end";
    assert!(prompt.contains(fenced_pulls), "{prompt}");
    assert!(
        !prompt.contains("#30 Crash on start"),
        "the issue is not listed against itself: {prompt}"
    );
}
