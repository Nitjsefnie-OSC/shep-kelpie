use super::*;
use crate::github::ApiError;

#[tokio::test]
async fn an_installed_app_that_mints_a_token_is_ok() {
    let scene = Scene::new().await;

    let report = scene.report().await;

    assert_eq!(
        ok(&report, "koji: github app"),
        "kelpie's App is installed on shep-pm/koji and mints a token"
    );
}

// No App is unsure: kelpie falls back to the user's own `gh` login, so it does not fail the run.
#[tokio::test]
async fn an_owner_with_no_app_is_unsure_and_names_setup() {
    let scene = Scene::new().await;
    scene.runs("golbat", |t| {
        t["git"]["remote"] = json!("someone-else/golbat");
    });

    let report = scene.report().await;

    let Verdict::Unsure { what, next } = verdict(&report, "golbat: github app") else {
        panic!("{:#?}", report.render());
    };
    assert_eq!(what, "kelpie has no GitHub App for someone-else");
    assert!(next.contains("`shep kelpie github setup`"), "{next}");
    assert!(next.contains("`--org someone-else`"), "{next}");
    assert!(report.passed());
}

#[tokio::test]
async fn github_busy_or_out_of_reach_is_unsure_and_a_refused_app_is_missing() {
    let scene = Scene::new().await;
    for (error, said) in [
        (ApiError::Refused(502), "GitHub answered HTTP 502"),
        (ApiError::RateLimited, "rate limit"),
        (
            ApiError::Unreachable("curl exited 6".into()),
            "cannot reach GitHub",
        ),
    ] {
        // A GitHub that could not answer is believed for half a minute.
        scene.clock.advance(60);
        scene.github.fail_next(error);
        let report = scene.report().await;
        let Verdict::Unsure { what, .. } = verdict(&report, "koji: github app") else {
            panic!("{:#?}", report.render());
        };
        assert!(what.contains(said), "{what}");
    }

    scene.clock.advance(60);
    scene.github.fail_next(ApiError::Refused(401));
    let report = scene.report().await;
    let (what, fix) = missing(&report, "koji: github app");
    assert_eq!(what, "no token mints: GitHub answered HTTP 401");
    assert!(
        fix.contains("`shep kelpie github setup --replace`"),
        "{fix}"
    );
}

#[tokio::test]
async fn an_app_not_installed_on_the_repo_is_missing_and_names_its_install_page() {
    let scene = Scene::new().await;
    scene.runs("rotom", |_| {});

    let report = scene.report().await;

    let (what, fix) = missing(&report, "rotom: github app");
    assert_eq!(what, "kelpie-shep-pm is not installed on shep-pm/rotom");
    assert_eq!(
        fix,
        "install it there from https://github.com/apps/kelpie-shep-pm/installations/new"
    );
    ok(&report, "koji: github app");
    assert!(!report.passed());
}

#[tokio::test]
async fn an_org_s_repo_the_app_covers_with_no_maintainer_set_is_missing_and_names_the_setting() {
    let scene = Scene::new().await;
    scene.forge.set_owner_is_user(false);

    let report = scene.report().await;

    let (what, fix) = missing(&report, "koji: maintainer");
    assert_eq!(
        what,
        "shep-pm is an organization, so a ruling mentions no one and notifies no one"
    );
    assert!(fix.contains("set `git.maintainer`"), "{fix}");
    assert!(!report.passed());
}

#[tokio::test]
async fn a_maintainer_that_is_set_a_user_s_repo_or_an_app_that_does_not_cover_it_need_no_line() {
    // An organization's repo, but the maintainer is set.
    let scene = Scene::new().await;
    scene.forge.set_owner_is_user(false);
    scene.runs("golbat", |t| {
        t["git"]["maintainer"] = json!("octocat");
    });
    let report = scene.report().await;
    assert!(!subjects(&report).contains(&"golbat: maintainer"));

    // An organization's repo the App is not installed on: no ruling goes through it.
    let scene = Scene::new().await;
    scene.forge.set_owner_is_user(false);
    scene.runs("rotom", |_| {});
    let report = scene.report().await;
    assert!(!subjects(&report).contains(&"rotom: maintainer"));

    // A user's repo, which the default mentions.
    let scene = Scene::new().await;
    let report = scene.report().await;
    assert!(!subjects(&report).contains(&"koji: maintainer"));
}

#[tokio::test]
async fn an_owner_the_forge_cannot_place_is_unsure_and_names_the_setting() {
    let scene = Scene::new().await;
    scene.forge.set_owner_unreadable(true);

    let report = scene.report().await;

    let Verdict::Unsure { what, next } = verdict(&report, "koji: maintainer") else {
        panic!("{:#?}", report.render());
    };
    assert!(
        what.contains("cannot tell whether shep-pm is a user"),
        "{what}"
    );
    assert!(next.contains("`git.maintainer`"), "{next}");
}
