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

// crabpot is in kelpie on every platform, so no platform has a node srt to
// miss. Windows fences with an install, and its line names each state of it.
#[tokio::test]
async fn macos_and_linux_need_nothing_installed_and_windows_names_its_install() {
    let mut scene = Scene::new().await;

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
    assert_eq!(
        ok(&report, "sandbox"),
        "the crabpot install can run, loopback closed",
        "{:#?}",
        report.render()
    );

    scene.host.windows = Some(Ok(WindowsInstall::provisioned(true)));
    let report = scene.report().await;
    assert_eq!(
        ok(&report, "sandbox"),
        "the crabpot install can run, loopback open"
    );
}

#[tokio::test]
async fn a_windows_machine_with_no_install_is_told_to_make_one() {
    let mut scene = Scene::new().await;
    scene.host.os = "windows";
    scene.host.windows = Some(Ok(WindowsInstall {
        provisioned: false,
        setup_installed: None,
        setup_expected: 1,
        loopback_open: false,
        seclogon_runs: true,
        desktop_session: true,
    }));
    let report = scene.report().await;
    let (what, fix) = missing(&report, "sandbox");
    assert!(what.contains("no crabpot install"), "{what}");
    assert_eq!(fix, "run `crabpot windows-install`");
    assert!(!report.passed());
}

#[tokio::test]
async fn a_windows_install_of_another_setup_version_names_both_numbers() {
    let mut scene = Scene::new().await;
    scene.host.os = "windows";
    scene.host.windows = Some(Ok(WindowsInstall {
        provisioned: false,
        setup_installed: Some(3),
        setup_expected: 4,
        loopback_open: false,
        seclogon_runs: true,
        desktop_session: true,
    }));
    let (what, fix) = missing(&scene.report().await, "sandbox");
    assert!(
        what.contains("setup version 3") && what.contains("wants 4"),
        "{what}"
    );
    assert_eq!(fix, "run `crabpot windows-install`");
}

#[tokio::test]
async fn a_windows_machine_with_seclogon_off_or_in_session_0_is_told_its_fix() {
    let mut scene = Scene::new().await;
    scene.host.os = "windows";
    scene.host.windows = Some(Ok(WindowsInstall {
        seclogon_runs: false,
        ..WindowsInstall::provisioned(false)
    }));
    let (what, fix) = missing(&scene.report().await, "sandbox");
    assert!(what.contains("Secondary Logon"), "{what}");
    assert!(fix.contains("seclogon"), "{fix}");

    scene.host.windows = Some(Ok(WindowsInstall {
        desktop_session: false,
        ..WindowsInstall::provisioned(false)
    }));
    let (what, fix) = missing(&scene.report().await, "sandbox");
    assert!(what.contains("session 0"), "{what}");
    assert!(fix.contains("desktop session"), "{fix}");

    scene.host.windows = Some(Err("srt-win user status exited 1"));
    let (what, fix) = missing(&scene.report().await, "sandbox");
    assert!(what.contains("cannot be read"), "{what}");
    assert_eq!(fix, "run `crabpot windows-install`");
}

// A project with the browser on runs a dev server that binds a local port,
// and only an `--allow-loopback` install can hold it. The fix says what
// opening loopback allows.
#[tokio::test]
async fn a_listening_project_on_a_closed_windows_install_is_told_the_widening() {
    let mut scene = Scene::new().await;
    scene.host.os = "windows";
    scene.runs("golbat", |t| t["browser"] = json!({ "enabled": true }));
    let report = scene.report().await;
    let (what, fix) = missing(&report, "sandbox");
    assert!(what.contains("dev server"), "{what}");
    assert!(what.contains("closes loopback"), "{what}");
    assert!(
        fix.contains("crabpot windows-install --allow-loopback"),
        "{fix}"
    );
    assert!(
        fix.contains("every service on its own loopback address"),
        "{fix}"
    );
    assert!(!report.passed());

    // With the install open, the same project runs, and the line says so.
    scene.host.windows = Some(Ok(WindowsInstall::provisioned(true)));
    let report = scene.report().await;
    assert_eq!(
        ok(&report, "sandbox"),
        "the crabpot install can run, loopback open"
    );

    // A missing install on such a project is fixed with the open form too.
    scene.host.windows = Some(Ok(WindowsInstall {
        provisioned: false,
        setup_installed: None,
        setup_expected: 1,
        loopback_open: false,
        seclogon_runs: true,
        desktop_session: true,
    }));
    let (_, fix) = missing(&scene.report().await, "sandbox");
    assert_eq!(fix, "run `crabpot windows-install --allow-loopback`");
}
