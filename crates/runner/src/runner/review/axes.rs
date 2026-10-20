//! A project that lists the `spec` reviewer, through the runner and its stand-ins

use crate::ports::{CLEAN, Role};
use crate::runner::step;
use crate::test::{Rig, Scripted};

#[test]
fn a_listed_spec_reviewer_gets_the_issue_s_criteria_after_its_body() {
    let rig = Rig::new("shep");
    rig.reviewers(&["spec"]);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    step(&runner).unwrap(); // the worker's first turn
    rig.claude.script([Scripted::Text(CLEAN)]);
    step(&runner).unwrap(); // round 1, spec: clean

    let seen = rig.claude.all_seen();
    let reviews: Vec<_> = seen
        .iter()
        .filter(|s| s.call.role == Role::Reviewer)
        .collect();
    let [spec] = reviews.as_slice() else {
        panic!("one spec read: {reviews:#?}");
    };
    let prompt = &spec.call.prompt;
    let (body, criteria) = prompt
        .split_once("\n--- end ---\n\n")
        .expect("the criteria follow the body's last line");
    assert!(body.starts_with("You are checking"), "{prompt}");
    assert!(
        criteria.starts_with("The issue this pull request resolves asks for the following."),
        "{prompt}"
    );
    assert!(
        criteria.ends_with("#7 Title of #7\n\nBody of #7."),
        "{prompt}"
    );
}
