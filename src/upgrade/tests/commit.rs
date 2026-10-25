//! The commit each build names, and the runners an upgrade left on another

use super::*;

// A runner whose `status` names the build it was already running, and one that
// names the build the upgrade installs, made from a tree with changes on top.
const ON_OLD: &str = r#"{"work_items":[],"commit":"1f2e3a4"}"#;
const ON_NEW: &str = r#"{"work_items":[],"commit":"9c8b7a6-dirty"}"#;

#[tokio::test]
async fn an_install_says_which_commit_the_build_it_installed_was_made_from() {
    let rig = Rig::new().await;
    let new = rig.built_from("new", "0.3.0", "1f2e3a4");

    let (ran, said) = rig.install(&new).await;

    ran.unwrap();
    assert!(
        said.iter()
            .any(|l| l.contains("kelpie 0.3.0 (shep 0.16.1, built from 1f2e3a4)")),
        "{said:?}"
    );
}

#[tokio::test]
async fn a_rollback_says_which_commit_the_build_it_put_back_was_made_from() {
    let rig = Rig::new().await;
    // Two installs: the second keeps the first as the previous build.
    let first = rig.built_from("first", "0.2.0", "1f2e3a4");
    let second = rig.built_from("second", "0.3.0", "9c8b7a6");
    rig.install(&first).await.0.unwrap();
    rig.install(&second).await.0.unwrap();

    let (ran, said) = rig.upgrade(Action::Rollback).await;

    ran.unwrap();
    assert!(
        said.iter()
            .any(|l| l.contains("put back kelpie 0.2.0 (shep 0.16.1, built from 1f2e3a4)")),
        "{said:?}"
    );
}

#[tokio::test]
async fn an_upgrade_names_a_runner_still_on_another_commit() {
    let rig = Rig::new().await;
    // The stand-in runner is restarted and still answers with the build it
    // was already running, as one whose restart did not take would.
    rig.shepherd.says("koji", &[ON_OLD]);
    let new = rig.built_from("new", "0.3.0", "9c8b7a6");

    let (ran, said) = rig.install(&new).await;

    ran.unwrap();
    assert!(
        said.contains(&"`koji` still runs 1f2e3a4, not the installed 9c8b7a6".to_owned()),
        "{said:?}"
    );
}

#[tokio::test]
async fn an_upgrade_says_nothing_about_a_runner_on_the_installed_commit() {
    let rig = Rig::new().await;
    rig.shepherd.says("koji", &[ON_NEW]);
    let new = rig.built_from("new", "0.3.0", "9c8b7a6");

    let (ran, said) = rig.install(&new).await;

    ran.unwrap();
    assert!(!said.iter().any(|l| l.contains("still runs")), "{said:?}");
    assert!(
        !said.iter().any(|l| l.contains("names no build")),
        "{said:?}"
    );
}

#[tokio::test]
async fn an_upgrade_names_a_runner_whose_build_names_no_commit() {
    // The stand-in runner answers with a build from before a build named its
    // own commit, which is not known to be the installed one.
    let rig = Rig::new().await;
    let new = rig.built_from("new", "0.3.0", "9c8b7a6");

    let (ran, said) = rig.install(&new).await;

    ran.unwrap();
    assert!(
        said.contains(&"`koji` names no build, and the installed build is 9c8b7a6".to_owned()),
        "{said:?}"
    );
}

#[tokio::test]
async fn a_runner_left_stopped_is_not_named_as_behind() {
    let rig = Rig::new().await;
    rig.runs("lab", &rig.installed, false);
    let new = rig.built_from("new", "0.3.0", "9c8b7a6");

    let (ran, said) = rig.install(&new).await;

    ran.unwrap();
    assert!(!said.iter().any(|l| l.contains("`lab` still")), "{said:?}");
    assert!(
        !said.iter().any(|l| l.contains("`lab` names no build")),
        "{said:?}"
    );
}
