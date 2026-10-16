//! The project runner
//!
//! One runner per project, run as a sheep under kelpie's own shepherd. It
//! checks the project's settings when it starts, keeps the project's state
//! file, answers the maintainer's triggers, and runs the worker's turns.
//! Every change is saved before it takes effect in memory.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use crate::agents::{Agents, AgentsError};
use crate::board::{LabelError, Skip, agent_label, old_worker_label, pinned};
use crate::forge_ledger::Ledger as ForgeLedger;
use crate::hooks::Hooks;
use crate::local_paths::LocalPaths;
use crate::pacer::Assessment;
use crate::ports::{
    Forge, ForgeError, ForgeHold, Guarded, GuardedVoice, KeychainBlocked, PacedVoice, Ports,
    RateHeld, RecordedVoice, Say, SessionId, Timestamp,
};
use crate::review_bot::{Bot, Profile};
use crate::settings::{
    Account, AgentName, ForgeSlug, Gateways, ListedReviewer, RoleAgents, Settings, SettingsError,
};
use crate::skills::{RepoPlugins, Skills};
use crate::state::{ProjectState, Ruling, StateError, StateStore};
use crate::webhook::{KelpieSettings, Webhook};
use crate::work_item::{ReviewCallState, Seat, Timings, WorkItem, new_session_id};

mod adopt;
#[cfg(test)]
mod agents_tests;
mod alert;
mod attach;
mod briefing;
mod browser;
mod builds;
mod ci_help;
mod claim;
mod claude_files;
mod close_merged;
mod closed;
mod code_graph;
#[cfg(test)]
mod coderabbit;
mod compact;
mod contributions;
mod dependencies;
mod dev_server;
mod dispatch;
mod drain;
mod e2e;
mod effects;
mod finishing;
mod flight;
mod follow_up;
mod friction;
mod gate;
mod gpu;
#[cfg(test)]
mod gpu_tests;
mod guard_hooks;
#[cfg(test)]
mod handover_tests;
#[cfg(test)]
mod hooks_tests;
mod in_flight;
mod instructions;
mod kept;
#[cfg(test)]
mod kept_tests;
mod ledger;
#[cfg(test)]
mod ledger_tests;
mod left;
#[cfg(test)]
mod limits_tests;
mod lineup;
mod login_refresh;
mod looks;
mod main_ci;
mod merge;
mod older_bots;
mod open_checks;
#[cfg(test)]
mod overlap_tests;
mod owner_template;
#[cfg(test)]
mod owner_template_tests;
mod pace;
mod panel;
mod parent;
mod paths;
mod pm;
mod points;
#[cfg(test)]
mod points_tests;
mod pull_request_file;
mod question;
mod refused;
mod replies;
mod report;
mod representative;
mod reread;
mod retro;
mod review;
mod review_bot;
mod reviewers_check;
mod rework;
mod ruling;
#[cfg(test)]
mod several;
mod skip_log;
mod slots;
#[cfg(test)]
mod slots_tests;
mod snapshot;
mod started;
mod step_log;
mod talk;
mod thread_replies;
mod timings;
mod token_wait;
mod transcript_count;
mod triage;
mod trigger;
mod turn;
mod turn_hooks;
mod untrusted;
mod voice;
mod words;
mod worker_files;

pub use crate::coderabbit::LABEL as SUMMON_LABEL;
pub use adopt::AdoptError;
pub use attach::{AttachError, Attaching};
pub use claim::IN_PROGRESS;
#[cfg(test)]
pub(crate) use dependencies::is_bump;
pub use drain::{CallRole, CallRunning, Draining};
pub use finishing::{Run, StartError};
#[cfg(test)]
pub use flight::step;
pub use flight::{CarriedTurn, Pass, advance};
pub use gpu::GpuStatus;
pub use in_flight::Stopping;
pub use ledger::count_stopped;
pub use left::leave as leave_answer;
pub use lineup::{Board, MoveError, Place};
pub use looks::{BOARD_POLL, MAIN_CI_POLL};
pub use main_ci::RedStatus;
pub use merge::DropError;
pub use owner_template::{TemplateSource, source_of};
pub use pace::PacerStatus;
pub use paths::{ProjectName, ProjectNameError, ProjectPaths};
pub use pm::{PmAttaching, PmError, PmStatus};
pub use refused::Refusal;
pub use replies::READ_EVERY;
pub use report::StepReport;
pub use representative::{Post, RepresentError};
pub(crate) use reread::{check_gateway_shapes, check_gateways};
use reviewers_check::{check_bots, check_coderabbit, check_local};
pub use rework::{HUMAN, ReworkError};
pub use ruling::{Action, Answer, RuleError};
pub use step_log::FILE as STEPS_FILE;
pub use timings::{Totals, settle};
use trigger::issue_list;
pub use trigger::{ACTIONS, BudgetStatus, Status, WorkItemStatus, answer, wakes};
pub use trigger::{GateError, WhichItem};
pub use words::{Wants, read_answer};

#[cfg(test)]
pub(crate) use gate::CHECKS_SETTLE;
#[cfg(test)]
pub(crate) use review_bot::SETTLE_LEAST;

/// Why a runner could not start
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// A setting is missing, malformed or does not hold
    Settings(SettingsError),
    /// An agent file cannot be read or used
    Agents(AgentsError),
    /// The forge could not be asked about the project's repo
    Forge(ForgeError),
    /// The state file could not be read
    State(StateError),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Settings(e) => e.fmt(f),
            Self::Agents(e) => e.fmt(f),
            Self::Forge(e) => write!(f, "cannot check the repo on the forge: {e}"),
            Self::State(e) => e.fmt(f),
        }
    }
}

impl core::error::Error for OpenError {}

impl From<SettingsError> for OpenError {
    fn from(e: SettingsError) -> Self {
        Self::Settings(e)
    }
}

impl From<AgentsError> for OpenError {
    fn from(e: AgentsError) -> Self {
        Self::Agents(e)
    }
}

impl From<ForgeError> for OpenError {
    fn from(e: ForgeError) -> Self {
        Self::Forge(e)
    }
}

impl From<StateError> for OpenError {
    fn from(e: StateError) -> Self {
        Self::State(e)
    }
}

/// Why `add` was refused
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddError {
    /// No slot is free under `concurrency.active_items`, counting the items waiting for
    /// one, or one for this issue is open already: the issues of those in
    /// the way
    InFlight(Vec<u64>),
    /// The forge could not show the issue
    Forge(ForgeError),
    /// The issue's `agent:` label cannot be used
    Label(LabelError),
    /// The issue writer is labelling the issue
    Labelling(u64),
    /// `finish` holds the board back, so no new work item opens
    Finishing,
    /// No random session id could be drawn, with the OS's reason
    Session(String),
    /// The work item could not be saved
    State(StateError),
}

impl fmt::Display for AddError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InFlight(issues) if issues.len() == 1 => {
                write!(f, "the work item for {} is in flight", issue_list(issues))
            }
            Self::InFlight(issues) => {
                write!(f, "the work items for {} are in flight", issue_list(issues))
            }
            Self::Forge(e) => write!(f, "cannot read the issue: {e}"),
            Self::Label(e) => e.fmt(f),
            Self::Labelling(issue) => write!(
                f,
                "the issue writer is labelling #{issue}: add it once that call ends"
            ),
            Self::Finishing => f.write_str(
                "the runner is finishing, so it opens no new work item: `shep kelpie start` \
                 lets it pick again",
            ),
            Self::Session(e) => write!(f, "cannot draw a session id: {e}"),
            Self::State(e) => e.fmt(f),
        }
    }
}

impl core::error::Error for AddError {}

/// One project's runner
#[derive(Debug)]
pub struct Runner {
    project: ProjectName,
    settings: Settings,
    // The project's repo on the forge: `git.remote`, or the checkout's
    // `origin` when it is absent, read when the runner starts
    remote: ForgeSlug,
    // The project's extra worker instructions, read once when the runner starts
    extra_instructions: Option<String>,
    // What the repo's owner's `.github` repository holds as a pull request template,
    // read when the runner starts or its settings change
    owner_template: Option<owner_template::OwnerTemplate>,
    // Each step's skill, loaded when the runner starts or its settings change
    skills: Skills,
    // The plugins the repo turns on, loaded with them
    repo_plugins: RepoPlugins,
    // The default branch's commit the plugins were read at
    plugins_head: Option<String>,
    paths: ProjectPaths,
    kelpie: PathBuf,
    store: StateStore,
    state: ProjectState,
    ports: Ports,
    // The pacer's last reading of each account's usage and when, kept in memory only
    pacing: BTreeMap<Account, (Timestamp, Assessment)>,
    // What the board passed over on its last poll, kept in memory only
    skipped: Vec<Skip>,
    // The issues triage found untouched on its last poll, kept in memory only
    triage_due: Vec<triage::DueStatus>,
    // Issues the verb queued for triage, ahead of `triage_due`, kept in memory only
    triage_queue: Vec<u64>,
    // What keeps each issue's triage from finishing, kept in memory only
    triage_tries: BTreeMap<u64, triage::comment::Tries>,
    // Each adopted pull request that keeps failing to start, kept in memory only
    adopt_failing: BTreeMap<u64, adopt::Failing>,
    // The forge's refusals in a row to close a done parent, kept in memory only
    close_refused: parent::Refused,
    // The parents off the ready list that built nothing, kept in memory only
    unbuilt: parent::Unbuilt,
    // The review's reviewers, in order, from the project's list
    lineup: Vec<ListedReviewer>,
    // The reviewers a contribution's pass walks, in order. None while the
    // project picks no contributions.
    contribution_lineup: Vec<ListedReviewer>,
    // The project's implementers
    agents: RoleAgents,
    // Every agent kelpie's files define, from which each turn runs its work item's agent
    book: Agents,
    // The maintainer's home folder, for `~/` in kelpie's own settings
    home: PathBuf,
    // The GPU's last reading, from the page kelpie's settings name
    gpu: gpu::GpuWatch,
    // The gateways the adapters were built with, which a change waits to replace
    gateways: Gateways,
    // None when kelpie's settings set no webhook
    webhook: Option<Webhook>,
    // The last failed webhook post, kept in memory so a restart tries at once
    retry: Option<alert::Retry>,
    // Reading the webhook's topic for replies, kept in memory only
    reading: replies::Reading,
    // What a reply's code is checked against, on an ntfy webhook
    totp: Option<replies::Authenticator>,
    // Reading the rulings' threads for answers, kept in memory only
    threads: thread_replies::Reading,
    // The account kelpie acts as, read once a run when a rework first needs it
    viewer: Option<String>,
    // The issue of the work item a step or trigger is working on, set before
    // anything reads it: every change to a work item goes to this one
    focus: Option<u64>,
    // What last did something in a step, kept in memory only, so the next
    // step starts with the one after it
    last_acted: Option<turn::Slot>,
    // What the runner carried on without, kept in memory only until
    // `take_notes` hands it out
    notes: claim::Notes,
    // The steps logged in the middle of a pass, such as a `session-started`
    // of a call just started, kept in memory only until `take_started` hands
    // them out
    started: Vec<StepReport>,
    // The calls this process has in flight, kept in memory only
    flights: flight::Flights,
    // The browsers opened for turns about to launch, kept in memory only
    opened: browser::Opened,
    // The dev servers planned for those turns, kept in memory only
    planned: dev_server::Planned,
    // Where each call is recorded as it ends
    ledger: crate::usage::Ledger,
    // The project's series on shep's metrics endpoint, which the ledger feeds too
    metrics: Arc<crate::metrics::Recorder>,
    steps: step_log::StepLog,
    // What the board briefing keeps between writes, in memory only
    brief: briefing::BoardCache,
    // What the board withholds, as the forge does
    local: LocalPaths,
    // The project's own hooks, which the forge port, the workers' guard and
    // the handover run
    hooks: Hooks,
    // What the notice points have told, in memory only
    told_points: points::Told,
    // The project manager's wakes and decisions, in memory only
    pm: pm::Desk,
    // The panel's votes on the rulings, in memory only
    panel: panel::Desk,
    // What the batch retro keeps between passes, in memory only
    retro_book: retro::Book,
    // Whether `drain` holds back every new call, in memory only, so a
    // restart ends it
    draining: bool,
    // What `finish` keeps in memory, beside the state file's `finishing`
    finish: finishing::Finish,
    // What the answers folder held that was no answer, in memory only
    left: left::Seen,
    // When each work item parked on a merge ruling last read the review bots
    // its pass went on without, kept in memory only
    late_reads: BTreeMap<u64, Timestamp>,
    // When each work item parked with no pull request last read its issue,
    // kept in memory only
    parked_reads: BTreeMap<u64, Timestamp>,
    // When the board was last read and each failing step runs again, in memory only
    looks: looks::Looks,
    // The open pull requests the pass in progress listed, in memory only
    snapshot: snapshot::Snapshot,
    // Why the stored settings are refused while they are, in memory only
    refusal: Option<refused::Refusal>,
    // Until when every forge call is held, the forge's rate limit used up
    forge_hold: ForgeHold,
    // Counts the writes kelpie made, so a pass can tell one was made since it began
    forge_ledger: Arc<ForgeLedger>,
    // The login `gh` acts as, which a ruling mentions when `git.maintainer` is unset,
    // asked once a run, in memory only
    gh_login: std::cell::OnceCell<String>,
    // Whether a ruling posted with no one to mention has been told, in memory only
    unmentioned_told: std::cell::Cell<bool>,
    // The posts to a contribution that failed, until one works or is given up, in memory only
    post_tries:
        std::cell::RefCell<std::collections::BTreeMap<String, contributions::post::Attempt>>,
    // The pull request, head and id of a post the forge took and the state did not keep, until
    // it does, in memory only
    post_unsaved: std::cell::RefCell<Vec<(u64, String, contributions::post::Posted)>>,
    // Whether a worker's turn waits for a token GitHub is not giving, in memory only
    token_wait: token_wait::TokenWait,
    // When each work item began waiting for a bot's lease, in memory only
    lease_waits: review_bot::lease_wait::LeaseWaits,
    // The turns waiting for a model host that cannot be reached, in memory only
    unreachable: turn::unreachable::Unreachable,
    // Why each work item's due turn did not start on its last step, in memory only
    held_up: turn::held_up::HeldUps,
    // The build folders and the disk as last read while a turn ran, in memory only
    builds: builds::BuildWatch,
    // The ids of comments kelpie posted as the login `gh` acts as since the
    // last save, which the state file keeps: none of them is ever an answer
    posted: std::cell::RefCell<Vec<u64>>,
}

impl Runner {
    /// Checks the project's settings hold, and reads its state
    ///
    /// `settings` and `kelpie_settings` are as [`crate::settings::source::load`]
    /// read them. `kelpie` is the kelpie binary, which each worker's file-tool
    /// hook runs. No forge post may name `home`, kelpie's home or the checkout.
    ///
    /// # Errors
    ///
    /// [`OpenError`] naming the setting, forge call or file that failed.
    pub fn open(
        project: ProjectName,
        settings: Settings,
        kelpie_settings: KelpieSettings,
        paths: &ProjectPaths,
        home: &Path,
        kelpie: &Path,
        mut ports: Ports,
    ) -> Result<Self, OpenError> {
        let checkout = settings.git.checkout.as_path();
        let local = LocalPaths::new([home, paths.kelpie_home.as_path(), checkout]);
        let forge_hold = ForgeHold::shared(Arc::clone(&ports.pace));
        let told = claim::Notes::default();
        let gpu = gpu::GpuWatch::start(
            Arc::clone(&ports.gpu),
            kelpie_settings.gpu_metrics_url.clone(),
        );
        let store = StateStore::new(paths.state.clone());
        let book = Agents::load(&paths.agents)?;
        let (book, mut notes) = kept::keep_old_agents(&store, &paths.agents, book)?;
        notes.extend(book.skipped());
        notes.extend(worker_files::remove_old(&paths.worker));
        notes.extend(e2e::record(settings.e2e.as_ref(), &paths.e2e()));
        let now = ports.clock.now();
        notes.extend(review::prune_at_start(
            &settings.local_rounds,
            &paths.name(),
            &paths.folder,
            now,
        ));
        notes.extend(e2e::dog_cannot_find(settings.e2e.as_ref(), paths));
        told.extend(notes);
        let agents = settings.role_agents(&book)?;
        let lineup = settings.lineup(&book, home)?;
        let gateways = kelpie_settings.gateways();
        check_gateways(&gateways, &book, &agents, &lineup)?;
        let webhook = kelpie_settings.webhook;
        if webhook.is_none() {
            eprintln!(
                "kelpie's settings set no webhook, so rulings reach you only in the log, \
                 `status` and `shep kelpie rule`"
            );
        }
        let totp = replies::authenticator(webhook.as_ref(), &paths.totp)?;
        let remote = check_checkout(&settings)?;
        if let Some(reason) = settings.rulings.refusal() {
            return Err(SettingsError::Invalid {
                setting: "rulings.pm_alone",
                reason,
            }
            .into());
        }
        let contribution_lineup =
            open_checks::contributions(&settings, ports.voice.as_ref(), &remote, &book, home)?;
        open_checks::triage(&settings, ports.voice.as_ref(), &remote, &book)?;
        // What the project's `publish` hooks judge is every text bound for the forge.
        let metrics = Arc::new(crate::metrics::Recorder::new(Arc::clone(&ports.metrics)));
        let hooks = Hooks::new(project.as_str(), remote.as_str(), checkout, &settings.hooks)
            .counted_by(Arc::clone(&metrics));
        let clock = Arc::clone(&ports.clock);
        let say_to = |told: &claim::Notes| -> Say {
            let told = told.clone();
            Box::new(move |line| told.push(line))
        };
        // The guard sits outside the hold, so its own questions to the forge are held too.
        let forge_ledger = Arc::new(ForgeLedger::in_home(&paths.kelpie_home, project.as_str()));
        let mut rate_held = RateHeld::new(
            ports.forge,
            Arc::clone(&clock),
            forge_hold.clone(),
            say_to(&told),
        )
        .with_ledger(Arc::clone(&forge_ledger))
        .counting(ports.revalidated.clone())
        .paced_for(project.as_str());
        if let Some(app_reads) = ports.app_reads.take() {
            rate_held = rate_held
                .with_app_reads(app_reads)
                .with_standing(Arc::new(told.clone()));
        }
        let held: Arc<dyn Forge> = Arc::new(rate_held);
        let recorded =
            RecordedVoice::new(ports.voice, Arc::clone(&clock), Arc::clone(&forge_ledger));
        let paced = PacedVoice::new(
            Arc::new(recorded),
            clock,
            forge_hold.clone(),
            project.as_str(),
            say_to(&told),
        );
        let voice = GuardedVoice::new(Arc::new(paced), local.clone())
            .with_flock(ports.flock.clone(), Arc::clone(&held))
            .with_hooks(hooks.clone());
        ports.voice = Arc::new(voice);
        let guarded = Guarded::new(held, local.clone())
            .with_flock(ports.flock.clone())
            .with_hooks(hooks.clone());
        ports.forge = Arc::new(guarded);
        let voice = Arc::clone(&ports.voice);
        ports.agents = Arc::new(KeychainBlocked::new(ports.agents, voice, remote.clone()));
        let extra_instructions = instructions::read_extra(&settings)?;
        let (owner_template, note) =
            owner_template::refreshed(ports.forge.as_ref(), remote.owner(), None);
        told.extend(note);
        let env_home = std::env::var_os("HOME").map(PathBuf::from);
        guard_hooks::check(
            &settings,
            env_home.as_deref(),
            std::env::var_os("PATH").as_deref(),
        )?;
        check_bots(&lineup, &ports)?;
        crate::skills::check(&settings.skills, &paths.skills)?;
        let skills = Skills::load(&settings.skills, &paths.skills);
        let repo_plugins = RepoPlugins::load(
            checkout,
            &paths.kelpie_home,
            &paths.skills,
            ports.git.as_ref(),
            &settings.repo_setup,
        );
        let plugins_head = crate::skills::default_head(checkout);
        check_coderabbit(&remote, &lineup, &ports)?;
        check_local(&lineup, &ports)?;
        let mut state = store.load()?.unwrap_or_default();
        // A review call in flight when the runner stopped never resumes on
        // its own, unlike a turn: nothing reruns review_step to naturally
        // clear it, so a restart clears it here instead of leaving it stuck
        // running forever and refusing every later drop.
        let cut_short =
            |item: &WorkItem| matches!(item.review_call, ReviewCallState::Running { .. });
        if state.work_items.iter().any(cut_short) {
            for item in state.work_items.iter_mut().filter(|item| cut_short(item)) {
                item.call_ended();
            }
            store.save(&state)?;
        }
        // A new run is a new epoch, and the dog reclaims what the old one held.
        if !state.leases.is_empty() {
            state.leases.clear();
            store.save(&state)?;
        }
        if timings::reload(&mut state, ports.clock.now()) {
            store.save(&state)?;
        }
        if triage::settle_since(&mut state, &settings, ports.clock.now()) {
            store.save(&state)?;
        }
        let spoken = points::Told::of(&state);
        metrics.features(&settings, dev_server::is_on(&settings));
        metrics.work_items(&state);
        let mut runner = Self {
            project,
            settings,
            remote,
            extra_instructions,
            owner_template,
            skills,
            repo_plugins,
            plugins_head,
            paths: paths.clone(),
            kelpie: kelpie.to_owned(),
            store,
            state,
            ports,
            pacing: BTreeMap::new(),
            skipped: Vec::new(),
            triage_due: Vec::new(),
            triage_queue: Vec::new(),
            triage_tries: BTreeMap::new(),
            adopt_failing: BTreeMap::new(),
            close_refused: parent::Refused::new(),
            unbuilt: parent::Unbuilt::new(),
            lineup,
            contribution_lineup,
            agents,
            book,
            home: home.to_owned(),
            gpu,
            gateways,
            webhook,
            retry: None,
            reading: replies::Reading::default(),
            totp,
            threads: thread_replies::Reading::default(),
            viewer: None,
            focus: None,
            last_acted: None,
            notes: told,
            started: Vec::new(),
            flights: flight::Flights::default(),
            opened: browser::Opened::default(),
            planned: dev_server::Planned::default(),
            ledger: crate::usage::Ledger::in_folder(&paths.folder).sending(Arc::clone(&metrics)),
            metrics,
            steps: step_log::StepLog::in_folder(&paths.folder),
            brief: briefing::BoardCache::default(),
            local,
            hooks,
            told_points: spoken,
            pm: pm::Desk::default(),
            panel: panel::Desk::default(),
            retro_book: retro::Book::default(),
            draining: false,
            finish: finishing::Finish::default(),
            left: left::Seen::default(),
            late_reads: BTreeMap::new(),
            parked_reads: BTreeMap::new(),
            looks: looks::Looks::default(),
            snapshot: snapshot::Snapshot::default(),
            refusal: None,
            forge_hold,
            forge_ledger,
            gh_login: std::cell::OnceCell::new(),
            unmentioned_told: std::cell::Cell::new(false),
            post_tries: std::cell::RefCell::default(),
            post_unsaved: std::cell::RefCell::default(),
            token_wait: token_wait::TokenWait::default(),
            lease_waits: review_bot::lease_wait::LeaseWaits::default(),
            unreachable: turn::unreachable::Unreachable::default(),
            held_up: turn::held_up::HeldUps::default(),
            builds: builds::BuildWatch::default(),
            posted: std::cell::RefCell::default(),
        };
        runner.settle_older_bots()?;
        runner.settle_labels();
        runner.make_agent_labels();
        runner.brief_now();
        Ok(runner)
    }

    /// The project's settings, as read when the runner started
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Kelpie's gateways, as read when the runner started
    pub fn gateways(&self) -> &Gateways {
        &self.gateways
    }

    /// The project's repo on the forge, as read when the runner started
    pub fn remote(&self) -> &ForgeSlug {
        &self.remote
    }

    // The profile of `bot`, which a start checks every listed bot has.
    fn profile(&self, bot: Bot) -> std::sync::Arc<dyn Profile> {
        let found = self.ports.review_bots.iter().find(|p| p.bot() == bot);
        std::sync::Arc::clone(found.expect("a listed review bot has a profile"))
    }

    // The review bots the project lists, in its order.
    fn listed_bots(&self) -> Vec<crate::review_bot::BotReviewer> {
        self.lineup.iter().filter_map(ListedReviewer::bot).collect()
    }

    /// A log line for each step whose skill could not load
    pub fn skill_notices(&self) -> impl Iterator<Item = String> + '_ {
        self.skills.notices().chain(self.repo_plugins.notices())
    }

    fn names(&self) -> Names {
        Names {
            now: self.ports.clock.now(),
        }
    }

    /// The project's state as `status` reports it
    pub fn status(&self) -> Status<'_> {
        let now = self.ports.clock.now();
        Status {
            project: self.project.as_str(),
            merging: self.settings.git.merging,
            work_item: self
                .state
                .work_items
                .first()
                .map(|item| self.item_status(item, now)),
            work_items: self
                .state
                .work_items
                .iter()
                .map(|item| self.item_status(item, now))
                .collect(),
            active_items: self.settings.concurrency.active_items.get(),
            working: self.issues_where(|i| !i.parked() && i.seat != Seat::Waiting),
            waiting_for_slot: self.issues_where(|i| i.seat == Seat::Waiting),
            parked: self.parked_issues(),
            pending_rulings: self.settings.concurrency.pending_rulings,
            adopted: &self.state.adopted,
            contributions: self.followed_status(now),
            bumps: self.bumps_status(),
            triage: self.triage_status(),
            order: &self.state.order[..self.state.order.len().min(trigger::STATUS_ORDER)],
            skipped: &self.skipped,
            standing: self.notes.standing_lines(),
            red_workflows: self.red_status(now),
            rulings: (self.state.rulings.iter())
                .map(trigger::RulingStatus)
                .collect(),
            panel: self.panel_status(),
            leases: &self.state.leases,
            history: &self.state.history[self
                .state
                .history
                .len()
                .saturating_sub(trigger::STATUS_HISTORY)..],
            pacer: self.pacer_status(now),
            skills: self.skills.status(),
            plugins: self.repo_plugins.status(),
            pull_request_template: self.template_source(),
            gpu: self.gpu.status(),
            disk_free_bytes: self.builds.free_bytes(),
            local_leases: self.local_leases(),
            board: &self.paths.board,
            pm: self.pm_status(),
            build: crate::upgrade::seen::running(),
            draining: self.draining_status(),
            run: self.run_status(),
            forge_held_until: self.forge_hold.until(now),
            settings_refused: self.refusal.as_ref(),
        }
    }

    fn item_status<'a>(&self, item: &'a WorkItem, now: Timestamp) -> WorkItemStatus<'a> {
        let split = item.split(now, self.timing_phase(item));
        WorkItemStatus {
            waiting: self.model_wait(item.issue),
            held_up: self.held_up.of(item.issue),
            build_bytes: self.builds.folder_bytes(item.issue),
            ..WorkItemStatus::new(item, self.settings.budget.per_item_usd, split)
        }
    }

    /// Opens a work item for `issue`, and returns the implementer its
    /// worker runs on. Its first turn runs on a later pass.
    ///
    /// # Errors
    ///
    /// [`AddError`] when the runner is finishing, the project has
    /// `concurrency.active_items` working or one for this issue open
    /// already, the issue cannot be read or its `agent:` label names no
    /// listed implementer, or the change cannot be saved. Nothing changes then.
    pub fn add(&mut self, issue: u64) -> Result<AgentName, AddError> {
        if !self.picks() {
            return Err(AddError::Finishing);
        }
        if self.state.item(issue).is_some() {
            return Err(AddError::InFlight(vec![issue]));
        }
        if self.flights.labelling() == Some(issue) {
            return Err(AddError::Labelling(issue));
        }
        if !self.slot_free() {
            return Err(AddError::InFlight(self.slot_issues()));
        }
        let found = self
            .ports
            .forge
            .issue(&self.remote, issue)
            .map_err(AddError::Forge)?;
        let (agent, note) = self
            .labelled_agent(issue, &found.labels)
            .map_err(AddError::Label)?;
        let session = new_session_id().map_err(|e| AddError::Session(e.to_string()))?;
        let mut next = self.state.clone();
        let pinned = pinned(&found.labels);
        let mut item = self.fresh(issue, found.title, agent.clone(), session);
        item.pinned = pinned;
        next.work_items.push(item);
        Self::label_ruled(&mut next, issue);
        self.save(next).map_err(AddError::State)?;
        self.notes.extend(note);
        self.mark_held(issue, true);
        Ok(agent)
    }

    // The implementer an issue with `labels` runs on: the one its `agent:`
    // label names, or the default. Beside it, the log line an old `worker:`
    // label gets once the work item opens, since it no longer picks anything.
    fn labelled_agent(
        &self,
        issue: u64,
        labels: &[String],
    ) -> Result<(AgentName, Option<String>), LabelError> {
        let listed = self.agents.implementer_names();
        if let Some(agent) = agent_label(labels, &listed)? {
            return Ok((agent, None));
        }
        let agent = self.agents.default_implementer.name.clone();
        let note = old_worker_label(labels).map(|old| {
            format!(
                "issue #{issue} is labelled `{old}`, which kelpie no longer reads, so it runs \
                 on the default implementer, {agent}: an `agent:<name>` label picks another \
                 that `agents.implementers` lists"
            )
        });
        Ok((agent, note))
    }

    // A work item on `kelpie/<issue>` whose first turn is due, with nothing
    // recorded yet
    fn fresh(&self, issue: u64, title: String, agent: AgentName, session: SessionId) -> WorkItem {
        WorkItem {
            timings: Some(Timings::starting(self.ports.clock.now())),
            ..WorkItem::new(
                issue,
                title,
                format!("kelpie/{issue}"),
                self.paths.worktree(issue),
                self.paths.build(issue),
                agent,
                session,
            )
        }
    }

    // The work item the runner is working on, while it is open
    pub(super) fn current(&self) -> Option<&WorkItem> {
        self.state.item(self.focus?)
    }

    // The work item the runner is working on, in a state about to be saved
    pub(super) fn current_in<'a>(&self, next: &'a mut ProjectState) -> Option<&'a mut WorkItem> {
        next.item_mut(self.focus?)
    }

    // Works on the work item for `issue` from here on
    pub(super) fn on(&mut self, issue: Option<u64>) -> &mut Self {
        self.focus = issue;
        self
    }

    fn save(&mut self, mut next: ProjectState) -> Result<(), StateError> {
        slots::seat(&self.state, &mut next, self.item_slots());
        self.charge(&mut next);
        self.note_changes(&mut next);
        // A ruling that is answered takes the place read to in its thread with
        // it, and so does one raised again under its number.
        let kept = |id: &u64| {
            let raised_again = |r: &Ruling| {
                (self.state.rulings.iter())
                    .find(|old| old.id == r.id)
                    .is_some_and(|old| !old.is_raised_with(r))
            };
            next.rulings.iter().any(|r| r.id == *id && !raised_again(r))
        };
        next.heard.retain(|id, _| kept(id));
        next.unmentioned.retain(|id, _| kept(id));
        // The comments kelpie posted since the last save go in with it.
        let fresh = self.posted.take();
        next.note_posted(&fresh);
        if let Err(e) = self.store.save(&next) {
            self.posted.borrow_mut().extend(fresh);
            return Err(e);
        }
        if looks::board_moved(&self.state, &next) {
            self.looks.board_moved();
        }
        self.threads
            .settled(&self.state, &next, self.ports.clock.now());
        self.state = next;
        self.metrics.work_items(&self.state);
        Ok(())
    }
}

/// How [`check_checkout`]'s refusal of a checkout with no `origin` ends
pub(crate) const NO_ORIGIN: &str = "has no `origin` remote to cut branches from";

/// The project's repo on the forge, once `git.checkout` is found to be a
/// git checkout with an `origin`, which the runner needs to open
///
/// The repo is `git.remote`, or the GitHub repo `origin` names when that
/// is absent.
///
/// # Errors
///
/// [`SettingsError::Invalid`] naming `git.checkout` and what is wrong with
/// it, or `git.remote` when it is absent and `origin` is not a GitHub repo.
pub(crate) fn check_checkout(settings: &Settings) -> Result<ForgeSlug, SettingsError> {
    let repo = &settings.git.checkout;
    let invalid = |reason: String| SettingsError::Invalid {
        setting: "git.checkout",
        reason,
    };
    if !repo.is_dir() {
        return Err(invalid(format!("{} is not a folder", repo.display())));
    }
    let git = |args: &[&str]| {
        crate::worktree::in_repo(repo)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|e| invalid(format!("cannot run git to check it: {e}")))
    };
    let output = git(&["rev-parse", "--is-inside-work-tree"])?;
    if !output.status.success() || output.stdout.trim_ascii() != b"true" {
        return Err(invalid(format!(
            "{} is not a git work tree",
            repo.display()
        )));
    }
    // Every work item's branch is cut from `origin/main`.
    let origin = git(&["remote", "get-url", "origin"])?;
    if !origin.status.success() {
        return Err(invalid(format!("{} {NO_ORIGIN}", repo.display())));
    }
    if let Some(remote) = &settings.git.remote {
        return Ok(remote.clone());
    }
    let url = String::from_utf8_lossy(&origin.stdout).trim().to_owned();
    crate::flock::forge_of(&url).ok_or_else(|| SettingsError::Invalid {
        setting: "git.remote",
        reason: "it is not set, and the checkout's `origin` is not a GitHub repo over HTTPS \
                 or SSH: set it to the repo as `owner/name`"
            .to_owned(),
    })
}

/// Whether `worker.instructions_file`, when set, can be read, which the runner needs to open
///
/// # Errors
///
/// [`SettingsError::Invalid`] naming `worker.instructions_file`.
pub(crate) fn check_instructions(settings: &Settings) -> Result<(), SettingsError> {
    instructions::read_extra(settings).map(drop)
}

// What a ruling's question names: when it is raised.
#[derive(Debug, Clone)]
struct Names {
    now: Timestamp,
}

#[cfg(test)]
mod tests;
