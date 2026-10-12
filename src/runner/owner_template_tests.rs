//! A repo with no template of its own gets its owner's `.github` template

use std::fs;
use std::sync::Mutex;

use super::{Runner, TemplateSource, step};
use crate::ports::{Cost, Usage};
use crate::test::{Rig, Scripted};

// The rig's project is `shep-pm/shep`, so its owner's repository is this
const OWNERS: &str = "shep-pm/.github";

// The instructions file the worker's first turn was started with
fn first_instructions(rig: &Rig, runner: &Mutex<Runner>) -> String {
    rig.ask(runner, "add", Some("7"));
    let usage = Usage {
        input: 1,
        cache_write: 10,
        cache_write_5m: 0,
        cache_read: 100,
        output: 1000,
    };
    rig.claude.script([Scripted::Reply(usage, Cost(1))]);
    step(runner).unwrap();
    let [seen] = rig.claude.seen().try_into().unwrap();
    let file = seen.call.instructions.expect("the worker has instructions");
    fs::read_to_string(file).unwrap()
}

fn instructions_with_owners_template(text: &str) -> String {
    let rig = Rig::new("koji");
    rig.forge
        .put_file(OWNERS, ".github/pull_request_template.md", text);
    let runner = rig.open().unwrap();
    first_instructions(&rig, &runner)
}

#[test]
fn a_repo_without_a_template_gets_its_owners_text_in_the_instructions() {
    let text = instructions_with_owners_template("## Summary\n\n- what changed\n");
    assert!(
        text.contains(
            "The repo has no pull request template of its own, so its owner's applies, \
             `.github/pull_request_template.md` of `shep-pm/.github`. Fill it in for your \
             pull request's body instead of writing your own, and still end the body with \
             `Resolves #<issue>`. Its text:\n\n<template>\n## Summary\n\n- what changed\n\
             </template>\n"
        ),
        "{text}"
    );
    assert!(!text.contains("/mattpocock:pr"), "{text}");
}

#[test]
fn the_repos_own_template_wins_over_its_owners() {
    let rig = Rig::new("koji");
    rig.forge
        .put_file(OWNERS, "pull_request_template.md", "Owner's words\n");
    rig.land("PULL_REQUEST_TEMPLATE.md", "Repo's words\n");
    let runner = rig.open().unwrap();
    let text = first_instructions(&rig, &runner);
    assert!(
        text.contains("pull request template, `PULL_REQUEST_TEMPLATE.md`"),
        "{text}"
    );
    assert!(!text.contains("Owner's words"), "{text}");
    assert!(!text.contains("`shep-pm/.github`"), "{text}");
}

#[test]
fn a_folder_of_the_owners_templates_reaches_the_worker_by_name() {
    let rig = Rig::new("koji");
    let folder = "docs/PULL_REQUEST_TEMPLATE";
    rig.forge
        .put_file(OWNERS, &format!("{folder}/fix.md"), "## Fix\n");
    rig.forge
        .put_file(OWNERS, &format!("{folder}/feature.md"), "## Feature\n");
    let runner = rig.open().unwrap();
    let text = first_instructions(&rig, &runner);
    assert!(
        text.contains(
            "The repo has no pull request templates of its own, so its owner's apply, in \
             `docs/PULL_REQUEST_TEMPLATE` of `shep-pm/.github`. Fill in the one that fits for \
             your pull request's body instead of writing your own, and still end the body \
             with `Resolves #<issue>`. Each follows by name:\n\n\
             <template name=\"feature.md\">\n## Feature\n</template>\n\
             <template name=\"fix.md\">\n## Fix\n</template>\n"
        ),
        "{text}"
    );
}

#[test]
fn an_owner_without_a_github_repository_gives_no_template() {
    let rig = Rig::new("koji");
    let runner = rig.open().unwrap();
    let text = first_instructions(&rig, &runner);
    assert!(!text.contains("pull request template"), "{text}");
    assert!(text.contains("/mattpocock:pr"), "{text}");
}

#[test]
fn an_owners_repository_without_a_template_gives_none() {
    let rig = Rig::new("koji");
    rig.forge.put_file(OWNERS, "README.md", "# Hello\n");
    rig.forge.put_file(OWNERS, ".github/CODEOWNERS", "* @me\n");
    let runner = rig.open().unwrap();
    let text = first_instructions(&rig, &runner);
    assert!(!text.contains("pull request template"), "{text}");
    assert!(text.contains("/mattpocock:pr"), "{text}");
}

#[test]
fn a_failed_read_keeps_the_last_good_template() {
    let rig = Rig::new("koji");
    rig.forge
        .put_file(OWNERS, "pull_request_template.md", "First words\n");
    let runner = rig.open().unwrap();

    rig.forge
        .put_file(OWNERS, "pull_request_template.md", "Second words\n");
    rig.forge.set_down(true);
    reread(&rig, &runner);
    rig.forge.set_down(false);
    let text = first_instructions(&rig, &runner);
    assert!(text.contains("First words"), "{text}");
    let told: Vec<String> = runner.lock().unwrap().take_notes();
    assert!(
        told.iter().any(|line| line
            .starts_with("cannot read the pull request template of shep-pm's .github: ")
            && line.ends_with("so the last one read stays")),
        "{told:?}"
    );
}

#[test]
fn a_reread_picks_up_a_changed_template() {
    let rig = Rig::new("koji");
    rig.forge
        .put_file(OWNERS, "pull_request_template.md", "First words\n");
    let runner = rig.open().unwrap();

    rig.forge
        .put_file(OWNERS, "pull_request_template.md", "Second words\n");
    reread(&rig, &runner);
    let text = first_instructions(&rig, &runner);
    assert!(text.contains("Second words"), "{text}");
    assert!(!text.contains("First words"), "{text}");
}

#[test]
fn a_template_over_8_kib_is_cut_at_a_line_and_says_so() {
    let line = format!("{}\n", "x".repeat(99));
    let text = instructions_with_owners_template(&line.repeat(100));
    let (_, shown) = text.split_once("<template>\n").expect(&text);
    let (shown, after) = shown.split_once("[cut here").expect(&text);
    assert_eq!(shown, line.repeat(81), "the last line that fits in 8 KiB");
    assert!(
        after.starts_with(": the template is longer than 8 KiB]\n"),
        "{after}"
    );
}

#[test]
fn status_names_where_the_template_came_from() {
    let source = |runner: &Mutex<Runner>| {
        let runner = runner.lock().unwrap();
        serde_json::to_value(runner.status()).unwrap()["pull_request_template"].clone()
    };

    let rig = Rig::new("koji");
    let runner = rig.open().unwrap();
    assert_eq!(source(&runner), "none");

    let rig = Rig::new("koji");
    rig.forge
        .put_file(OWNERS, "pull_request_template.md", "Words\n");
    let runner = rig.open().unwrap();
    assert_eq!(source(&runner), serde_json::json!({ "owner": OWNERS }));
    assert_eq!(
        runner.lock().unwrap().template_source(),
        TemplateSource::Owner(OWNERS.to_owned())
    );

    let rig = Rig::new("koji");
    rig.forge
        .put_file(OWNERS, "pull_request_template.md", "Words\n");
    fs::write(rig.repo().join("pull_request_template.md"), "Repo\n").unwrap();
    let runner = rig.open().unwrap();
    assert_eq!(source(&runner), "repo");
}

fn reread(rig: &Rig, runner: &Mutex<Runner>) {
    let mut runner = runner.lock().unwrap();
    runner
        .reread(rig.settings(), rig.kelpie_settings())
        .unwrap();
}
