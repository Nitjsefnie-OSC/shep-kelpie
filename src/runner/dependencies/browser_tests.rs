//! A web bump is proved in the local browser where the project has one

use std::sync::Mutex;

use super::tests::{bump, notes, poll, settle, words};
use crate::ports::{Checks, E2eOutcome};
use crate::runner::Runner;
use crate::test::Rig;

// Shep's own launch file: one configuration on port 4421 in `web`.
const LAUNCH: &str = include_str!("../../../fixtures/launch.json");

const OFF: &str = "[app.dogs.kelpie.browser]\nenabled = false\nheaded = false\ndomains = []";

// A project with the browser on, its tools installed, `LAUNCH` on `main` and `routes` set,
// and a web bump, #50, whose checks passed.
fn browsing(routes: &str) -> (Rig, Mutex<Runner>, String) {
    browsing_with(routes, Some(LAUNCH))
}

// The same, with `launch` as `main`'s launch file, if it has one.
fn browsing_with(routes: &str, launch: Option<&str>) -> (Rig, Mutex<Runner>, String) {
    let rig = Rig::new("shep");
    rig.edit_settings(|s| {
        assert!(s.contains(OFF), "the example's browser moved");
        s.replace(
            OFF,
            &format!(
                "[app.dogs.kelpie.browser]\nenabled = true\nheaded = true\n\
                 domains = [\"app.example.com\"]\nroutes = {routes}"
            ),
        )
    });
    let tools = rig.paths().tools;
    std::fs::create_dir_all(tools.playwright_mcp().parent().unwrap()).unwrap();
    std::fs::write(tools.playwright_mcp(), "").unwrap();
    std::fs::create_dir_all(tools.browsers().join("chromium-1200")).unwrap();
    if let Some(launch) = launch {
        rig.land(".claude/launch.json", launch);
    }
    let (rig, runner) = with_bumps(rig);
    let head = bump(&rig, 50, "web/package-lock.json");
    rig.forge.set_checks(&head, Checks::Passed);
    (rig, runner, head)
}

fn with_bumps(rig: Rig) -> (Rig, Mutex<Runner>) {
    rig.edit_settings(|s| {
        format!("{s}\n[app.dogs.kelpie.dependencies]\npick = \"on\"\nweb = [\"web/\"]\n")
    });
    let runner = rig.open().unwrap();
    (rig, runner)
}

fn standing(rig: &Rig, runner: &Mutex<Runner>) -> String {
    let listed = super::tests::bumps(rig, runner);
    listed[0]["standing"].as_str().unwrap().to_owned()
}

#[test]
fn a_web_bump_whose_routes_load_clean_is_merged_with_no_e2e_host_needed() {
    let (rig, runner, head) = browsing("[\"/\", \"/settings\"]");
    rig.e2e.script([E2eOutcome::NoHost]);

    settle(&rig, &runner);
    poll(&rig, &runner);

    let asked = rig.pages.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0].urls,
        ["http://localhost:4421/", "http://localhost:4421/settings"]
    );
    assert_eq!(asked[0].gate, Some(4421), "the dev server answered first");
    assert_eq!(rig.forge.merges(), [(50, head)]);
    let notes = notes(&runner);
    assert_eq!(words(&notes, "bump #50: browser passed"), 1, "{notes:?}");
}

#[test]
fn the_dev_server_runs_in_the_bumps_checkout_in_a_sandbox_and_stops_after_the_proof() {
    let (rig, runner, _) = browsing("[\"/\"]");

    settle(&rig, &runner);

    let started = rig.dev_servers.started();
    assert_eq!(started.len(), 1);
    let run = &started[0];
    let worktree = rig.paths().bump_worktree(50);
    assert_eq!(run.cwd, worktree.join("web"));
    assert_eq!(run.port, 4421);
    assert_eq!(
        run.program, "sh",
        "the install comes first, then the server"
    );
    assert!(
        run.args[1].contains("npm ci --ignore-scripts"),
        "{:?}",
        run.args
    );
    assert_eq!(run.args[3], worktree.join("web").display().to_string());
    assert_eq!(run.args[4..8], ["npm", "run", "dev", "--"]);
    assert_eq!(run.policy.write, [worktree, rig.paths().bump_build(50)]);
    assert_eq!(run.policy.hosts, ["app.example.com", "registry.npmjs.org"]);
    assert_eq!(
        run.policy.read,
        [rig.paths().bump_worktree(50), rig.paths().bump_build(50)],
        "its own checkout and build folder are read back out of the fence on kelpie's home"
    );
    assert!(run.policy.listen);
    assert!(!run.policy.no_read.is_empty(), "credentials are fenced");
    assert_eq!(rig.dev_servers.live(), 0, "stopped when the proof ended");
    assert_eq!(
        run.gate.get(),
        None,
        "and the browser's right to its port shut"
    );
    let asked = rig.pages.asked();
    assert_eq!(asked[0].session.folder, rig.paths().bump_browser(50));
}

#[test]
fn a_route_with_a_request_answering_5xx_is_red_naming_the_route_and_the_status_only() {
    let (rig, runner, _) = browsing("[\"/\", \"/items\"]");
    rig.pages.answers("/items", 503);

    settle(&rig, &runner);
    poll(&rig, &runner);

    assert!(rig.forge.merges().is_empty());
    assert!(
        rig.e2e.ran().is_empty(),
        "the e2e run waits on the browser's pass"
    );
    let comments = rig.forge.comments();
    assert_eq!(comments.len(), 1);
    let body = &comments[0].1;
    assert!(
        body.contains("route `/items` had a request answer 503"),
        "{body}"
    );
    assert!(!body.contains("localhost"), "{body}");
    assert!(!body.contains("/api/items"), "{body}");
    assert_eq!(rig.forge.pull_request_closings(), [50]);
    assert_eq!(rig.forge.created().len(), 1);
    let notes = notes(&runner);
    assert_eq!(
        words(&notes, "bump #50: browser proof failed"),
        1,
        "{notes:?}"
    );
}

#[test]
fn a_console_error_on_a_route_is_red() {
    let (rig, runner, _) = browsing("[\"/\"]");
    rig.pages.logs_errors("/", 2);

    settle(&rig, &runner);
    poll(&rig, &runner);

    assert!(rig.forge.merges().is_empty());
    let comments = rig.forge.comments();
    assert!(
        comments[0].1.contains("route `/` logged 2 console errors"),
        "{comments:?}"
    );
    assert_eq!(rig.forge.pull_request_closings(), [50]);
}

#[test]
fn a_page_that_does_not_load_is_red_and_its_reason_is_not_repeated() {
    let (rig, runner, _) = browsing("[\"/\"]");
    let mut visit = crate::test::clean_visit();
    visit.loaded = Err("net::ERR_CONNECTION_REFUSED at http://localhost:4421/".to_owned());
    rig.pages.script("/", visit);

    settle(&rig, &runner);
    poll(&rig, &runner);

    let comments = rig.forge.comments();
    assert!(
        comments[0].1.contains("route `/` did not load"),
        "{comments:?}"
    );
    assert!(!comments[0].1.contains("ERR_CONNECTION"), "{comments:?}");
}

#[test]
fn a_5xx_from_another_origin_does_not_fail_a_route() {
    let (rig, runner, head) = browsing("[\"/\"]");
    let mut visit = crate::test::clean_visit();
    visit.requests.push(crate::ports::PageRequest {
        method: "GET".to_owned(),
        url: "https://app.example.com/api".to_owned(),
        status: Some(502),
    });
    visit.console_errors.push(crate::ports::ConsoleError {
        text: "Failed to load resource: the server responded with a status of 502 (Bad Gateway)"
            .to_owned(),
        url: Some("https://app.example.com/api".to_owned()),
    });
    rig.pages.script("/", visit);
    rig.e2e.script([E2eOutcome::NoHost]);

    settle(&rig, &runner);
    poll(&rig, &runner);

    assert_eq!(rig.forge.merges(), [(50, head)]);
}

#[test]
fn a_resource_of_the_dev_server_that_fails_to_load_is_red() {
    let (rig, runner, _) = browsing("[\"/\"]");
    rig.pages.fails_to_load("/", "dev-origin/logo.png");

    settle(&rig, &runner);
    poll(&rig, &runner);

    assert!(rig.forge.merges().is_empty());
    assert!(
        rig.forge.comments()[0]
            .1
            .contains("route `/` logged a console error")
    );
}

#[test]
fn a_project_with_both_proves_a_bump_in_the_browser_and_then_on_the_e2e_host() {
    let (rig, runner, head) = browsing("[\"/\"]");

    settle(&rig, &runner);
    poll(&rig, &runner);

    assert_eq!(rig.pages.asked().len(), 1);
    assert_eq!(rig.e2e.ran().len(), 1);
    assert_eq!(rig.forge.merges(), [(50, head)]);
}

#[test]
fn a_project_with_both_is_red_when_the_e2e_run_fails_after_the_browser_passed() {
    let (rig, runner, _) = browsing("[\"/\"]");
    rig.e2e.script([E2eOutcome::Failed {
        output: "1 failed\n".to_owned(),
    }]);

    settle(&rig, &runner);
    poll(&rig, &runner);

    assert_eq!(rig.pages.asked().len(), 1);
    assert!(rig.forge.merges().is_empty());
    assert!(rig.forge.comments()[0].1.contains("its e2e run failed"));
    assert_eq!(rig.forge.pull_request_closings(), [50]);
}

#[test]
fn a_project_with_only_an_e2e_host_opens_no_browser() {
    let rig = Rig::new("shep");
    let (rig, runner) = with_bumps(rig);
    let head = bump(&rig, 50, "web/package-lock.json");
    rig.forge.set_checks(&head, Checks::Passed);

    settle(&rig, &runner);
    poll(&rig, &runner);

    assert!(rig.pages.asked().is_empty());
    assert!(rig.dev_servers.started().is_empty());
    assert_eq!(rig.e2e.ran().len(), 1);
    assert_eq!(rig.forge.merges(), [(50, head)]);
}

#[test]
fn a_project_with_neither_leaves_the_bump_open_and_status_says_why() {
    let rig = Rig::new("shep");
    let (rig, runner) = with_bumps(rig);
    rig.e2e.script([E2eOutcome::NoHost]);
    let head = bump(&rig, 50, "web/package-lock.json");
    rig.forge.set_checks(&head, Checks::Passed);

    settle(&rig, &runner);
    poll(&rig, &runner);

    assert!(rig.pages.asked().is_empty());
    assert!(rig.forge.merges().is_empty());
    assert!(standing(&rig, &runner).contains("no e2e host"));
    assert!(
        rig.forge.pull_request_closings().is_empty(),
        "it is not red"
    );
}

#[test]
fn a_project_with_the_browser_on_and_no_launch_file_proves_on_its_e2e_host_alone() {
    let (rig, runner, head) = browsing_with("[\"/\"]", None);

    settle(&rig, &runner);
    poll(&rig, &runner);

    assert!(rig.pages.asked().is_empty());
    assert!(rig.dev_servers.started().is_empty());
    assert_eq!(rig.e2e.ran().len(), 1);
    assert_eq!(rig.forge.merges(), [(50, head)]);
}

#[test]
fn a_launch_file_that_cannot_be_read_leaves_the_bump_open_and_status_says_why() {
    let (rig, runner, _) = browsing_with("[\"/\"]", Some("not json"));

    settle(&rig, &runner);

    assert!(standing(&rig, &runner).contains("not a launch file"));
    assert!(rig.e2e.ran().is_empty());
    assert!(rig.forge.merges().is_empty());
}

#[test]
fn a_dev_server_that_does_not_answer_leaves_the_bump_open_and_is_not_started_again() {
    let (rig, runner, _) = browsing("[\"/\"]");
    rig.dev_servers.never_answer();

    settle(&rig, &runner);
    poll(&rig, &runner);
    poll(&rig, &runner);

    assert!(rig.pages.asked().is_empty());
    assert!(rig.forge.merges().is_empty());
    assert_eq!(rig.dev_servers.started().len(), 1, "one try for a head");
    assert!(standing(&rig, &runner).contains("had not answered"));
    assert!(
        rig.forge.pull_request_closings().is_empty(),
        "it is not red"
    );
}

#[test]
fn a_browser_that_cannot_be_driven_is_tried_again_at_the_next_poll() {
    let (rig, runner, head) = browsing("[\"/\"]");
    rig.pages.cannot_open("Chromium crashed");

    settle(&rig, &runner);
    assert!(standing(&rig, &runner).contains("could not be driven"));
    assert!(rig.forge.merges().is_empty());

    rig.pages.can_open();
    poll(&rig, &runner);
    poll(&rig, &runner);

    assert_eq!(rig.pages.asked().len(), 2);
    assert_eq!(rig.forge.merges(), [(50, head)]);
}
