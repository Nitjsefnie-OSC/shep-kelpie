//! What the triager may ask the person who opened an issue, by who they are,
//! through the runner's stand-ins

use std::sync::Mutex;

use serde_json::{Value, json};

use super::ask_tests::{ASK_WORDS, posted};
use super::pick_tests::{MAINTAINER, issue, list, rig_with};
use super::research_tests::{calls, facts, reply};
use crate::ports::{AuthorAssociation, Role};
use crate::runner::{Runner, step};
use crate::test::{Rig, Scripted};

// A rig with `lines` under `[triage]` and open issue 30 by `author`, whom the
// forge counts as `association`, whose first triager reply is `found`
fn filed_as(
    lines: &str,
    author: &str,
    association: Option<AuthorAssociation>,
    found: Value,
) -> (Rig, Mutex<Runner>) {
    let rig = rig_with(lines);
    let mut crash = issue(&rig, 30, author, 5);
    crash.title = "Crash on start".to_owned();
    crash.body = "It crashes when I run it.".to_owned();
    crash.author_association = association;
    list(&rig, crash);
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(ASK_WORDS)]);
    rig.claude.script_for(Role::Triager, [reply(found)]);
    let runner = rig.open().unwrap();
    (rig, runner)
}

// A reply with `n` open questions, and the one file the checkout holds
fn asking(n: usize) -> Value {
    let open: Vec<Value> = (1..=n)
        .map(|i| json!({ "question": format!("What did you see, {i}?"), "recommended": "a crash" }))
        .collect();
    json!({ "type": "bug", "open": open, "files": ["README.md"] })
}

#[test]
fn the_call_prompt_names_the_authors_association_and_the_role_it_asks_as() {
    let (rig, runner) = filed_as(
        "pick = \"all\"",
        "newcomer",
        Some(AuthorAssociation::FirstTimer),
        asking(1),
    );
    assert_eq!(step(&runner).unwrap(), None);
    let prompt = &calls(&rig, Role::Triager)[0].call.prompt;
    assert!(prompt.contains("`FIRST_TIMER`"), "{prompt}");
    assert!(prompt.contains("a reporter"), "{prompt}");
    assert!(prompt.contains("Ask at most 1 question"), "{prompt}");
}

#[test]
fn the_call_prompt_says_when_the_listing_gave_no_association() {
    let (rig, runner) = filed_as("pick = \"all\"", "newcomer", None, asking(1));
    assert_eq!(step(&runner).unwrap(), None);
    let prompt = &calls(&rig, Role::Triager)[0].call.prompt;
    assert!(prompt.contains("no association"), "{prompt}");
    assert!(prompt.contains("a reporter"), "{prompt}");
}

#[test]
fn a_reply_with_three_questions_for_a_none_author_posts_one() {
    let (rig, runner) = filed_as(
        "pick = \"all\"",
        "newcomer",
        Some(AuthorAssociation::None),
        asking(3),
    );
    assert_eq!(step(&runner).unwrap(), None);
    let [comment] = posted(&rig).try_into().expect("one comment");
    assert!(comment.contains("What did you see, 1?"), "{comment}");
    assert!(!comment.contains("What did you see, 2?"), "{comment}");
    assert!(!comment.contains("What did you see, 3?"), "{comment}");
}

#[test]
fn a_contributor_is_asked_three_of_four_questions() {
    let (rig, runner) = filed_as(
        "pick = \"all\"",
        "regular",
        Some(AuthorAssociation::Contributor),
        asking(4),
    );
    assert_eq!(step(&runner).unwrap(), None);
    let [comment] = posted(&rig).try_into().expect("one comment");
    assert!(comment.contains("What did you see, 3?"), "{comment}");
    assert!(!comment.contains("What did you see, 4?"), "{comment}");
    let prompt = &calls(&rig, Role::Triager)[0].call.prompt;
    assert!(
        prompt.contains("`CONTRIBUTOR`") && prompt.contains("a developer"),
        "{prompt}"
    );
}

#[test]
fn a_listed_login_is_asked_as_its_role_in_place_of_its_association() {
    let (rig, runner) = filed_as(
        "pick = \"all\"\nauthors = { \"QC-Tester\" = \"reporter\" }",
        "qc-tester",
        Some(AuthorAssociation::Collaborator),
        asking(3),
    );
    assert_eq!(step(&runner).unwrap(), None);
    let [comment] = posted(&rig).try_into().expect("one comment");
    assert!(!comment.contains("What did you see, 2?"), "{comment}");
    let prompt = &calls(&rig, Role::Triager)[0].call.prompt;
    assert!(
        prompt.contains("`COLLABORATOR`") && prompt.contains("a reporter"),
        "{prompt}"
    );
}

#[test]
fn the_maintainers_own_issue_is_asked_every_question_as_before() {
    let (rig, runner) = filed_as(
        "pick = \"maintainer\"",
        MAINTAINER,
        Some(AuthorAssociation::Contributor),
        asking(4),
    );
    assert_eq!(step(&runner).unwrap(), None);
    let [comment] = posted(&rig).try_into().expect("one comment");
    assert!(comment.contains("What did you see, 4?"), "{comment}");
    let prompt = &calls(&rig, Role::Triager)[0].call.prompt;
    assert!(prompt.contains("Ask at most 10 questions"), "{prompt}");
    assert!(prompt.contains("the maintainer"), "{prompt}");
}

#[test]
fn text_posted_to_an_author_but_the_maintainer_holds_no_file_path() {
    let (rig, runner) = filed_as(
        "pick = \"all\"",
        "newcomer",
        Some(AuthorAssociation::None),
        asking(1),
    );
    assert_eq!(step(&runner).unwrap(), None);
    let said = facts(&rig);
    assert!(!said.contains("README.md"), "{said}");
    let [comment] = posted(&rig).try_into().expect("one comment");
    assert!(!comment.contains("README.md"), "{comment}");

    let (rig, runner) = filed_as(
        "pick = \"maintainer\"",
        MAINTAINER,
        Some(AuthorAssociation::Owner),
        asking(1),
    );
    assert_eq!(step(&runner).unwrap(), None);
    assert!(facts(&rig).contains("- `README.md`"));
    let [comment] = posted(&rig).try_into().expect("one comment");
    assert!(comment.contains("README.md"), "{comment}");
}
