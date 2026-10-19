use super::*;

#[tokio::test]
async fn the_base_is_named_and_found_on_the_forge() {
    let scene = Scene::new().await;
    scene.runs("golbat", |t| t["git"]["base"] = json!("next"));
    scene.forge.set_branches(&["next"]);
    let report = scene.report().await;

    let default = ok(&report, "koji: base");
    assert!(default.contains("cut from `origin/main`"), "{default}");
    let named = ok(&report, "golbat: base");
    assert!(named.contains("cut from `origin/next`"), "{named}");
    assert!(named.contains("merge into `next`"), "{named}");
}

#[tokio::test]
async fn a_base_the_repo_does_not_have_is_missing_and_fails_the_run() {
    let scene = Scene::new().await;
    scene.runs("golbat", |t| t["git"]["base"] = json!("next"));
    let report = scene.report().await;

    let (what, fix) = missing(&report, "golbat: base");
    assert!(
        what.contains("`next` is not a branch of shep-pm/golbat"),
        "{what}"
    );
    assert!(fix.contains("set `git.base`"), "{fix}");
    assert!(!report.passed());
}
