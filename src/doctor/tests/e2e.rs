use super::*;

#[tokio::test]
async fn e2e_is_silent_without_a_table() {
    let scene = Scene::new().await;
    scene.runs("golbat", |_| {});

    let report = scene.report().await;

    assert!(!subjects(&report).iter().any(|s| s.ends_with(": e2e")));
}

#[tokio::test]
async fn e2e_names_the_host_when_ssh_and_rsync_are_here() {
    let scene = Scene::new().await;
    scene.runs("golbat", |t| {
        t.insert("e2e".into(), json!({ "host": "e2e-box" }));
    });

    let report = scene.report().await;

    assert_eq!(
        ok(&report, "golbat: e2e"),
        "runs `scripts/e2e-remote.sh` on e2e-box over ssh and rsync"
    );
}

#[tokio::test]
async fn e2e_names_the_program_that_is_missing() {
    let mut scene = Scene::new().await;
    scene.runs("golbat", |t| {
        t.insert(
            "e2e".into(),
            json!({ "host": "e2e-box", "command": "ci/e2e.sh" }),
        );
    });

    for gone in ["ssh", "rsync"] {
        scene.host.off_path = vec![gone];
        let report = scene.report().await;
        let (what, fix) = missing(&report, "golbat: e2e");
        assert!(what.contains(&format!("`{gone}`")), "{what}");
        assert!(fix.contains(&format!("install `{gone}`")), "{fix}");
        assert!(!report.passed());
    }
}
