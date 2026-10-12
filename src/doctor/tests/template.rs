use super::*;

const SUBJECT: &str = "koji: pull request template";

#[tokio::test]
async fn a_repo_with_no_template_anywhere_says_none() {
    let scene = Scene::new().await;

    let report = scene.report().await;

    assert_eq!(ok(&report, SUBJECT), "none");
}

#[tokio::test]
async fn a_template_in_the_owners_github_repository_is_named() {
    let scene = Scene::new().await;
    scene.forge.put_file(
        "shep-pm/.github",
        "docs/pull_request_template.md",
        "Words\n",
    );

    let report = scene.report().await;

    assert_eq!(ok(&report, SUBJECT), "the owner's, from shep-pm/.github");
}

#[tokio::test]
async fn the_repos_own_template_is_named_before_the_owners() {
    let scene = Scene::new().await;
    scene
        .forge
        .put_file("shep-pm/.github", "pull_request_template.md", "Words\n");
    std::fs::write(scene.home.join("koji/pull_request_template.md"), "Mine\n").unwrap();

    let report = scene.report().await;

    assert_eq!(ok(&report, SUBJECT), "the repo's own");
}

#[tokio::test]
async fn an_owners_template_that_cannot_be_read_leaves_doctor_unsure() {
    let scene = Scene::new().await;
    scene.forge.set_down(true);

    let report = scene.report().await;

    match verdict(&report, SUBJECT) {
        Verdict::Unsure { what, .. } => {
            assert!(
                what.starts_with("cannot read shep-pm's .github repository: "),
                "{what}"
            );
        }
        other => panic!("`{SUBJECT}` is {other:?}"),
    }
}
