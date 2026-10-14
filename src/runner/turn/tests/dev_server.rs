//! A worker turn with the browser is given the project's dev server from `main`'s launch file
//!
//! The plan tests prepare a second turn's call without opening its browser, so a worker's
//! sandbox runs them. The tests that run a turn open the browser's socket, which it cannot.

use std::path::Path;
use std::sync::Arc;

use super::browser::{install, instructions_of, turn_on};
use super::*;
use crate::ports::{Agents, DevRun, Ending, Role, Session};
use crate::settings::Harness;
use crate::test::{FakeClaude, FakeDevServers};

// Shep's own launch file: one configuration, `shep-web`, on port 4421 in `web`.
const SHEP: &str = include_str!("../../../../fixtures/launch.json");

// A project with the browser on, its tools installed, and `launch` on `main`.
fn project(launch: Option<&str>) -> Rig {
    let rig = Rig::new("shep");
    turn_on(&rig);
    install(&rig);
    if let Some(launch) = launch {
        rig.land(".claude/launch.json", launch);
        rig.land("web/package.json", "{}\n");
    }
    rig
}

// The dev server planned for a second turn of work item 7, and what that turn is told
//
// The first turn begins the work item. Where a sandbox cannot bind the browser's socket it
// fails, and the item and its worktree are there all the same. `edit` runs in the worktree
// before the second turn is prepared.
fn planned(rig: &Rig, edit: impl FnOnce(&Path)) -> (Option<DevRun>, String) {
    try_planned(rig, edit).unwrap()
}

// The same, or why the second turn's call cannot be prepared.
fn try_planned(rig: &Rig, edit: impl FnOnce(&Path)) -> Result<(Option<DevRun>, String), String> {
    let runner = rig.open().unwrap();
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    rig.ask(&runner, "add", Some("7"));
    let _ = step(&runner);
    let runner = runner.lock().unwrap();
    let item = runner.current().expect("an item").clone();
    edit(&item.worktree);
    let session = Session::Resume(item.session.clone());
    let call = runner.worker_call(&item, session, Some("again".into()), true)?;
    let told = fs::read_to_string(call.instructions.as_ref().unwrap()).unwrap();
    assert_eq!(
        runner.dev_port(7),
        runner.planned.get(7).map(|run| run.port)
    );
    // The keychain is blocked as the worker's own call blocks it.
    let blocked = runner.planned.get(7).map(|run| run.policy.no_keychain);
    assert!(blocked.is_none_or(|blocked| blocked == runner.app_covers()));
    Ok((runner.planned.get(7), told))
}

#[test]
fn shep_launch_file_on_main_plans_its_server_in_the_worktree_on_its_port() {
    let rig = project(Some(SHEP));
    let (run, told) = planned(&rig, |_| {});

    let run = run.expect("a plan");
    assert_eq!(run.program, "npm");
    assert_eq!(run.args, ["run", "dev", "--", "--port", "4421"]);
    assert_eq!(run.port, 4421);
    let worktree = rig.paths().worktree(7);
    assert_eq!(run.cwd, worktree.join("web"));
    assert_eq!(
        run.env.get("NODE_USE_ENV_PROXY").map(String::as_str),
        Some("1")
    );
    assert_eq!(run.env.get("PORT"), None);
    assert_eq!(run.log, rig.paths().build(7).join("dev-server.log"));
    assert_eq!(run.ready_within, crate::ports::READY_WITHIN);
    assert!(told.contains("`http://localhost:4421`"), "{told}");
    assert!(told.contains(&run.log.display().to_string()), "{told}");
}

#[test]
fn the_servers_sandbox_writes_the_worktree_and_build_folder_and_reaches_the_browsers_hosts() {
    let rig = project(Some(SHEP));
    let (run, _) = planned(&rig, |_| {});

    let policy = run.expect("a plan").policy;
    assert_eq!(
        policy.write,
        [rig.paths().worktree(7), rig.paths().build(7)]
    );
    assert_eq!(policy.hosts, ["app.example.com"]);
    assert!(policy.listen);
    assert_eq!(policy.services, ["com.apple.FSEvents"]);
    assert!(policy.sockets.is_empty());
    assert!(
        policy.no_write.iter().any(|p| p.ends_with(".git/config")),
        "{:?}",
        policy.no_write
    );
    assert!(!policy.no_read.is_empty());
}

#[test]
fn an_edit_of_the_launch_file_in_the_worktree_stops_the_turn_before_any_server_is_planned() {
    let rig = project(Some(SHEP));
    let edit = r#"{"configurations":[{"name":"mine","runtimeExecutable":"sh","port":9999}]}"#;
    let refused = try_planned(&rig, |worktree| {
        fs::write(worktree.join(".claude/launch.json"), edit).unwrap();
    });

    let why = refused.unwrap_err();
    assert!(why.contains(".claude/launch.json"), "{why}");
}

#[test]
fn a_named_configuration_is_the_one_planned_else_the_first() {
    let two = r#"{"configurations":[
        {"name":"api","runtimeExecutable":"cargo","runtimeArgs":["run"],"port":8080},
        {"name":"web","runtimeExecutable":"npm","port":3000}]}"#;
    let first = planned(&project(Some(two)), |_| {}).0.expect("a plan");
    assert_eq!((first.program.as_str(), first.port), ("cargo", 8080));

    let rig = project(Some(two));
    rig.edit_settings(|s| {
        s.replace(
            "domains = [\"app.example.com\"]",
            "domains = [\"app.example.com\"]\nconfiguration = \"web\"",
        )
    });
    let web = planned(&rig, |_| {}).0.expect("a plan");
    assert_eq!((web.program.as_str(), web.port), ("npm", 3000));
}

#[test]
fn the_configured_port_is_used_when_it_is_free() {
    let rig = project(Some(SHEP));
    let (run, _) = planned(&rig, |_| {});
    assert_eq!(run.expect("a plan").port, 4421);
}

#[test]
fn a_port_that_is_taken_is_replaced_under_auto_port_in_the_variable_and_the_arguments() {
    let rig = project(Some(SHEP));
    rig.dev_servers.take(4421);
    // One pick for the first turn, one for the second.
    rig.dev_servers.pick_next(5301);
    rig.dev_servers.pick_next(5301);
    let (run, told) = planned(&rig, |_| {});

    let run = run.expect("a plan");
    assert_eq!(run.port, 5301);
    assert_eq!(run.env.get("PORT").map(String::as_str), Some("5301"));
    assert_eq!(run.args, ["run", "dev", "--", "--port", "5301"]);
    assert!(told.contains("localhost:5301"), "{told}");
}

#[test]
fn a_port_that_is_taken_without_auto_port_plans_no_server_and_the_instructions_say_so() {
    let fixed = r#"{"configurations":[{"name":"web","runtimeExecutable":"npm","port":4421}]}"#;
    let rig = project(Some(fixed));
    rig.dev_servers.take(4421);
    let (run, told) = planned(&rig, |_| {});

    assert_eq!(run, None);
    assert!(
        told.contains("runs no dev server for this turn: port 4421 is taken"),
        "{told}"
    );
    assert!(!told.contains("http://localhost"), "{told}");
}

#[test]
fn a_project_without_a_launch_file_on_main_plans_nothing() {
    let (run, told) = planned(&project(None), |_| {});
    assert_eq!(run, None);
    assert!(
        !told.contains("dev server") && !told.contains("localhost"),
        "{told}"
    );
}

#[test]
fn a_project_without_the_browser_plans_nothing_though_main_has_a_launch_file() {
    let rig = Rig::new("shep");
    install(&rig);
    rig.land(".claude/launch.json", SHEP);
    let (run, told) = planned(&rig, |_| {});
    assert_eq!(run, None);
    assert!(!told.contains("localhost"), "{told}");
}

#[test]
fn a_cwd_that_leaves_the_worktree_plans_nothing_and_the_instructions_say_why() {
    let escape =
        r#"{"configurations":[{"name":"web","runtimeExecutable":"npm","port":4421,"cwd":".."}]}"#;
    let (run, told) = planned(&project(Some(escape)), |_| {});
    assert_eq!(run, None);
    assert!(told.contains("leaves the worktree"), "{told}");
}

// Records how many dev servers were running while each call ran.
struct Watching {
    inner: FakeClaude,
    servers: FakeDevServers,
    live: Mutex<Vec<usize>>,
}

impl Agents for Watching {
    fn prepare(&self, call: &AgentCall) -> Result<(), AgentError> {
        self.inner.prepare(call)
    }

    fn run(&self, call: &AgentCall, ending: &Ending) -> Result<AgentReply, AgentError> {
        self.live.lock().unwrap().push(self.servers.live());
        self.inner.run(call, ending)
    }
}

// A runner on the stand-in Claude, and what it saw of the dev servers during each call.
fn watching(rig: &Rig) -> (Mutex<crate::runner::Runner>, Arc<Watching>) {
    let watching = Arc::new(Watching {
        inner: rig.claude.clone(),
        servers: rig.dev_servers.clone(),
        live: Mutex::default(),
    });
    let runner = rig.open_agents(watching.clone()).unwrap();
    (runner, watching)
}

fn worker_turn(rig: &Rig, runner: &Mutex<crate::runner::Runner>) -> crate::test::Seen {
    rig.ask(runner, "add", Some("7"));
    let _ = step(runner);
    let seen = rig.claude.all_seen();
    seen.into_iter()
        .find(|s| s.call.role == Role::Worker)
        .expect("a worker call")
}

// Every call ran with one server up, and none is left up after.
fn assert_served_each_call(rig: &Rig, watching: &Watching) {
    let live = watching.live.lock().unwrap();
    assert!(!live.is_empty() && live.iter().all(|&n| n == 1), "{live:?}");
    assert_eq!(rig.dev_servers.started().len(), live.len());
    assert_eq!(rig.dev_servers.live(), 0, "stopped after the call");
}

#[test]
fn the_server_starts_before_the_call_and_stops_after_it() {
    let rig = project(Some(SHEP));
    let (runner, watching) = watching(&rig);
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    worker_turn(&rig, &runner);

    assert_served_each_call(&rig, &watching);
    assert_eq!(rig.dev_servers.started()[0].port, 4421);
}

#[test]
fn the_server_stops_when_the_turn_times_out_or_fails() {
    for error in [
        AgentError::TimedOut(Harness::ClaudeCode),
        AgentError::Failed(Harness::ClaudeCode, "exit 1".into()),
    ] {
        let rig = project(Some(SHEP));
        let (runner, watching) = watching(&rig);
        rig.claude.script([Scripted::Fail(error.clone())]);
        worker_turn(&rig, &runner);

        assert_served_each_call(&rig, &watching);
    }
}

#[test]
fn a_server_that_never_answers_leaves_the_turn_running_and_the_instructions_say_so() {
    let rig = project(Some(SHEP));
    rig.dev_servers.never_answer();
    let (runner, watching) = watching(&rig);
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    let worker = worker_turn(&rig, &runner);

    assert_served_each_call(&rig, &watching);
    let told = instructions_of(&worker);
    assert!(told.contains("had not answered after 60 seconds"), "{told}");
    let log = rig.paths().build(7).join("dev-server.log");
    assert!(told.contains(&log.display().to_string()), "{told}");
    let notes = runner.lock().unwrap().take_notes();
    assert!(
        notes.iter().any(|n| n.contains("had not answered")),
        "{notes:?}"
    );
}

#[test]
fn a_server_that_cannot_start_leaves_the_turn_running_and_the_instructions_say_so() {
    let rig = project(Some(SHEP));
    rig.dev_servers.cannot_start("npm is not installed");
    let (runner, watching) = watching(&rig);
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    let worker = worker_turn(&rig, &runner);

    let live = watching.live.lock().unwrap();
    assert!(!live.is_empty() && live.iter().all(|&n| n == 0), "{live:?}");
    let told = instructions_of(&worker);
    assert!(
        told.contains("could not start the dev server: npm is not installed"),
        "{told}"
    );
}

#[test]
fn a_project_without_a_launch_file_starts_nothing_during_the_turn() {
    let rig = project(None);
    let (runner, watching) = watching(&rig);
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    worker_turn(&rig, &runner);

    assert!(rig.dev_servers.started().is_empty());
    assert!(watching.live.lock().unwrap().iter().all(|&n| n == 0));
}
