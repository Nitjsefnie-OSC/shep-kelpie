//! Which identity reads, through the forge ledger and the fakes' call counts

use std::collections::BTreeMap;

use super::*;
use crate::forge_ledger::{self, Ledger, Line};
use crate::ports::Warnings;
use crate::ports::{Cooldown, ForgeHold};
use crate::test::{FakeClock, FakeForge};

fn repo() -> ForgeSlug {
    ForgeSlug::try_from("shep-pm/koji".to_owned()).unwrap()
}

// Which repos the App covers, and with which permissions.
#[derive(Default)]
struct Covers(Mutex<BTreeMap<String, Reading>>);

impl AppForges for Covers {
    fn reading(&self, repo: &ForgeSlug) -> Reading {
        let covered = self.0.lock().unwrap();
        covered
            .get(repo.as_str())
            .cloned()
            .unwrap_or(Reading::Login)
    }
}

const ALL: [&str; 4] = ["issues", "pull_requests", "checks", "statuses"];

struct Rig {
    held: RateHeld,
    login: FakeForge,
    app: FakeForge,
    clock: FakeClock,
    covers: Arc<Covers>,
    said: Arc<Mutex<Vec<String>>>,
    home: tempfile::TempDir,
}

impl Rig {
    // `koji` is covered by the App, which holds `holds`, when `holds` is some.
    fn new(holds: Option<&[&str]>) -> Self {
        Self::watched(holds, None)
    }

    // As `new`, telling warnings that stand to `standing`.
    fn watched(holds: Option<&[&str]>, standing: Option<Arc<dyn Warnings>>) -> Self {
        let home = tempfile::tempdir().unwrap();
        let login = FakeForge::new(home.path().join("login"));
        let app = FakeForge::new(home.path().join("app"));
        let clock = FakeClock::at(1_000);
        // Pull request 7 is open on both, with a head to read.
        for forge in [&login, &app] {
            forge.open_pull_request(7, "work", &[]);
            forge.set_lagging(7, Some("abc"));
        }
        let covers = Arc::new(Covers::default());
        if let Some(holds) = holds {
            let reading = Reading::App {
                reads: Arc::new(app.clone()),
                holds: holds.iter().map(|&h| h.to_owned()).collect(),
            };
            covers
                .0
                .lock()
                .unwrap()
                .insert(repo().as_str().to_owned(), reading);
        }
        let said = Arc::new(Mutex::new(Vec::new()));
        let told = Arc::clone(&said);
        let held = RateHeld::new(
            Arc::new(login.clone()),
            Arc::new(clock.clone()),
            ForgeHold::default(),
            Box::new(move |line| told.lock().unwrap().push(line)),
        )
        .with_ledger(Arc::new(Ledger::in_home(home.path(), "koji")))
        .with_app_reads(covers.clone());
        let held = match standing {
            Some(standing) => held.with_standing(standing),
            None => held,
        };
        Self {
            held,
            login,
            app,
            clock,
            covers,
            said,
            home,
        }
    }

    fn lines(&self) -> Vec<Line> {
        forge_ledger::read(&self.home.path().join(forge_ledger::FILE)).unwrap()
    }

    fn said(&self) -> Vec<String> {
        self.said.lock().unwrap().clone()
    }
}

#[test]
fn a_covered_projects_reads_go_through_the_app_and_the_ledger_says_so() {
    let rig = Rig::new(Some(&ALL));

    rig.held.ready_issues(&repo()).unwrap();
    rig.held.open_pull_requests(&repo()).unwrap();
    rig.held.pull_request(&repo(), 7).unwrap();

    assert_eq!((rig.app.asked(), rig.login.asked()), (3, 0));
    let seen: Vec<_> = rig
        .lines()
        .iter()
        .map(|l| (l.kind, l.by, l.ended))
        .collect();
    assert_eq!(
        seen,
        [
            (Kind::ReadyIssues, Identity::App, Ended::Ok),
            (Kind::OpenPullRequests, Identity::App, Ended::Ok),
            (Kind::PullRequest, Identity::App, Ended::Ok),
        ]
    );
    assert!(rig.said().is_empty());
}

#[test]
fn an_uncovered_projects_reads_go_through_the_login() {
    let rig = Rig::new(None);

    rig.held.ready_issues(&repo()).unwrap();
    rig.held.pull_request(&repo(), 7).unwrap();

    assert_eq!((rig.app.asked(), rig.login.asked()), (0, 2));
    assert!(rig.lines().iter().all(|l| l.by == Identity::Login));
    assert!(rig.said().is_empty(), "an uncovered repo is not a fault");
}

#[test]
fn what_the_app_does_not_read_stays_on_the_login() {
    let rig = Rig::new(Some(&ALL));

    rig.held.viewer().unwrap();
    rig.held.can_push(&repo()).unwrap();
    rig.held.set_label(&repo(), 7, "ready", true).unwrap();
    // The App's token reaches one repo, and a parent's sub-issues can sit in
    // another, so the list is the login's.
    rig.held.sub_issues(&repo(), 7).unwrap();

    assert_eq!((rig.app.asked(), rig.login.asked()), (0, 4));
}

#[test]
fn a_token_without_a_permission_a_read_needs_leaves_that_read_to_the_login_and_says_which() {
    // An App registered before the checks and statuses permissions existed.
    let rig = Rig::new(Some(&["issues", "pull_requests"]));

    rig.held.pull_request(&repo(), 7).unwrap();
    rig.held.pull_request(&repo(), 7).unwrap();
    rig.held.ready_issues(&repo()).unwrap();

    assert_eq!((rig.app.asked(), rig.login.asked()), (1, 2));
    let said = rig.said();
    assert_eq!(said.len(), 1, "said once: {said:#?}");
    assert!(said[0].contains("checks and statuses"), "{said:#?}");
    assert!(said[0].contains("shep-pm/koji"), "{said:#?}");
}

#[test]
fn a_permission_the_token_lacks_is_said_once_whichever_read_meets_it() {
    let rig = Rig::new(Some(&["issues", "checks", "statuses"]));

    rig.held.open_pull_requests(&repo()).unwrap();
    rig.held.reviewed(&repo(), 7).unwrap();
    rig.held.open_pull_requests(&repo()).unwrap();

    let said = rig.said();
    assert_eq!(said.len(), 1, "said once: {said:#?}");
    assert!(
        said[0].contains("lacks the pull_requests permission"),
        "{said:#?}"
    );
}

// What the runner's notes would hold: the warnings that stand, by key.
#[derive(Default)]
struct Standing(Mutex<BTreeMap<String, String>>);

impl Warnings for Standing {
    fn stands(&self, key: &str, line: String) {
        self.0.lock().unwrap().entry(key.to_owned()).or_insert(line);
    }

    fn ends(&self, key: &str) {
        self.0.lock().unwrap().remove(key);
    }
}

#[test]
fn a_missing_permission_stands_until_the_token_holds_it() {
    let standing = Arc::new(Standing::default());
    let rig = Rig::watched(Some(&["issues"]), Some(standing.clone()));

    rig.held.open_pull_requests(&repo()).unwrap();
    rig.held.pull_request(&repo(), 7).unwrap();

    let keys: Vec<String> = standing.0.lock().unwrap().keys().cloned().collect();
    assert_eq!(
        keys,
        [
            "app-lacks checks and statuses on shep-pm/koji",
            "app-lacks pull_requests on shep-pm/koji"
        ]
    );

    let hold = |holds: &[&str]| {
        let reading = Reading::App {
            reads: Arc::new(rig.app.clone()),
            holds: holds.iter().map(|&h| h.to_owned()).collect(),
        };
        let mut covers = rig.covers.0.lock().unwrap();
        covers.insert(repo().as_str().to_owned(), reading);
    };
    hold(&["issues", "pull_requests"]);
    rig.held.open_pull_requests(&repo()).unwrap();
    let keys: Vec<String> = standing.0.lock().unwrap().keys().cloned().collect();
    assert_eq!(keys, ["app-lacks checks and statuses on shep-pm/koji"]);

    hold(&ALL);
    rig.held.pull_request(&repo(), 7).unwrap();
    assert!(standing.0.lock().unwrap().is_empty());
}

#[test]
fn a_read_the_app_is_refused_for_want_of_a_permission_falls_to_the_login_and_says_which() {
    let rig = Rig::new(Some(&ALL));
    rig.app
        .set_failing("gh: Resource not accessible by integration (HTTP 403)");

    rig.held.pull_request(&repo(), 7).unwrap();
    rig.held.pull_request(&repo(), 7).unwrap();

    assert_eq!((rig.app.asked(), rig.login.asked()), (2, 2));
    let said = rig.said();
    assert_eq!(said.len(), 1, "said once: {said:#?}");
    assert!(
        said[0].contains("pull_requests and checks and statuses"),
        "{said:#?}"
    );
    let seen: Vec<_> = rig.lines().iter().map(|l| (l.by, l.ended)).collect();
    assert_eq!(
        seen[..2],
        [(Identity::App, Ended::Failed), (Identity::Login, Ended::Ok)]
    );
}

#[test]
fn any_other_failure_of_the_app_is_the_reads_answer() {
    let rig = Rig::new(Some(&ALL));
    rig.app.set_down(true);

    let error = rig.held.ready_issues(&repo()).unwrap_err();

    assert_eq!(
        error,
        ForgeError::Failed("error connecting to api.github.com".into())
    );
    assert_eq!(rig.login.asked(), 0);
}

#[test]
fn the_apps_limit_used_up_sends_its_reads_to_the_login_until_it_resets_and_holds_nothing_else() {
    let rig = Rig::new(Some(&ALL));
    rig.app.set_limited(Cooldown {
        retry_after: Some(120),
        reset: None,
    });

    rig.held.ready_issues(&repo()).unwrap();
    rig.held.open_pull_requests(&repo()).unwrap();

    // The first call went to the App and was answered by the login, and
    // the second never asked the App.
    assert_eq!((rig.app.asked(), rig.login.asked()), (1, 2));
    rig.held.viewer().unwrap();
    let said = rig.said();
    assert_eq!(said.len(), 1, "{said:#?}");
    assert!(
        said[0].starts_with("the App's rate limit is used up"),
        "{said:#?}"
    );
    let seen: Vec<_> = rig.lines().iter().map(|l| (l.by, l.ended)).collect();
    assert_eq!(
        seen[..2],
        [(Identity::App, Ended::UsedUp), (Identity::Login, Ended::Ok)]
    );

    rig.app.set_unlimited();
    rig.clock.advance(120);
    rig.held.ready_issues(&repo()).unwrap();
    assert_eq!(rig.app.asked(), 2, "the App reads again once it has reset");
}

#[test]
fn the_logins_limit_used_up_does_not_stop_the_apps_reads() {
    let rig = Rig::new(Some(&ALL));
    rig.login.set_used_up(Some(Timestamp(1_600)));

    rig.held.viewer().unwrap_err();
    rig.held.ready_issues(&repo()).unwrap();

    assert_eq!(rig.app.asked(), 1);
    assert_eq!(
        rig.held.viewer().unwrap_err(),
        ForgeError::Held(Timestamp(1_600))
    );
}

#[test]
fn a_repo_the_app_stops_covering_is_read_by_the_login_from_then_on() {
    let rig = Rig::new(Some(&ALL));
    rig.held.ready_issues(&repo()).unwrap();

    rig.covers.0.lock().unwrap().clear();
    rig.held.ready_issues(&repo()).unwrap();

    assert_eq!((rig.app.asked(), rig.login.asked()), (1, 1));
}
