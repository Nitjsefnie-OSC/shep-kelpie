//! The line that says where a project's tickets live

use super::*;

#[tokio::test]
async fn a_project_says_where_its_tickets_live() {
    let scene = Scene::new().await;
    scene.runs("golbat", |t| t["tickets"] = json!({ "source": "local" }));
    let folder = scene.kelpie_home.join("golbat").join("tickets");
    let report = scene.report().await;
    assert_eq!(ok(&report, "koji: tickets"), "github: the repo's issues");
    assert_eq!(
        ok(&report, "golbat: tickets"),
        format!("local: 0 tickets in {}", folder.display())
    );

    crate::test::write_in(
        &folder,
        "k1.md",
        "---\ntitle: \"One\"\nlabels: []\nstate: open\n---\n",
    );
    let report = scene.report().await;
    assert_eq!(
        ok(&report, "golbat: tickets"),
        format!("local: 1 ticket in {}", folder.display())
    );
}

#[tokio::test]
async fn a_local_store_project_lists_what_kelpie_still_writes_to_its_repo() {
    let scene = Scene::new().await;
    scene.runs("golbat", |t| t["tickets"] = json!({ "source": "local" }));
    let report = scene.report().await;
    assert_eq!(
        ok(&report, "golbat: repo writes"),
        "the branch, the pull request and its comments, and nothing else"
    );
    assert!(
        !subjects(&report).contains(&"koji: repo writes"),
        "a GitHub project writes its issues too"
    );
}

#[tokio::test]
async fn a_local_ticket_that_cannot_be_read_is_missing_and_fails_the_run() {
    let scene = Scene::new().await;
    scene.runs("golbat", |t| t["tickets"] = json!({ "source": "local" }));
    let folder = scene.kelpie_home.join("golbat").join("tickets");
    crate::test::write_in(&folder, "k1.md", "no front matter\n");
    let report = scene.report().await;
    let (what, fix) = missing(&report, "golbat: tickets");
    assert!(what.contains("k1.md"), "{what}");
    assert_eq!(fix, "repair or remove the file it names");
    assert!(!report.passed());
}
