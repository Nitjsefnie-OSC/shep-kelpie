//! A repo with no template of its own gets its owner's `.github` template

use std::fs;
use std::sync::Mutex;

use super::{Runner, TemplateSource, step};
use crate::ports::{Cost, Usage, Visibility};
use crate::runner::instructions::TEMPLATE_FILE;
use crate::test::{Rig, Scripted};

// The rig's project is `shep-pm/shep`, so its owner's repository is this
const OWNERS: &str = "shep-pm/.github";

// The instructions file the worker's first turn was started with
fn first_instructions(rig: &Rig, runner: &Mutex<Runner>) -> String {
    rig.ask(runner, "add", Some("7"));
    rig.claude.script([Scripted::Reply(usage(1), Cost(1))]);
    step(runner).unwrap();
    let [seen] = rig.claude.seen().try_into().unwrap();
    let file = seen.call.instructions.expect("the worker has instructions");
    fs::read_to_string(file).unwrap()
}

fn usage(n: u64) -> Usage {
    Usage {
        input: n,
        cache_write: 10 * n,
        cache_write_5m: 0,
        cache_read: 100 * n,
        output: 1000 * n,
    }
}

// A rig whose owner's `.github` has one template, with what its worker's first
// turn was started with
fn with_owners_template(text: &str) -> (Rig, String) {
    let rig = Rig::new("koji");
    rig.forge
        .put_file(OWNERS, ".github/pull_request_template.md", text);
    let runner = rig.open().unwrap();
    let instructions = first_instructions(&rig, &runner);
    (rig, instructions)
}

// The template file in the worker's build folder
fn template_file(rig: &Rig) -> std::path::PathBuf {
    rig.build_7().join(TEMPLATE_FILE)
}

#[test]
fn a_repo_without_a_template_gets_its_owners_text_in_a_file_it_reads() {
    let (rig, text) = with_owners_template("## Summary\n\n- what changed\n");
    let file = template_file(&rig);
    assert!(
        text.contains(&format!(
            "The repo has no pull request template of its own, so its owner's applies, \
             `.github/pull_request_template.md` of `{OWNERS}`. Its text is in `{}`: read it \
             before you write the body, fill it in rather than write your own, and end with \
             `Resolves #<issue>`.",
            file.display()
        )),
        "{text}"
    );
    assert!(
        !text.contains("## Summary"),
        "the template's text is not in what every request carries: {text}"
    );
    assert!(!text.contains("/mattpocock:pr"), "{text}");
    assert_eq!(
        fs::read_to_string(file).unwrap(),
        "<template>\n## Summary\n\n- what changed\n</template>\n"
    );
}

#[test]
fn a_planted_link_at_the_template_file_is_replaced_and_its_target_is_left_alone() {
    let rig = Rig::new("koji");
    rig.forge
        .put_file(OWNERS, ".github/pull_request_template.md", "## Summary\n");
    let runner = rig.open().unwrap();
    let build = rig.build_7();
    fs::create_dir_all(&build).unwrap();
    let (file, planted) = (build.join(TEMPLATE_FILE), build.join("planted.md"));
    fs::write(&planted, "#!/bin/sh\n").unwrap();
    std::os::unix::fs::symlink(&planted, &file).unwrap();

    rig.claude.script([Scripted::Reply(usage(1), Cost(1))]);
    rig.ask(&runner, "add", Some("7"));
    step(&runner).unwrap();

    assert_eq!(fs::read_to_string(&planted).unwrap(), "#!/bin/sh\n");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "<template>\n## Summary\n</template>\n"
    );
}

#[test]
fn the_repos_own_template_wins_over_its_owners_and_no_file_is_written() {
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
    assert!(
        !template_file(&rig).exists(),
        "the worker reads the repo's own"
    );
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
    let file = template_file(&rig);
    assert!(
        text.contains(&format!(
            "The repo has no pull request templates of its own, so its owner's apply, in \
             `{folder}` of `{OWNERS}`. Each is in `{}`, by name: read it before you write the \
             body, fill in the one that fits rather than write your own, and end with \
             `Resolves #<issue>`.",
            file.display()
        )),
        "{text}"
    );
    assert!(
        !text.contains("## Feature"),
        "the templates' text is in the file, not the instructions: {text}"
    );
    assert_eq!(
        fs::read_to_string(file).unwrap(),
        "<template name=\"feature.md\">\n## Feature\n</template>\n\
         <template name=\"fix.md\">\n## Fix\n</template>\n"
    );
}

#[test]
fn an_owner_without_a_github_repository_gives_no_template() {
    let rig = Rig::new("koji");
    let runner = rig.open().unwrap();
    let text = first_instructions(&rig, &runner);
    assert!(!text.contains("pull request template"), "{text}");
    assert!(text.contains("/mattpocock:pr"), "{text}");
    assert!(!template_file(&rig).exists());
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
    assert!(!template_file(&rig).exists());
}

#[test]
fn a_private_github_repository_gives_no_template() {
    let rig = Rig::new("koji");
    rig.forge
        .put_file(OWNERS, "pull_request_template.md", "Secret words\n");
    rig.forge
        .set_visibility_of(&OWNERS.to_owned().try_into().unwrap(), Visibility::Private);
    let runner = rig.open().unwrap();
    let text = first_instructions(&rig, &runner);
    assert!(!text.contains("Secret words"), "{text}");
    assert!(text.contains("/mattpocock:pr"), "{text}");
    assert!(!template_file(&rig).exists());
}

#[test]
fn the_dot_github_folder_outranks_the_root_and_docs() {
    let rig = Rig::new("koji");
    rig.forge
        .put_file(OWNERS, "pull_request_template.md", "Root words\n");
    rig.forge
        .put_file(OWNERS, "docs/pull_request_template.md", "Docs words\n");
    rig.forge
        .put_file(OWNERS, ".github/pull_request_template.md", "Github words\n");
    let runner = rig.open().unwrap();
    let text = first_instructions(&rig, &runner);
    assert!(text.contains(".github/pull_request_template.md"), "{text}");
    let file = fs::read_to_string(template_file(&rig)).unwrap();
    assert!(file.contains("Github words"), "{file}");
    assert!(!file.contains("Root words"), "{file}");
    assert!(!file.contains("Docs words"), "{file}");
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
    assert!(text.contains("of `shep-pm/.github`"), "{text}");
    let held = fs::read_to_string(template_file(&rig)).unwrap();
    assert!(held.contains("First words"), "{held}");
    assert!(!held.contains("Second words"), "{held}");
    let told: Vec<String> = runner.lock().unwrap().take_notes();
    assert!(
        told.iter().any(|line| line
            .starts_with("cannot read the pull request template of shep-pm's .github: ")
            && line.ends_with("so the last one read stays")),
        "{told:?}"
    );
}

#[test]
fn a_reread_changes_the_template_file_before_the_next_call() {
    let rig = Rig::new("koji");
    rig.forge
        .put_file(OWNERS, "pull_request_template.md", "First words\n");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude.script([
        Scripted::Reply(usage(1), Cost(1)),
        Scripted::Reply(usage(1), Cost(1)),
    ]);
    step(&runner).unwrap(); // the first turn's call
    let held = fs::read_to_string(template_file(&rig)).unwrap();
    assert!(held.contains("First words"), "{held}");

    rig.forge
        .put_file(OWNERS, "pull_request_template.md", "Second words\n");
    reread(&rig, &runner);
    step(&runner).unwrap(); // the turn ends with no pull request, so the worker is sent back
    let held = fs::read_to_string(template_file(&rig)).unwrap();
    assert!(held.contains("Second words"), "{held}");
    assert!(!held.contains("First words"), "{held}");
}

#[test]
fn a_template_over_8_kib_is_cut_at_a_line_and_says_so() {
    let line = format!("{}\n", "x".repeat(99));
    let (rig, _) = with_owners_template(&line.repeat(100));
    let file = fs::read_to_string(template_file(&rig)).unwrap();
    let (shown, after) = file.split_once("[cut here").expect(&file);
    assert_eq!(
        shown,
        format!("<template>\n{}", line.repeat(81)),
        "the last line that fits in 8 KiB"
    );
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
