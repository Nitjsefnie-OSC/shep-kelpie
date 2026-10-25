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

// macOS and Linux run crabpot, linked into kelpie, so they have no node srt to miss.
#[tokio::test]
async fn macos_and_linux_need_no_node_srt_and_windows_is_told_to_install_it() {
    let mut scene = Scene::new().await;
    std::fs::remove_file(Tools::under(&scene.kelpie_home).sandbox()).unwrap();

    for os in ["macos", "linux"] {
        scene.host.os = os;
        scene.host.keychain = os == "macos";
        let report = scene.report().await;
        assert_eq!(
            ok(&report, "sandbox"),
            "the sandbox runtime can run",
            "{os}: {:#?}",
            report.render()
        );
    }

    scene.host.os = "windows";
    scene.host.keychain = false;
    let report = scene.report().await;
    let (what, _) = missing(&report, "sandbox");
    assert!(what.contains("no runner starts"), "{what}");
}
