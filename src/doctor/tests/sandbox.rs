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
