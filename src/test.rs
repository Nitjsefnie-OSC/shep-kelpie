//! The main seam's rig: a runner on stand-ins for Claude, the forge, the
//! webhook and the clock, over a real git repo in a throwaway home

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::adapters::{GpuCurl, LocalReviewer};
use crate::coderabbit::CodeRabbit;
use crate::codex::Codex;
use crate::cubic::Cubic;
use crate::github::AppVoice;
use crate::home::{ProjectName, ProjectPaths};
use crate::ports::{
    Agents, CLEAN, CallPacer, Checks, Clock, Cost, Forge, LocalPace, Meter, MeterError, Pace,
    Ports, Role, SessionId, Tickets, Timestamp, Usage, Utilization, Window,
};
use crate::review_bot::Profile;
use crate::runner::{CHECKS_SETTLE, OpenError, Runner, SETTLE_LEAST, StepReport, answer, step};
use crate::settings::SettingsError;
use crate::webhook::{KelpieSettings, Webhook};
use crate::work_item::{CallRecord, Known, Phase, Seconds, TimingPhase, Timings, Turn, WorkItem};

mod alerts;
mod classifier;
mod claude;
mod coderabbit;
mod codex;
mod cubic;
mod dev_server;
mod disk;
mod e2e_runner;
mod elsewhere;
mod endpoint;
mod forge;
mod fork;
mod git;
mod github;
mod home;
mod leased;
mod leases;
mod memory;
mod pages;
mod reads;
mod reviewer;
mod rig_settings;
mod samples;
mod sandbox;
mod script;
mod shepherd;
mod tempdir;
mod test_runner;

pub(crate) use alerts::FakeAlerts;
pub(crate) use classifier::{FakeClassifier, SeenAsk};
pub(crate) use claude::{FakeClaude, Hold, LEFT_BEHIND, Scripted, Seen};
pub(crate) use dev_server::FakeDevServers;
pub(crate) use disk::{FakeDisk, GB};
pub(crate) use e2e_runner::FakeE2e;
pub(crate) use elsewhere::Elsewhere;
pub(crate) use endpoint::{Answer, StandInEndpoint, Take, unreachable_url};
pub(crate) use forge::{FakeForge, VIEWER};
pub(crate) use git::{FakeGit, Synced};
pub(crate) use github::{Asked, FakeGithub, FakeSigner, PEM, REVIEW_ID, SentReview};
pub(crate) use home::Home;
pub(crate) use leases::{DogStandIn, FakeLeases, Told};
pub(crate) use memory::Memory;
pub(crate) use pages::{FakePages, clean as clean_visit};
pub(crate) use reads::{
    assert_granted, assert_reads, assert_refuses, installed_sources, sandbox_refuses,
    source_folders, tools_refuse,
};
pub(crate) use reviewer::{FakeReviewer, ScriptedRound, SeenRound};
pub(crate) use samples::RecordedSink;
pub(crate) use sandbox::OpenSandbox;
pub(crate) use script::write_script;
pub(crate) use shepherd::FakeShepherd;
pub(crate) use tempdir::short_tempdir;
pub(crate) use test_runner::FakeTests;

/// Makes an owner-only FIFO at `path`
///
/// Runs `mkfifo(1)`: `rustix` has no `mkfifoat` on macOS, and `nix` would put
/// a second copy of itself in the tree beside the one `shep-core` builds.
pub(crate) fn make_fifo(path: &Path) {
    let made = Command::new("mkfifo")
        .args(["-m", "600"])
        .arg(path)
        .status()
        .unwrap();
    assert!(made.success(), "mkfifo {}: {made}", path.display());
}

/// [`a_work_item`] as an older file's loads: the comments on its pull request
/// are marked as seen
pub(crate) fn a_work_item_from_before_comments() -> WorkItem {
    let mut item = a_work_item();
    item.talked.quiet = true;
    item
}

/// A work item with one call, so every field of its format shows
pub(crate) fn a_work_item() -> WorkItem {
    WorkItem {
        pinned: true,
        turn: Turn::Running {
            since: Timestamp(9),
            prompt: None,
        },
        pull_request: Some(51),
        phase: Phase::Ci {
            head: Some("c0ffee".into()),
            since: Timestamp(11),
        },
        red_head: Some("bad".into()),
        conflict: Some(crate::work_item::Conflict {
            head: "c0ffee".into(),
            main: "a11ce".into(),
            turns: 1,
        }),
        known: Known {
            labels: vec!["review please".into()],
            ready: false,
            head: None,
        },
        timings: Some(Timings {
            created: Timestamp(5),
            since: Timestamp(12),
            seconds: Seconds::of(&[(TimingPhase::Worker, 4), (TimingPhase::Ci, 3)]),
            call: None,
            queued: false,
        }),
        counts: crate::work_item::Counts {
            review_rounds: 2,
            fix_turns: 1,
            rulings: 1,
            ci_fix_turns: 1,
            worker_unreported: 0,
            reviewer_unreported: 0,
            ci_failures: crate::work_item::CiFailures::default(),
        },
        calls: vec![CallRecord {
            role: Role::Worker,
            at: Timestamp(10),
            session: SessionId("5e55".into()),
            usage: Usage {
                input: 1,
                cache_write: 2,
                cache_write_5m: 0,
                cache_read: 3,
                output: 4,
            },
            cost: Cost(5),
            session_cost: Cost(6),
            unpriced: false,
        }],
        ..WorkItem::new(
            ticket(42),
            "Add a thing",
            "kelpie/42",
            "/k/wt/shep/42".into(),
            "/k/targets/shep/42".into(),
            "opus-high".to_owned().try_into().unwrap(),
            SessionId("5e55".into()),
        )
    }
}

/// The `[app.dogs.kelpie]` table of a runner's Flockfile entry, such as
/// `settings.example.toml`, as shep hands it to the runner
pub(crate) fn project_table(entry: &str) -> serde_json::Map<String, serde_json::Value> {
    let parsed: toml::Table = toml::from_str(entry).unwrap();
    match serde_json::to_value(&parsed["app"][0]["dogs"]["kelpie"]).unwrap() {
        serde_json::Value::Object(table) => table,
        other => panic!("the entry's kelpie table is {other}"),
    }
}

/// A runner entry like `settings.example.toml` with `tables` added to its
/// kelpie table, such as an older `[app.dogs.kelpie.review]`
pub(crate) fn with_tables(entry: &str, tables: &str) -> String {
    const PACING: &str = "\n[app.dogs.kelpie.pacing]\n";
    assert!(entry.contains(PACING), "the example's pacing table moved");
    entry.replace(PACING, &format!("\n{tables}{PACING}"))
}

/// The `git.checkout` in `settings.example.toml`, which the rig points at its own
const EXAMPLE_REPO: &str = "~/GitHub/shep";

/// The reviewers `settings.example.toml` lists, commented out
const EXAMPLE_REVIEWERS: &str = "# reviewers = [\"qwen\", \"defect-hunter\"]\n";

/// The rig's reviewers: its qwen round and then its own Claude session, `claude`
pub(crate) const RIG_REVIEWERS: &str = "reviewers = [\"qwen\", \"claude\"]\n";

/// What the rig adds to its settings to run no retro
pub(crate) const NO_RETRO: &str = "\n[app.dogs.kelpie.skills]\nretro = { kind = \"none\" }\n";

// What [`Rig::retro_on`] puts in its place, a retro being off when left out
const RETRO_ON: &str = "\n[app.dogs.kelpie.skills]\nretro = { kind = \"kelpie\" }\n";

/// The rig's own session reviewer, as the older Claude round ran: Sonnet 5
/// at medium, asked for findings in kelpie's format
pub(crate) const CLAUDE_REVIEWER: &str = "---\nrole: reviewer\nharness: claude-code\n\
model: claude-sonnet-5\neffort: medium\n---\nReview the change against {{BASE}} for \
defects. Output one finding per line as SEVERITY|file:line|what|why, or exactly CLEAN.\n";

/// The rig's reviewers with CodeRabbit read last
pub(crate) const RIG_REVIEWERS_AND_CODERABBIT: &str =
    "reviewers = [\"qwen\", \"claude\", \"coderabbit\"]\n";

/// A meter that reports what a test sets, and counts its reads
///
/// It starts with the account idle: nothing used in either window, the
/// week `Rig::EPOCH` begins and the session window five hours long.
#[derive(Debug, Clone)]
pub(crate) struct FakeMeter {
    reading: Arc<Mutex<Result<Utilization, MeterError>>>,
    reads: Arc<AtomicUsize>,
}

impl FakeMeter {
    pub(crate) fn idle() -> Self {
        Self {
            reading: Arc::new(Mutex::new(Ok(Rig::utilization(0, 0)))),
            reads: Arc::default(),
        }
    }

    /// Reports `usage` from now on
    pub(crate) fn set(&self, usage: Utilization) {
        *self.reading.lock().unwrap() = Ok(usage);
    }

    /// Fails every read with `error`, until a test sets a reading
    pub(crate) fn fail(&self, error: MeterError) {
        *self.reading.lock().unwrap() = Err(error);
    }

    /// How many times usage was read
    pub(crate) fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

impl Meter for FakeMeter {
    fn read(&self, _now: Timestamp) -> Result<Utilization, MeterError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.reading.lock().unwrap().clone()
    }
}

/// A clock that moves only when a test moves it
#[derive(Debug, Clone)]
pub(crate) struct FakeClock(Arc<AtomicU64>);

impl FakeClock {
    pub(crate) fn at(seconds: u64) -> Self {
        Self(Arc::new(AtomicU64::new(seconds)))
    }

    pub(crate) fn advance(&self, seconds: u64) {
        self.0.fetch_add(seconds, Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        Timestamp(self.0.load(Ordering::SeqCst))
    }

    fn wait(&self, seconds: u64) {
        self.advance(seconds);
    }
}

/// One project's world: kelpie's home, the maintainer's home, and the
/// project's repo cloned from a bare origin
#[derive(Debug)]
pub(crate) struct Rig {
    pub(crate) home: Home,
    pub(crate) project: ProjectName,
    pub(crate) claude: FakeClaude,
    pub(crate) forge: FakeForge,
    pub(crate) git: FakeGit,
    pub(crate) meter: FakeMeter,
    pub(crate) codex_meter: FakeMeter,
    pub(crate) reviewer: FakeReviewer,
    pub(crate) local_leases: LocalReviewer,
    // The gateway's leases, answered with nothing and counted, in place of the
    // real read, for a test that says who asked
    counted_leases: Arc<Mutex<Option<Arc<leased::LeaseReads>>>>,
    pub(crate) alerts: FakeAlerts,
    pub(crate) disk: FakeDisk,
    pub(crate) leases: FakeLeases,
    pub(crate) tests: FakeTests,
    pub(crate) dev_servers: FakeDevServers,
    pub(crate) e2e: FakeE2e,
    pub(crate) pages: FakePages,
    pub(crate) samples: RecordedSink,
    /// The gateway classifier triage's spam check asks
    pub(crate) classifier: FakeClassifier,
    pub(crate) clock: FakeClock,
    /// The login's forge hold and pace, which no test's calls reach the cap of
    /// until it swaps in a pacer with one
    pub(crate) pace: Arc<dyn Pace>,
    /// GitHub as kelpie's App sees it: the rig has registered no App until
    /// [`Rig::with_app`]
    pub(crate) github: FakeGithub,
}

/// How long a test waits on a call, a socket or a thread that runs on real
/// time before it fails by name
pub(crate) const CALL_PATIENCE: Duration = Duration::from_secs(60);

/// Where every fake clock starts, in seconds since the Unix epoch
pub(crate) const EPOCH: u64 = 1_790_000_000;

impl Rig {
    /// Where every rig's clock starts
    pub(crate) const EPOCH: u64 = EPOCH;

    /// One day, in seconds
    pub(crate) const DAY: u64 = 86_400;

    /// Usage with `week` and `session` percent spent. The week began at
    /// [`Self::EPOCH`] and the session window resets five hours on.
    pub(crate) fn utilization(week: u32, session: u32) -> Utilization {
        Utilization {
            session: Window {
                used_pct: session,
                resets_at: Timestamp(Self::EPOCH + 5 * 3600),
            },
            week: Window {
                used_pct: week,
                resets_at: Timestamp(Self::EPOCH + 7 * Self::DAY),
            },
        }
    }

    /// The repo the rig's project works on GitHub
    pub(crate) const REMOTE: &str = "shep-pm/shep";

    /// Where the rig says the kelpie binary is
    pub(crate) const KELPIE: &str = "/opt/kelpie/bin/kelpie";

    /// The rig's webhook URL: a credential, so finding it anywhere else is a leak
    pub(crate) const WEBHOOK_URL: &str = "https://alerts.example.invalid/hook/kelpie-s3cr3t";

    /// The part of [`Self::WEBHOOK_URL`] no output may carry
    pub(crate) const WEBHOOK_SECRET: &str = "s3cr3t";

    /// A project with the example settings, pointed at a fresh repo
    pub(crate) fn new(project: &str) -> Self {
        let rig = Self::stand_ins(project, Home::made());
        rig.make_repo();

        let example = include_str!("../settings.example.toml");
        assert!(example.contains(EXAMPLE_REPO), "the example's repo moved");
        // Most tests are about what comes after the review, so the rig's
        // project runs a short one: a local round and then a Claude session. A
        // test of a new project's review takes that out with [`Rig::default_review`].
        assert!(
            example.contains(EXAMPLE_REVIEWERS),
            "the example's reviewers moved"
        );
        let settings = example
            .replace(EXAMPLE_REPO, &rig.repo().display().to_string())
            .replace(EXAMPLE_REVIEWERS, RIG_REVIEWERS);
        // A retro is one more call after every batch, so most tests
        // leave it off and a test of the retro turns it on with [`Rig::retro_on`].
        let settings = format!("{settings}{NO_RETRO}");
        // No reviewer reads a head before CI has passed on it, and a run has
        // two minutes to register, so most tests run the project without CI
        // and a test of the gate turns it on with [`Rig::ci_on`].
        let settings = settings.replace("block = true", "block = false");
        let paths = rig.paths();
        std::fs::create_dir_all(&paths.folder).unwrap();
        std::fs::write(rig.settings_file(), settings).unwrap();
        rig.write_agent("claude", CLAUDE_REVIEWER);
        // The maintainer's qwen-review script, which the runner checks is there as it starts.
        let script = rig.home.path().join(".claude/scripts/qwen-review.sh");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        write_script(&script, "#!/bin/sh\nexit 1\n");
        let kelpie = format!(
            "[webhook]\nkind = \"ntfy\"\nurl = \"{}\"\n",
            Self::WEBHOOK_URL
        );
        std::fs::write(rig.kelpie_settings_file(), kelpie).unwrap();
        rig.write_totp_secret();
        rig
    }

    /// The webhook the rig's kelpie settings name
    pub(crate) fn webhook(&self) -> Webhook {
        self.kelpie_settings()
            .webhook
            .expect("the rig's kelpie settings name a webhook")
    }

    /// Kelpie's own settings as the rig's file holds them, empty when it is gone
    pub(crate) fn kelpie_settings(&self) -> KelpieSettings {
        self.try_kelpie_settings().unwrap()
    }

    fn try_kelpie_settings(&self) -> Result<KelpieSettings, SettingsError> {
        match std::fs::read_to_string(self.kelpie_settings_file()) {
            Ok(text) => KelpieSettings::from_section(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(KelpieSettings::default()),
            Err(e) => panic!("cannot read the rig's kelpie settings: {e}"),
        }
    }

    /// Replaces the rig's kelpie settings file with `text`
    pub(crate) fn set_kelpie_settings(&self, text: &str) {
        std::fs::write(self.kelpie_settings_file(), text).unwrap();
    }

    /// Gives kelpie a gateway named `name` at `url`, in its own settings, read
    /// when a runner next opens. Its key's variable is `CARGO_PKG_NAME`, which
    /// every test run has set.
    pub(crate) fn gateway(&self, name: &str, url: &str) {
        let text = std::fs::read_to_string(self.kelpie_settings_file()).unwrap();
        let table = format!("[gateways.{name}]\nurl = \"{url}\"\nkey_env = \"CARGO_PKG_NAME\"\n");
        std::fs::write(self.kelpie_settings_file(), format!("{text}{table}")).unwrap();
    }

    fn make_repo(&self) {
        let root = self.home.path();
        let origin = self.origin();
        git(
            root,
            &[
                "init",
                "--quiet",
                "--bare",
                "--initial-branch=main",
                path(&origin),
            ],
        );
        git(
            root,
            &["clone", "--quiet", path(&origin), path(&self.repo())],
        );
        std::fs::write(self.repo().join("README.md"), "a project\n").unwrap();
        git(&self.repo(), &["add", "README.md"]);
        git(&self.repo(), &["commit", "--quiet", "-m", "first"]);
        git(&self.repo(), &["push", "--quiet", "origin", "main"]);
    }

    fn origin(&self) -> PathBuf {
        self.home.path().join("origin.git")
    }

    /// Lands a commit on origin's `main` from another clone, as a merge
    /// elsewhere would, and returns its hash
    pub(crate) fn land_on_origin(&self, file: &str) -> String {
        self.land(file, "landed elsewhere\n")
    }

    /// Lands `file` holding `text` on origin's `main`, and returns the commit
    pub(crate) fn land(&self, file: &str, text: &str) -> String {
        let other = self.home.path().join("other");
        if !other.exists() {
            git(
                self.home.path(),
                &["clone", "--quiet", path(&self.origin()), path(&other)],
            );
        }
        let at = other.join(file);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
        git(&other, &["add", file]);
        git(&other, &["commit", "--quiet", "-m", "landed elsewhere"]);
        git(&other, &["push", "--quiet", "origin", "main"]);
        git(&other, &["rev-parse", "HEAD"])
    }

    /// Pushes a commit of `file` to `branch` on origin from another clone,
    /// as the maintainer would by hand, and returns its hash
    pub(crate) fn push_by_hand(&self, branch: &str, file: &str) -> String {
        let hand = self.home.path().join("by-hand");
        if !hand.exists() {
            git(
                self.home.path(),
                &["clone", "--quiet", path(&self.origin()), path(&hand)],
            );
        }
        git(&hand, &["fetch", "--quiet", "origin"]);
        let start = match self.forge.head_of(branch) {
            Some(_) => format!("origin/{branch}"),
            None => "origin/main".to_owned(),
        };
        git(&hand, &["checkout", "--quiet", "-B", branch, &start]);
        std::fs::write(hand.join(file), "pushed by hand\n").unwrap();
        git(&hand, &["add", file]);
        git(&hand, &["commit", "--quiet", "-m", file]);
        git(&hand, &["push", "--quiet", "origin", branch]);
        git(&hand, &["rev-parse", "HEAD"])
    }

    /// Asserts the worker reads `path` under the settings of the call `seen`,
    /// and that a commit from its worktree would not carry it
    pub(crate) fn assert_worker_reads(&self, seen: &Seen, path: &Path) {
        assert!(
            !path.starts_with(&seen.call.cwd),
            "a commit would carry {path:?}"
        );
        // The worker's rules name the old home as `~/.kelpie`.
        let home = self.home.path().join("shep/kelpie");
        let as_written = format!("~/.kelpie/{}", path.strip_prefix(&home).unwrap().display());
        let as_is = path.to_str().unwrap();
        let deny = seen.settings["permissions"]["deny"].as_array().unwrap();
        for rule in deny.iter().map(|r| r.as_str().unwrap()) {
            assert!(
                !denies(rule, &as_written) && !denies(rule, as_is),
                "{rule} hides {as_written}"
            );
        }
    }

    /// The project's checkout
    pub(crate) fn repo(&self) -> PathBuf {
        self.home.path().join("repos").join(self.project.as_str())
    }

    /// The file the rig keeps the project's table in, as the Flockfile entry's `[app.dogs.kelpie]`
    pub(crate) fn settings_file(&self) -> PathBuf {
        self.paths().folder.join("settings.toml")
    }

    /// The file the rig keeps kelpie's `[kelpie]` section in
    pub(crate) fn kelpie_settings_file(&self) -> PathBuf {
        self.paths().kelpie_home.join("settings.toml")
    }

    pub(crate) fn paths(&self) -> ProjectPaths {
        // Kelpie's home under the shepherd's, as the runner works it out.
        let shep = self.home.path().join("shep");
        ProjectPaths::under(&shep.join("kelpie"), &shep, &self.project)
    }

    /// Starts a runner, as a restarted sheep would, on the rig's stand-ins
    pub(crate) fn open(&self) -> Result<Mutex<Runner>, OpenError> {
        self.open_with(vec![Arc::new(CodeRabbit), Arc::new(Cubic), Arc::new(Codex)])
    }

    /// Starts a runner that reads each review bot by its profile in `review_bots`
    pub(crate) fn open_with(
        &self,
        review_bots: Vec<Arc<dyn Profile>>,
    ) -> Result<Mutex<Runner>, OpenError> {
        self.open_with_agents(review_bots, Arc::new(self.claude.clone()))
    }

    /// Starts a runner whose calls go to `agents` rather than the stand-in Claude
    pub(crate) fn open_agents(&self, agents: Arc<dyn Agents>) -> Result<Mutex<Runner>, OpenError> {
        let bots: Vec<Arc<dyn Profile>> =
            vec![Arc::new(CodeRabbit), Arc::new(Cubic), Arc::new(Codex)];
        self.open_with_agents(bots, agents)
    }

    fn open_with_agents(
        &self,
        review_bots: Vec<Arc<dyn Profile>>,
        agents: Arc<dyn Agents>,
    ) -> Result<Mutex<Runner>, OpenError> {
        let tokens = Arc::new(self.github.tokens(&self.paths().kelpie_home));
        let forge = Arc::new(self.forge.clone());
        let ports = Ports {
            agents,
            forge: Arc::clone(&forge) as Arc<dyn Forge>,
            tickets: Arc::clone(&forge) as Arc<dyn Tickets>,
            app_reads: None,
            revalidated: crate::ports::Revalidated::default(),
            voice: Arc::new(AppVoice::new(tokens, Box::new(self.github.clone()))),
            meter: Box::new(self.meter.clone()),
            codex_meter: Box::new(self.codex_meter.clone()),
            reviewer: Arc::new(self.reviewer.clone()),
            git: Arc::new(self.git.clone()),
            gpu: Arc::new(GpuCurl),
            disk: Arc::new(self.disk.clone()),
            local_leases: self.lease_port(),
            review_bots,
            alerts: Arc::new(self.alerts.clone()),
            leases: Arc::new(self.leases.clone()),
            test_runner: Arc::new(self.tests.clone()),
            dev_servers: Arc::new(self.dev_servers.clone()),
            e2e_runner: Arc::new(self.e2e.clone()),
            pages: Arc::new(self.pages.clone()),
            metrics: Arc::new(self.samples.clone()),
            classifier: Arc::new(self.classifier.clone()),
            clock: Arc::new(self.clock.clone()),
            pace: Arc::clone(&self.pace),
            flock: Vec::new(),
        };
        let paths = self.paths();
        Runner::open(
            self.project.clone(),
            self.try_settings()?,
            self.try_kelpie_settings()?,
            &paths,
            self.home.path(),
            Path::new(Self::KELPIE),
            ports,
        )
        .map(Mutex::new)
    }

    /// Sends a trigger the way `shep trigger <project> <action>` does
    pub(crate) fn ask(
        &self,
        runner: &Mutex<Runner>,
        action: &str,
        params: Option<&str>,
    ) -> serde_json::Value {
        let postings = runner.lock().unwrap().postings();
        let reply = answer(runner, &postings, action, params);
        runner.lock().unwrap().settle_notices();
        serde_json::from_str(&reply).expect("a JSON reply")
    }

    /// A running project whose worker's first turn pushed `work.txt` on
    /// `kelpie/7` and opened draft pull request 71, with CI not yet reported
    ///
    /// Its review ran each reviewer once, a clean qwen round and then a clean
    /// Claude round, so the gate tests all start where the gate itself
    /// begins: the worker's pull request open and its one pass over.
    ///
    /// Returns the pull request's head.
    pub(crate) fn with_pull_request(project: &str) -> (Self, Mutex<Runner>, String) {
        Self::with_pull_request_set(project, |_| {})
    }

    /// [`Rig::with_pull_request`], after `setup` changed the rig's settings
    pub(crate) fn with_pull_request_set(
        project: &str,
        setup: impl FnOnce(&Self),
    ) -> (Self, Mutex<Runner>, String) {
        let rig = Self::new(project);
        setup(&rig);
        let runner = rig.open().unwrap();
        rig.ask(&runner, "add", Some("7"));
        rig.forge.open_pull_request(71, "kelpie/7", &[7]);
        rig.claude
            .script([Scripted::Push("work.txt", "work\n"), Scripted::Text(CLEAN)]);
        step(&runner).unwrap(); // the worker's first turn: opens the pull request
        step(&runner).unwrap(); // review round 1, qwen: clean by default
        step(&runner).unwrap(); // review round 2, claude: scripted clean above
        let head = rig.forge.head_of("kelpie/7").expect("the worker pushed");
        // The gate begins here, so CI blocks from now on.
        rig.ci_on_now(&runner);
        (rig, runner, head)
    }

    /// [`Rig::with_pull_request`], with CI green and the worker parked on
    /// merge ruling 71 (the pull request's number) about the returned head
    pub(crate) fn parked(project: &str) -> (Self, Mutex<Runner>, String) {
        Self::parked_set(project, |_| {})
    }

    /// [`Rig::parked`], after `setup` changed the rig's settings
    pub(crate) fn parked_set(
        project: &str,
        setup: impl FnOnce(&Self),
    ) -> (Self, Mutex<Runner>, String) {
        let (rig, runner, head) = Self::with_pull_request_set(project, setup);
        rig.forge.set_checks(&head, Checks::Passed);
        assert!(matches!(
            rig.verdict(&runner),
            Some(StepReport::Ruling { id, .. }) if id == ticket(71)
        ));
        (rig, runner, head)
    }

    /// `login` writes `body` on `thread` now, and the comment's id
    pub(crate) fn writes_on(&self, thread: u64, login: &str, body: &str) -> u64 {
        (self.forge.threads).write(thread, login, body, self.clock.now())
    }

    /// Moves the clock on to the board's next read, past the first three
    /// waits of a step that failed
    pub(crate) fn next_look(&self) {
        self.clock.advance(crate::runner::BOARD_POLL.as_secs());
    }

    /// Steps once and, if nothing happened, waits out CI's settling and
    /// steps again: how a CI verdict is reached
    pub(crate) fn verdict(&self, runner: &Mutex<Runner>) -> Option<StepReport> {
        let report = step(runner).unwrap();
        if report.is_some() {
            return report;
        }
        self.clock.advance(CHECKS_SETTLE);
        step(runner).unwrap()
    }

    /// Steps a bot's round from the review it covers the head with: the
    /// step that sees the review reads its threads, and the one
    /// [`SETTLE_LEAST`] on reads them again and lands the round
    pub(crate) fn threads_read(&self, runner: &Mutex<Runner>) -> Option<StepReport> {
        assert_eq!(step(runner).unwrap(), None, "the threads read once");
        self.clock.advance(SETTLE_LEAST);
        step(runner).unwrap()
    }

    /// The worktree kelpie makes for issue 7
    pub(crate) fn worktree_7(&self) -> PathBuf {
        self.paths().worktree(ticket(7))
    }

    /// The build folder kelpie makes for issue 7
    pub(crate) fn build_7(&self) -> PathBuf {
        self.paths().build(ticket(7))
    }
}

// Whether a `Read(...)` deny rule covers `path`, written as the rule writes it
fn denies(rule: &str, path: &str) -> bool {
    let Some(glob) = rule.strip_prefix("Read(").and_then(|r| r.strip_suffix(')')) else {
        return false;
    };
    match glob.strip_suffix("**") {
        Some(folder) if !folder.contains('*') => path.starts_with(folder),
        None if !glob.contains('*') => path == glob,
        _ => panic!("teach this test to read {rule}"),
    }
}

pub(crate) fn write_in(folder: &Path, file: &str, text: &str) {
    let path = folder.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn path(p: &Path) -> &str {
    p.to_str().expect("a UTF-8 temporary path")
}

/// Runs git in `cwd` and returns its trimmed stdout
///
/// The maintainer's own git config never reaches the rig: no signing, no hooks.
pub(crate) fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=rig",
            "-c",
            "user.email=rig@example.invalid",
        ])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "git {args:?} in {}: {stderr}",
        cwd.display()
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Gives `main` in `repo` a commit that sets `file` to `contents`, and moves
/// `origin/main` to it
///
/// Before a branch is cut, it is the base the branch starts from. After, it
/// is `main` moving on.
pub(crate) fn main_commits(repo: &Path, file: &str, contents: &str) {
    let path = repo.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
    git(repo, &["add", file]);
    git(repo, &["commit", "--quiet", "-m", "main moves on"]);
    let tip = git(repo, &["rev-parse", "main"]);
    git(repo, &["update-ref", "refs/remotes/origin/main", &tip]);
}

/// Moves `worktree`'s branch to where `main` stands, the way a branch is cut
pub(crate) fn cut_from_main(worktree: &Path) {
    git(worktree, &["reset", "--quiet", "--hard", "main"]);
}

/// A repo at `home/repo` with one commit on `main`, and a worktree of it at
/// `home/wt` on its own branch, the way kelpie cuts one
pub(crate) fn linked_worktree(home: &Path) -> (PathBuf, PathBuf) {
    let repo = home.join("repo");
    let worktree = home.join("wt");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--quiet", "-b", "main"]);
    git(&repo, &["commit", "--quiet", "--allow-empty", "-m", "init"]);
    git(
        &repo,
        &["worktree", "add", "--quiet", "-b", "wt", path(&worktree)],
    );
    (repo, worktree)
}

/// The ticket of GitHub issue `number`
pub(crate) fn ticket(number: u64) -> crate::ticket::TicketId {
    crate::ticket::TicketId::from_issue(number).expect("a test's issue is not zero")
}
