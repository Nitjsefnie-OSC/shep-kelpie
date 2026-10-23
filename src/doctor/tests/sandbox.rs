use super::*;

#[tokio::test]
async fn doctor_names_the_linux_sandbox_limits() {
    let mut scene = Scene::new().await;
    scene.host.os = "linux";
    let report = scene.report().await;
    let found = ok(&report, "sandbox: limits");
    assert!(
        found.contains("nested harness folders made during a call are caught after it"),
        "{found}"
    );
    assert!(
        found.contains("cannot reach the lease door, so `cargo-test` runs unleased"),
        "{found}"
    );
    assert!(report.passed());
    let order = subjects(&report);
    assert_eq!(order[2..4], ["sandbox", "sandbox: limits"]);

    for os in ["macos", ""] {
        scene.host.os = os;
        let report = scene.report().await;
        assert!(!subjects(&report).contains(&"sandbox: limits"), "{os}");
    }
}

// macOS runs crabpot, linked into kelpie, so it has no node srt to miss.
#[tokio::test]
async fn a_macos_machine_needs_no_node_srt_and_another_is_told_to_install_it() {
    let mut scene = Scene::new().await;
    std::fs::remove_file(Tools::under(&scene.kelpie_home).sandbox()).unwrap();

    scene.host.os = "macos";
    scene.host.keychain = true;
    let report = scene.report().await;
    assert_eq!(
        ok(&report, "sandbox"),
        "the sandbox runtime can run",
        "{:#?}",
        report.render()
    );

    scene.host.os = "linux";
    scene.host.keychain = false;
    let report = scene.report().await;
    let (what, _) = missing(&report, "sandbox");
    assert!(what.contains("no runner starts"), "{what}");
}
