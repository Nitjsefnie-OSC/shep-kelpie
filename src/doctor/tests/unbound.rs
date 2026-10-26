use super::*;

#[tokio::test]
async fn an_unbound_file_gives_one_missing_line_naming_the_command() {
    let scene = Scene::new().await;
    scene.runs("golbat", |_| {});
    let stub = "---\nrole: pm\n---\nYou keep the project's notes.\n";
    crate::test::write_in(&scene.kelpie_home.join("agents"), "scribe.md", stub);

    let report = scene.report().await;

    let lines = subjects(&report);
    assert_eq!(lines.iter().filter(|s| **s == "agent scribe").count(), 1);
    let (what, fix) = missing(&report, "agent scribe");
    assert_eq!(what, "`scribe.md` binds no harness, model or effort");
    assert_eq!(fix, "shep kelpie agents new pm --name scribe");
}

#[tokio::test]
async fn a_folder_with_none_gives_no_line() {
    let scene = Scene::new().await;

    let report = scene.report().await;

    let lines = subjects(&report);
    assert!(lines.iter().all(|s| !s.starts_with("agent ")), "{lines:?}");
}

#[tokio::test]
async fn a_stub_the_migration_would_write_names_upgrade_and_migrate() {
    let scene = Scene::new().await;
    scene.runs("golbat", |_| {});
    let stub = "---\nextends: kelpie:pm\n---\n";
    crate::test::write_in(&scene.kelpie_home.join("agents"), "pm.md", stub);

    let report = scene.report().await;

    let (what, fix) = missing(&report, "agent pm");
    assert_eq!(what, "`pm.md` binds no harness, model or effort");
    assert_eq!(fix, "shep kelpie upgrade, or shep kelpie agents migrate");
}

#[tokio::test]
async fn a_file_the_migration_skips_says_why() {
    let scene = Scene::new().await;
    scene.runs("golbat", |_| {});
    let file = "---\nextends: kelpie:spec\nharness: pi\n---\n";
    crate::test::write_in(&scene.kelpie_home.join("agents"), "spec.md", file);

    let report = scene.report().await;

    let (what, fix) = missing(&report, "agent spec");
    assert!(
        what.starts_with("`spec.md` is skipped by the migration: "),
        "{what}"
    );
    assert!(what.contains("model"), "{what}");
    assert!(fix.contains("`model` and `effort`"), "{fix}");
}
