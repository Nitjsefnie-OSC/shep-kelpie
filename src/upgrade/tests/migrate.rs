//! The agent-file migration an upgrade runs after the install and before the restarts

use super::*;

const STUB: &str = "---\nextends: kelpie:pm\n---\n";
const WROTE: &str = "pm.md: wrote claude-code claude-opus-5-5 medium";

// A stand-in build that answers `version --json`, and for `agents migrate`
// appends the folder it was told to migrate to `log` and prints `reply`.
fn migrating(kelpie: &str, log: &Path, reply: &str, exit: u8) -> String {
    let json = format!(r#"{{"kelpie":"{kelpie}","shep":"0.17.0","protocol":{PROTOCOL_VERSION}}}"#);
    format!(
        "#!/bin/sh\n\
         case \"$1 $2\" in\n\
         'version --json') echo '{json}' ;;\n\
         'agents migrate') echo \"$KELPIE_HOME\" >> '{log}'; echo '{reply}'; exit {exit} ;;\n\
         esac\n",
        log = log.display()
    )
}

fn stub_in(rig: &Rig) -> PathBuf {
    let agents = rig.home.join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    let stub = agents.join("pm.md");
    std::fs::write(&stub, STUB).unwrap();
    stub
}

#[tokio::test]
async fn an_install_runs_the_installed_build_s_step_before_the_restarts_and_says_its_lines() {
    let rig = Rig::new().await;
    let log = rig.builds.join("migrated");
    let new = rig.builds.join("new");
    write_script(&new, &migrating("0.3.0", &log, WROTE, 0));

    let (ran, said) = rig.install(&new).await;

    ran.unwrap();
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        format!("{}\n", rig.home.display()),
        "the installed build migrated once, in kelpie's home"
    );
    let at = |needle: &str| said.iter().position(|l| l.contains(needle));
    let wrote = at(WROTE).unwrap_or_else(|| panic!("{said:?}"));
    assert!(at("installed kelpie 0.3.0").unwrap() < wrote, "{said:?}");
    assert!(wrote < at("restarting onto it").unwrap(), "{said:?}");
}

#[tokio::test]
async fn a_step_that_fails_is_said_and_the_restarts_go_on() {
    let mut rig = Rig::new().await;
    let log = rig.builds.join("migrated");
    let new = rig.builds.join("new");
    write_script(&new, &migrating("0.3.0", &log, "", 1));

    let (ran, said) = rig.install(&new).await;

    ran.unwrap();
    assert!(
        said.iter().any(|l| l.contains("were not migrated")),
        "{said:?}"
    );
    assert_eq!(rig.restarts(), ["kelpie", "koji"]);
}

#[tokio::test]
async fn a_bare_upgrade_migrates_in_process() {
    let rig = Rig::new().await;
    let stub = stub_in(&rig);
    rig.cargo_installs("0.3.0", "0.17.0");

    let (ran, said) = rig.upgrade(Action::Installed).await;

    ran.unwrap();
    assert!(said.iter().any(|l| l == WROTE), "{said:?}");
    let written = std::fs::read_to_string(stub).unwrap();
    assert!(written.contains("harness: claude-code\n"), "{written}");
    assert!(written.contains("model: claude-opus-5-5\n"), "{written}");
}

#[tokio::test]
async fn a_rollback_does_not_migrate() {
    let rig = Rig::new().await;
    let log = rig.builds.join("migrated");
    std::fs::remove_file(&rig.installed).unwrap();
    write_script(&rig.installed, &migrating("0.1.0", &log, WROTE, 0));
    let new = rig.build("new", "0.3.0", "0.17.0");
    rig.install(&new).await.0.unwrap();
    let stub = stub_in(&rig);

    let (ran, said) = rig.upgrade(Action::Rollback).await;

    ran.unwrap();
    assert!(!log.exists(), "the build put back was not asked to migrate");
    assert_eq!(std::fs::read_to_string(stub).unwrap(), STUB);
    assert!(said.iter().all(|l| l != WROTE), "{said:?}");
}
