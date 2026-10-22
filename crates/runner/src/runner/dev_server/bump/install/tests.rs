use std::collections::BTreeMap;
use std::time::Duration;

use super::*;
use crate::ports::{DevPort, Policy};

fn run(cwd: &Path) -> DevRun {
    DevRun {
        program: "npm".to_owned(),
        args: vec!["run".to_owned(), "dev".to_owned()],
        cwd: cwd.to_owned(),
        port: 4421,
        env: BTreeMap::new(),
        policy: Policy {
            hosts: vec!["app.example.com".to_owned()],
            ..Policy::default()
        },
        settings: PathBuf::from("settings.json"),
        log: PathBuf::from("log"),
        ready_within: Duration::from_secs(60),
        gate: DevPort::default(),
    }
}

fn checkout(files: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for file in files {
        let at = dir.path().join(file);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, "").unwrap();
    }
    dir
}

#[test]
fn the_packages_are_installed_from_the_lockfile_before_the_server_starts() {
    let dir = checkout(&["web/package-lock.json"]);
    let cwd = dir.path().join("web");
    let mut run = run(&cwd);

    before(&mut run, dir.path(), Path::new("/build"));

    assert_eq!(run.program, crate::spawn::sh().to_string_lossy());
    assert_eq!(run.args[0], "-c");
    assert!(
        run.args[1].contains("npm ci --ignore-scripts"),
        "{:?}",
        run.args
    );
    assert!(run.args[1].ends_with("shift; exec \"$@\""));
    // The folder, then the server's own command.
    assert_eq!(run.args[3], cwd.display().to_string());
    assert_eq!(run.args[4..], ["npm", "run", "dev"]);
    assert_eq!(run.env["npm_config_cache"], "/build/npm-cache");
    assert_eq!(run.policy.hosts, ["app.example.com", "registry.npmjs.org"]);
    assert!(run.ready_within > Duration::from_secs(60));
}

#[test]
fn a_lockfile_above_the_servers_folder_is_found() {
    let dir = checkout(&["pnpm-lock.yaml", "web/src/main.ts"]);
    let mut run = run(&dir.path().join("web/src"));

    before(&mut run, dir.path(), Path::new("/build"));

    assert_eq!(run.args[3], dir.path().display().to_string());
    assert!(run.args[1].contains("pnpm install --frozen-lockfile"));
}

#[test]
fn a_checkout_with_no_lockfile_starts_its_server_as_it_is() {
    let dir = checkout(&["web/package.json"]);
    let mut run = run(&dir.path().join("web"));

    before(&mut run, dir.path(), Path::new("/build"));

    assert_eq!(run.program, "npm");
    assert_eq!(run.args, ["run", "dev"]);
    assert_eq!(run.policy.hosts, ["app.example.com"]);
}

#[test]
fn the_script_stops_when_the_install_fails_and_runs_the_server_when_it_does_not() {
    let dir = checkout(&["package-lock.json"]);
    let build = dir.path().join("b");
    for (install, ran) in [("true", true), ("false", false)] {
        let mut run = run(dir.path());
        run.program = "echo".to_owned();
        run.args = vec!["served".to_owned()];
        before(&mut run, dir.path(), &build);
        let script = run.args[1].replace("npm ci --ignore-scripts --no-audit --no-fund", install);
        let out = std::process::Command::new(crate::spawn::sh())
            .arg("-c")
            .arg(script)
            .args(&run.args[2..])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).contains("served"),
            ran,
            "{install}"
        );
    }
}
