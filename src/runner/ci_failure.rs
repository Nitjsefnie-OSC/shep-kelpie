//! Whether a work item's failed CI is one its worker could have caught
//!
//! A worker runs the project's tests in its own sandbox before it pushes, so a red
//! run whose failing tests a worker can run is one the worker could have caught.
//! `docs/agents/sandbox-red-tests.md` lists the tests a worker's sandbox cannot
//! run: a run red on those alone is one no worker could have caught, whatever it
//! did. Kelpie reads the end of each failed run's log for a red CI prompt
//! (`super::ci_help`), and the failing test names are taken from that same text.
//!
//! The names are cargo's, in the three places a log carries them: the
//! `test <name> ... FAILED` line each failing test gets as it ends, the
//! `---- <name> stdout ----` block for each one whose output cargo printed, and
//! the list cargo ends a failing run with. The `test result: FAILED. <passed>
//! passed; <failed> failed` line counts the failures, and cargo's `to rerun pass`
//! line names the target they ran in, which tells an integration test's bare name
//! from a module path.
//!
//! A log is the last 6000 bytes of a run (`LOG_BYTES` in `src/adapters/gh/runs.rs`),
//! so a run with many failures has names cut off. Kelpie counts those `runnable`,
//! never `sandbox-only`: it may understate what a worker could not have caught,
//! never overstate what it could.

use std::collections::BTreeSet;

use super::Runner;
use super::ci_help::Logs;
use crate::metrics::CiTests;

/// The modules whose tests a worker's sandbox cannot run, as
/// `docs/agents/sandbox-red-tests.md` lists them, one entry per entry of its
/// table. `the_doc_and_the_list_agree` fails when the two part ways, so a new red
/// module cannot slip past the count. A module covers its child modules, and a
/// `tests/<name>.rs` entry covers every test of that binary.
const SANDBOX_RED: [&str; 39] = [
    "adapters::bridge",
    "adapters::bridge::proxy::tests",
    "adapters::claude::calls",
    "adapters::curl",
    "adapters::dev_server",
    "adapters::forwarder",
    "adapters::goose::adopt_tests",
    "adapters::goose::gateway_tests",
    "adapters::goose::tests",
    "adapters::gpu",
    "adapters::local::endpoint",
    "adapters::paddock",
    "adapters::pi::adopt_tests",
    "adapters::pi::attach_tests",
    "adapters::pi::gateway_tests",
    "adapters::pi::tests",
    "adapters::process",
    "adapters::test_runner",
    "doctor::gateways",
    "dog::door",
    "dog::link",
    "github",
    "issues::tests",
    "runner::attach",
    "runner::flight::carry",
    "runner::gpu_tests",
    "runner::limits_tests",
    "runner::panel::tests::fences",
    "runner::pm::tests::attach",
    "runner::pm::tests::code",
    "runner::review::local",
    "runner::slots_tests",
    "runner::turn::tests::browser",
    "runner::turn::tests::dev_server",
    "runner::turn::tests::held",
    "sheep::handover::tests::goose",
    "sheep::handover::tests::pi",
    "shepherd::connection",
    "tests/upgrade.rs",
];

/// Where a failing test ran, as cargo's own line names it
#[derive(Debug, PartialEq, Eq)]
enum Target {
    /// The crate's own tests, each named by its module path
    Lib,
    /// One `tests/<name>.rs` binary, whose tests are named without the file
    Test(String),
}

/// What a failed run's log says about its tests
#[derive(Debug, Default)]
struct Seen {
    /// Each test the log names as failing, once however many times it appears
    names: BTreeSet<String>,
    /// How many tests failed, as cargo's `test result` lines count them
    failed: usize,
    /// The target those tests ran in, from cargo's last `to rerun pass` line
    target: Option<Target>,
}

/// Whether a work item's failed CI had tests its worker could have run itself
pub(super) fn failed_tests(logs: &str) -> CiTests {
    let seen = read(logs);
    // Nothing the log shows as a failed test: lint, docs, fmt or a build that
    // never got as far as running one
    if seen.names.is_empty() && seen.failed == 0 {
        return CiTests::NoTests;
    }
    // A log is the end of a run. One that counts more failures than it names had
    // names cut off, and kelpie cannot say which modules they were in. A run with
    // names and no count is the same: kelpie cannot say the names are all of them.
    if seen.failed == 0 || seen.names.len() < seen.failed {
        return CiTests::Runnable;
    }
    match (seen.names.iter()).all(|name| sandbox_red(name, &seen.target)) {
        true => CiTests::SandboxOnly,
        false => CiTests::Runnable,
    }
}

// Whether `name` is one of the tests a worker's sandbox cannot run, given the
// target it ran in
fn sandbox_red(name: &str, target: &Option<Target>) -> bool {
    match target {
        // An integration test binary is listed by its file, and its tests carry no
        // module path of their own to match
        Some(Target::Test(binary)) => SANDBOX_RED.contains(&format!("tests/{binary}.rs").as_str()),
        // The crate's own tests, and any target kelpie could not name: cargo's name
        // for one is its module path, which the list covers from its module down
        Some(Target::Lib) | None => SANDBOX_RED.iter().any(|module| {
            name == *module
                || name
                    .strip_prefix(*module)
                    .is_some_and(|rest| rest.starts_with("::"))
        }),
    }
}

// Everything a log says about its tests
fn read(log: &str) -> Seen {
    let mut seen = Seen::default();
    // Whether the line before was cargo's `failures:` heading, which the list of
    // names it ends a failing run with follows
    let mut listing = false;
    for line in log.lines() {
        let text = after_step(line).trim_end();
        if listing {
            match listed_name(text) {
                Some(name) => {
                    seen.names.insert(name.to_owned());
                    continue;
                }
                None => listing = false,
            }
        }
        if text == "failures:" {
            listing = true;
        } else if let Some(name) = block_name(text) {
            seen.names.insert(name.to_owned());
        } else if let Some(name) = run_name(text) {
            seen.names.insert(name.to_owned());
        } else if let Some(failed) = failed_count(text) {
            seen.failed += failed;
        } else if let Some(target) = run_target(text) {
            seen.target = Some(target);
        }
    }
    seen
}

// The log's own text of a line: `gh run view --log-failed` puts the job and the
// step before it, as `failure_tail` keeps them
fn after_step(line: &str) -> &str {
    let mut parts = line.splitn(3, '\t');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(_), Some(_), Some(text)) => text,
        _ => line,
    }
}

// `    <name>`, one name of the list cargo ends a failing run with. The heading is
// followed by a blank line where cargo prints the blocks instead, which is not a
// name and ends the list.
fn listed_name(text: &str) -> Option<&str> {
    let name = text.strip_prefix("    ")?.trim();
    (!name.is_empty()).then_some(name)
}

// `---- <name> stdout ----`, the block cargo prints for a test whose output it showed
fn block_name(text: &str) -> Option<&str> {
    let inner = text.strip_prefix("---- ")?.strip_suffix(" ----")?;
    let (name, kind) = inner.rsplit_once(' ')?;
    matches!(kind, "stdout" | "stderr").then_some(name)
}

// `test <name> ... FAILED`, printed as the test ends
fn run_name(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("test ")?;
    let (name, _) = rest.rsplit_once(" ... FAILED")?;
    (!name.is_empty()).then_some(name)
}

// `test result: FAILED. 4599 passed; 2 failed; 28 ignored; 0 measured; ...`
fn failed_count(text: &str) -> Option<usize> {
    let rest = text
        .strip_prefix("test result: FAILED.")?
        .split_once("passed;")?
        .1;
    let mut words = rest.split_whitespace();
    let failed = words.next()?.parse::<usize>().ok()?;
    words.next()?.starts_with("failed").then_some(failed)
}

// `error: test failed, to rerun pass `--lib``, or the `--test <name>` of one
// integration test binary. Cargo prints it while the run is still writing, so it
// is not the log's last line.
fn run_target(text: &str) -> Option<Target> {
    let flag = text.split_once("to rerun pass `")?.1.strip_suffix('`')?;
    match flag.strip_prefix("--test ") {
        Some(binary) => Some(Target::Test(binary.to_owned())),
        None => (flag == "--lib").then_some(Target::Lib),
    }
}

impl Runner {
    /// Counts a work item's failed CI at `head` once, by whether its worker could
    /// have caught it. `logs` is what kelpie read of its failed runs: a run whose
    /// logs kelpie could not read, or found none of, counts nothing, since kelpie
    /// cannot say what failed there.
    ///
    /// One head counts once. The work item's own record of the head it marked red
    /// and the head it reran says kelpie has already counted it, and both are
    /// saved, so a runner that restarts on a red head does not count it again.
    pub(super) fn count_ci_failure(&mut self, head: &str, logs: &Logs) {
        let Logs::Read(logs) = logs else { return };
        if logs.trim().is_empty() {
            return;
        }
        let counted = {
            let item = self.current().expect("CI runs on a work item");
            item.red_head.as_deref() == Some(head) || item.reran.as_deref() == Some(head)
        };
        if counted {
            return;
        }
        self.metrics.ci_failure(failed_tests(logs));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::Checks;
    use crate::runner::{StepReport, step};
    use crate::test::{Rig, Scripted};

    // The end of a real failed run of this repo, as `failed_run_logs` hands it to a
    // worker: `gh run view --log-failed` on gh 2.96, then `failure_tail` at 6000
    // bytes. Run 38807299334 failed on two tests of `runner::turn::tests::dev_server`,
    // which the doc's table lists as red in a worker's sandbox, and nothing else.
    const SANDBOX_ONLY: &str = include_str!("../../fixtures/gh-run-log-sandbox-only.txt");

    // Run 38967162070 failed on one test of `runner::pm::tests::lease`, which the
    // doc lists no reason for: a worker could have run it.
    const UNLISTED_TEST: &str = include_str!("../../fixtures/gh-run-log-unlisted-test.txt");

    // Run 38950550504 failed in `cargo doc --no-deps --locked`, with no test run at
    // all. The GitHub runner's folder is written `/home/me`, as every fixture writes
    // the home folder.
    const NO_TEST: &str = include_str!("../../fixtures/gh-run-log-no-test.txt");

    // Run 38983239961 failed on one test of the `tests/settings_examples.rs` binary,
    // which the doc lists no reason for. It is the recorded shape of an integration
    // test's log: cargo names the binary in its `to rerun pass` line, and the test
    // carries no module path of its own.
    const INTEGRATION: &str = include_str!("../../fixtures/gh-run-log-integration-test.txt");

    #[test]
    fn a_run_red_only_on_tests_a_worker_cannot_run_is_sandbox_only() {
        assert_eq!(failed_tests(SANDBOX_ONLY), CiTests::SandboxOnly);
    }

    #[test]
    fn a_run_red_on_one_test_a_worker_could_run_is_runnable() {
        assert_eq!(failed_tests(UNLISTED_TEST), CiTests::Runnable);
    }

    #[test]
    fn a_run_that_failed_with_no_test_run_counts_as_none() {
        assert_eq!(failed_tests(NO_TEST), CiTests::NoTests);
    }

    // The same real log with everything above its count cut away, as a run with
    // more failures than this one would leave it: the names are gone, the count is not.
    #[test]
    fn a_run_whose_failing_names_are_cut_off_is_never_sandbox_only() {
        let cut = (SANDBOX_ONLY
            .lines()
            .skip_while(|line| !line.contains("test result: FAILED")))
        .collect::<Vec<_>>()
        .join("\n");
        assert_eq!(failed_tests(&cut), CiTests::Runnable, "{cut}");
    }

    #[test]
    fn a_run_that_names_its_tests_and_counts_them_agrees_with_itself() {
        let seen = read(SANDBOX_ONLY);
        assert_eq!(seen.failed, 2);
        assert_eq!(seen.names.len(), 2);
        assert_eq!(seen.target, Some(Target::Lib));

        let seen = read(UNLISTED_TEST);
        assert_eq!(seen.failed, 1);
        assert_eq!(seen.target, Some(Target::Lib));

        // Run 38983239961: one test of the `tests/settings_examples.rs` binary
        let seen = read(INTEGRATION);
        assert_eq!(seen.failed, 1);
        assert_eq!(seen.names.len(), 1);
        assert_eq!(
            seen.target,
            Some(Target::Test("settings_examples".to_owned()))
        );
    }

    #[test]
    fn a_test_of_a_listed_integration_binary_is_sandbox_only() {
        // The recorded log above with its binary named as `tests/upgrade.rs`, which
        // the doc's table lists. Only the name the log carries is changed here.
        let listed =
            INTEGRATION.replace("pass `--test settings_examples`", "pass `--test upgrade`");
        assert_eq!(failed_tests(&listed), CiTests::SandboxOnly);
        assert_eq!(failed_tests(INTEGRATION), CiTests::Runnable);
    }

    #[test]
    fn a_module_is_listed_from_its_own_tests_down() {
        assert!(sandbox_red("dog::door", &Some(Target::Lib)));
        assert!(sandbox_red(
            "dog::door::e2e::a_call_that_hangs_is_stopped",
            &Some(Target::Lib)
        ));
        assert!(sandbox_red(
            "adapters::pi::tests::a_turn_that_ends",
            &Some(Target::Lib)
        ));
        // `github` is the module, and a name that only starts with it is another
        assert!(sandbox_red("github::a_run_that_needs_a_port", &None));
        assert!(!sandbox_red("github_api::a_test", &None));
        assert!(!sandbox_red(
            "runner::pm::tests::lease::a_long_lease_wait",
            &Some(Target::Lib)
        ));
    }

    #[test]
    fn a_log_with_nothing_in_it_is_no_test_rather_than_a_run_read_wrong() {
        assert_eq!(failed_tests(""), CiTests::NoTests);
        assert_eq!(failed_tests("--- run 9\n"), CiTests::NoTests);
    }

    // The doc's table is what a person keeps true. Its modules and this list are
    // read against each other, so a module added to one and not the other fails here.
    #[test]
    fn the_doc_and_the_list_agree() {
        let doc = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/agents/sandbox-red-tests.md"
        ))
        .unwrap();
        let mut mine = SANDBOX_RED.to_vec();
        mine.sort_unstable();
        assert_eq!(from_the_doc(&doc), mine);
    }

    // Every module and test binary the doc's table names, in order. Only the table
    // counts: the prose around it names tests that pass in a worker too.
    fn from_the_doc(doc: &str) -> Vec<&str> {
        let table = (doc.lines())
            .skip_while(|line| !line.starts_with("| Reason | Modules |"))
            .take_while(|line| line.starts_with('|'))
            .flat_map(|row| row.split('`'))
            .filter(|token| listed_in_the_crate(token))
            .collect::<BTreeSet<_>>();
        table.into_iter().collect()
    }

    // Whether a word the doc sets in backticks names tests of this crate: a
    // `tests/<name>.rs` binary, or a module whose first segment is one of its files
    fn listed_in_the_crate(token: &str) -> bool {
        let crate_root = env!("CARGO_MANIFEST_DIR");
        let is_word = |s: &str| {
            !s.is_empty()
                && s.starts_with(|c: char| c.is_ascii_lowercase())
                && s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        };
        if let Some(binary) = token
            .strip_prefix("tests/")
            .and_then(|t| t.strip_suffix(".rs"))
            && is_word(binary)
        {
            return true;
        }
        let mut segments = token.split("::");
        let Some(first) = segments.next() else {
            return false;
        };
        if !is_word(first) || token.contains("::") && !segments.all(is_word) {
            return false;
        }
        std::path::Path::new(crate_root)
            .join("src")
            .join(format!("{first}.rs"))
            .exists()
            || std::path::Path::new(crate_root)
                .join("src")
                .join(first)
                .join("mod.rs")
                .exists()
    }

    // A work item whose CI is red, sent back for a fix turn with `logs` as its
    // failed runs' logs
    fn red_ci(logs: &str) -> (Rig, std::sync::Mutex<Runner>, String) {
        let (rig, runner, head) = Rig::with_pull_request("koji");
        rig.forge.set_ci_log(logs);
        rig.forge
            .set_checks(&head, Checks::Failed(vec!["test".to_owned()]));
        rig.verdict(&runner);
        (rig, runner, head)
    }

    fn counted(rig: &Rig, tests: &str) -> Option<f64> {
        rig.samples
            .value("kelpie_ci_failures_total", &[("tests", tests)])
    }

    #[test]
    fn a_red_run_on_tests_a_worker_cannot_run_counts_as_sandbox_only() {
        let (rig, _runner, _head) = red_ci(SANDBOX_ONLY);
        assert_eq!(counted(&rig, "sandbox-only"), Some(1.0));
        assert_eq!(counted(&rig, "runnable"), None);
        assert_eq!(counted(&rig, "none"), None);
    }

    #[test]
    fn a_red_run_on_a_test_a_worker_could_have_run_counts_as_runnable() {
        let (rig, _runner, _head) = red_ci(UNLISTED_TEST);
        assert_eq!(counted(&rig, "runnable"), Some(1.0));
        assert_eq!(counted(&rig, "sandbox-only"), None);
    }

    #[test]
    fn a_red_run_with_no_test_in_its_log_counts_as_none() {
        let (rig, _runner, _head) = red_ci(NO_TEST);
        assert_eq!(counted(&rig, "none"), Some(1.0));
        assert_eq!(counted(&rig, "sandbox-only"), None);
    }

    #[test]
    fn the_same_head_red_twice_counts_once_and_a_new_head_counts_again() {
        // The log names no file the branch changed, so kelpie reruns the failed jobs
        // rather than spend a turn, as `gate::rerun` does.
        let (rig, runner, head) = red_ci(UNLISTED_TEST);
        assert!(
            matches!(
                rig.forge.reruns().as_slice(),
                [rerun] if rerun == &head
            ),
            "{:?}",
            rig.forge.reruns()
        );

        // The rerun failed at the same head, which sends the worker back for a fix.
        let report = rig.verdict(&runner);
        assert!(
            matches!(&report, Some(StepReport::CiFailed { head: h, .. }) if h == &head),
            "{report:?}"
        );
        assert_eq!(counted(&rig, "runnable"), Some(1.0), "one head, one count");

        // The worker's fix pushes a new head, which goes red again.
        rig.claude.script([Scripted::Push("fix.txt", "fixed\n")]);
        step(&runner).unwrap();
        let fixed = rig.forge.head_of("kelpie/7").unwrap();
        rig.forge
            .set_checks(&fixed, Checks::Failed(vec!["test".to_owned()]));
        rig.verdict(&runner);
        assert_eq!(counted(&rig, "runnable"), Some(2.0), "{head} then {fixed}");
    }

    #[test]
    fn a_red_run_whose_logs_kelpie_cannot_read_counts_nothing() {
        let (rig, runner, head) = Rig::with_pull_request("koji");
        rig.forge.set_ci_log_error("gh run view failed");
        rig.forge
            .set_checks(&head, Checks::Failed(vec!["test".to_owned()]));
        rig.verdict(&runner);
        assert_eq!(rig.samples.total("kelpie_ci_failures_total"), 0.0);
    }
}
