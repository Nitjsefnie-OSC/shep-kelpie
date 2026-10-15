//! A settings change reaching a running runner
//!
//! Every setting takes effect live, at the next step or call that reads
//! it, except `git.checkout`, `git.remote`, `hooks` and kelpie's gateways: a
//! work item's worktree and pull request belong to the first two, and the
//! forge and voice ports hold the hooks built at start, as the adapters hold
//! the gateways, so those wait for the runner's next start. The agent files
//! are read again with them, and a change to those alone is a change too,
//! but agents that name a gateway the runner lacks wait with it. A change is
//! checked as a start checks it, and one that fails keeps the settings the
//! runner has.

use super::{OpenError, Runner, check_bots, check_coderabbit, check_local, instructions};
use crate::agents::Agents;
use crate::review_bot::Bot;
use crate::settings::{Gateways, Git, ListedReviewer, RoleAgents, Settings, SettingsError};
use crate::skills::{RepoPlugins, Skills, default_head};
use crate::webhook::{KelpieSettings, Webhook};

impl Runner {
    /// Takes `settings` and kelpie's own as the runner's, from its next step
    ///
    /// Returns a line for the log naming what changed and what waits for
    /// the next start, or nothing when nothing changed.
    ///
    /// # Errors
    ///
    /// [`OpenError`] naming the setting that does not hold. Nothing changes then.
    pub fn reread(
        &mut self,
        mut settings: Settings,
        kelpie: KelpieSettings,
    ) -> Result<Option<String>, OpenError> {
        // The owner's template does not depend on the settings, so it is read whatever they say.
        let (owner_template, note) = super::owner_template::refreshed(
            self.ports.forge.as_ref(),
            self.remote.owner(),
            self.owner_template.take(),
        );
        self.owner_template = owner_template;
        self.notes.extend(note);
        let mut waiting = Vec::new();
        if settings.git.checkout != self.settings.git.checkout {
            waiting.push("git.checkout");
            settings
                .git
                .checkout
                .clone_from(&self.settings.git.checkout);
        }
        if settings.git.remote != self.settings.git.remote {
            waiting.push("git.remote");
            settings.git.remote.clone_from(&self.settings.git.remote);
        }
        // The forge and voice ports hold the hooks built at start.
        if settings.hooks != self.settings.hooks {
            waiting.push("hooks");
            settings.hooks.clone_from(&self.settings.hooks);
        }
        // The start checks cover the App and the agent files against `pick`.
        if settings.contributions != self.settings.contributions {
            waiting.push("contributions");
            settings
                .contributions
                .clone_from(&self.settings.contributions);
        }
        // The start checks cover the App and the triager's file against `pick`.
        if settings.triage != self.settings.triage {
            waiting.push("triage");
            settings.triage.clone_from(&self.settings.triage);
        }
        // Merging a bot's pull request begins only at a start.
        if settings.dependencies != self.settings.dependencies {
            waiting.push("dependencies");
            settings
                .dependencies
                .clone_from(&self.settings.dependencies);
        }
        // The adapters were built with the gateways at start.
        if !kelpie.gateways().iter().eq(self.gateways.iter()) {
            waiting.push("gateways");
        }
        if let Some(reason) = settings.rulings.refusal() {
            return Err(SettingsError::Invalid {
                setting: "rulings.pm_alone",
                reason,
            }
            .into());
        }
        // The start refuses a browser this machine cannot give, so a reread does too.
        settings
            .browser
            .supported()
            .map_err(|reason| SettingsError::Invalid {
                setting: "browser.enabled",
                reason,
            })?;
        if settings.rulings != self.settings.rulings {
            self.notes.extend(settings.rulings.unused_under_always());
        }
        let gpu_metrics_url = kelpie.gpu_metrics_url.clone();
        let mut book = Agents::load(&self.paths.agents)?;
        let mut agents = settings.role_agents(&book)?;
        let mut lineup = settings.lineup(&book, &self.home)?;
        // The gateways must hold as a start checks them.
        check_gateways(&kelpie.gateways(), &book, &agents, &lineup)?;
        // Agents the adapters cannot route yet wait with the gateways they name.
        if check_gateways(&self.gateways, &book, &agents, &lineup).is_err() {
            waiting.push("agents");
            settings.agents.clone_from(&self.settings.agents);
            book.clone_from(&self.book);
            // The agents the runner has, under the new settings as a start reads them.
            agents = settings.role_agents(&book)?;
            lineup = settings.lineup(&book, &self.home)?;
        }
        let webhook = kelpie.webhook;
        let mut changed = changed((&self.settings, &self.webhook), (&settings, &webhook));
        let (was, now) = (&self.settings.agents, &settings.agents);
        let writer =
            (was.issue_writer != now.issue_writer) || (was.fallback_after != now.fallback_after);
        if agents != self.agents || lineup != self.lineup || book != self.book || writer {
            changed.push("agents");
        }
        if gpu_metrics_url != self.gpu.url() {
            changed.push("gpu_metrics_url");
        }
        // The default branch may have turned a plugin on or off since the last read.
        // Planned here and written once the checks below hold, so a reread that
        // fails leaves the plugin folders the runner is using as they were.
        let repo_plugins = RepoPlugins::plan(
            &self.settings.git.checkout,
            &self.paths.kelpie_home,
            &self.paths.skills,
            self.ports.git.as_ref(),
            &settings.repo_setup,
        );
        let plugins_head = default_head(&self.settings.git.checkout);
        if repo_plugins != self.repo_plugins {
            changed.push("repo plugins");
        }
        if changed.is_empty() && waiting.is_empty() {
            self.plugins_head = plugins_head;
            return Ok(None);
        }
        let extra_instructions = instructions::read_extra(&settings)?;
        check_bots(&lineup, &self.ports)?;
        let listed = |lineup: &[ListedReviewer]| {
            // parity: review-bot.private-repos
            let coderabbit = |r: &ListedReviewer| r.bot().is_some_and(|b| b.bot == Bot::Coderabbit);
            lineup.iter().any(coderabbit)
        };
        if listed(&lineup) && !listed(&self.lineup) {
            check_coderabbit(&self.remote, &lineup, &self.ports)?;
        }
        if lineup != self.lineup {
            check_local(&lineup, &self.ports)?;
        }
        crate::skills::check(&settings.skills, &self.paths.skills)?;
        let skills = (settings.skills != self.settings.skills)
            .then(|| Skills::load(&settings.skills, &self.paths.skills));
        let new_implementers = agents.implementer_names() != self.agents.implementer_names();
        self.settings = settings;
        self.metrics.features(&self.settings);
        self.lineup = lineup;
        self.agents = agents;
        if new_implementers {
            self.make_agent_labels();
        }
        if book != self.book {
            self.notes.extend(book.skipped());
        }
        self.book = book;
        self.gpu.point_at(gpu_metrics_url);
        self.extra_instructions = extra_instructions;
        self.webhook = webhook;
        // Read again under the new pacing settings at the next look.
        self.pacing.clear();
        let mut line = String::from("settings changed");
        if !changed.is_empty() {
            line.push_str(&format!(": {} now in effect", changed.join(", ")));
        }
        if !waiting.is_empty() {
            line.push_str(&format!(
                "; {} from the runner's next start",
                waiting.join(" and ")
            ));
        }
        if let Some(skills) = skills {
            for notice in skills.notices() {
                line.push_str(&format!("\n{notice}"));
            }
            self.skills = skills;
        }
        self.plugins_head = plugins_head;
        if repo_plugins != self.repo_plugins {
            let repo_plugins = repo_plugins.install(&self.paths.skills);
            for notice in repo_plugins.notices() {
                line.push_str(&format!("\n{notice}"));
            }
            self.repo_plugins = repo_plugins;
        }
        Ok(Some(line))
    }
}

impl Runner {
    /// Loads the repo's plugins again if its default branch moved since they
    /// were read, which settings of kelpie's own do not announce
    ///
    /// Returns a line for the log when the plugins changed.
    pub fn reread_plugins(&mut self) -> Option<String> {
        let checkout = &self.settings.git.checkout;
        if default_head(checkout) == self.plugins_head {
            return None;
        }
        let planned = RepoPlugins::plan(
            checkout,
            &self.paths.kelpie_home,
            &self.paths.skills,
            self.ports.git.as_ref(),
            &self.settings.repo_setup,
        );
        // The plan fetched the branch, so this is the commit it read.
        self.plugins_head = default_head(checkout);
        if planned == self.repo_plugins {
            return None;
        }
        let planned = planned.install(&self.paths.skills);
        let mut line = String::from("the default branch changed: repo plugins now in effect");
        for notice in planned.notices() {
            line.push_str(&format!("\n{notice}"));
        }
        self.repo_plugins = planned;
        Some(line)
    }
}

// Whether `gateways` route every listed agent that names one.
pub(crate) fn check_gateways(
    gateways: &Gateways,
    book: &Agents,
    agents: &RoleAgents,
    lineup: &[ListedReviewer],
) -> Result<(), SettingsError> {
    let listed = agents.implementers.iter().map(|i| &i.name);
    let reviewers = lineup.iter().map(|r| &r.name);
    let experts = (agents.advisor.iter().chain(agents.researcher.iter())).map(|e| &e.name);
    gateways.check_listed(book, listed.chain(reviewers).chain(experts))
}

// A project's settings and the webhook.
type Reach<'a> = (&'a Settings, &'a Option<Webhook>);

// The settings that differ, by key or by table. `webhook` also changes
// when kelpie's own settings do. Both `Settings` are taken apart without
// `..`, so a field added to one is a compile error until this decides what
// `reread` does with it: list it here, or leave it out with a reason.
fn changed((old, was): Reach<'_>, (new, now): Reach<'_>) -> Vec<&'static str> {
    let Settings {
        git: old_git,
        ci: old_ci,
        concurrency: old_concurrency,
        budget: old_budget,
        agents: _, // `reread` compares the agents it resolves, with the files
        pacing: old_pacing,
        worker: old_worker,
        researcher: old_researcher,
        rulings: old_rulings,
        contributions: _, // waits for the next start, when the runner checks it
        dependencies: _,  // waits for the next start, like `contributions`
        triage: _,        // waits for the next start, like `contributions`
        hooks: _,         // waits for the next start, so `reread` keeps the old
        skills: old_skills,
        repo_setup: old_repo_setup,
        code_graph: old_code_graph,
        browser: old_browser,
        local_rounds: old_local_rounds,
        builds: old_builds,
        e2e: _, // waits for the next start, when the runner writes it for the dog
    } = old;
    let Settings {
        git: new_git,
        ci: new_ci,
        concurrency: new_concurrency,
        budget: new_budget,
        agents: _,
        pacing: new_pacing,
        worker: new_worker,
        researcher: new_researcher,
        rulings: new_rulings,
        contributions: _,
        dependencies: _,
        triage: _,
        hooks: _,
        skills: new_skills,
        repo_setup: new_repo_setup,
        code_graph: new_code_graph,
        browser: new_browser,
        local_rounds: new_local_rounds,
        builds: new_builds,
        e2e: _,
    } = new;
    let Git {
        checkout: _, // waits for the next start
        remote: _,   // waits for the next start
        merging: old_merging,
        issues: old_issues,
        maintainer: old_maintainer,
    } = old_git;
    let Git {
        checkout: _,
        remote: _,
        merging: new_merging,
        issues: new_issues,
        maintainer: new_maintainer,
    } = new_git;
    [
        ("git.merging", old_merging != new_merging),
        ("git.issues", old_issues != new_issues),
        ("git.maintainer", old_maintainer != new_maintainer),
        ("ci", old_ci != new_ci),
        ("concurrency", old_concurrency != new_concurrency),
        ("budget", old_budget != new_budget),
        ("pacing", old_pacing != new_pacing),
        ("worker", old_worker != new_worker),
        ("skills", old_skills != new_skills),
        ("repo_setup", old_repo_setup != new_repo_setup),
        ("code_graph", old_code_graph != new_code_graph),
        ("browser", old_browser != new_browser),
        ("local_rounds", old_local_rounds != new_local_rounds),
        ("builds", old_builds != new_builds),
        ("researcher", old_researcher != new_researcher),
        ("rulings", old_rulings != new_rulings),
        ("webhook", was != now),
    ]
    .into_iter()
    .filter_map(|(name, differs)| differs.then_some(name))
    .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::ports::Visibility;
    use crate::settings::{Filing, GatewayName, HookPoint, Merging};
    use crate::test::Rig;
    use crate::webhook::{KelpieSettings, WebhookKind};

    fn settings_with(rig: &Rig, edit: impl FnOnce(String) -> String) -> crate::settings::Settings {
        rig.edit_settings(edit);
        rig.settings()
    }

    #[test]
    fn a_changed_setting_takes_effect_at_once() {
        let rig = Rig::new("shep");
        let runner = rig.open().unwrap();
        let next = settings_with(&rig, |s| {
            s.replace("merging = \"ask\"", "merging = \"auto\"")
                .replace("issues = \"ask\"", "issues = \"skip\"")
                .replace("fix_attempts = -1", "fix_attempts = 2")
                .replace("kickoff_hours = 8", "kickoff_hours = 6")
                .replace("active_items = 1", "active_items = 2")
        });
        let mut runner = runner.lock().unwrap();
        let line = runner.reread(next, rig.kelpie_settings()).unwrap();
        assert_eq!(
            line.as_deref(),
            Some(
                "settings changed: git.merging, git.issues, ci, concurrency, pacing now in effect"
            )
        );
        assert_eq!(runner.settings().concurrency.active_items.get(), 2);
        assert_eq!(runner.settings().git.merging, Merging::Auto);
        assert_eq!(runner.settings().git.issues, Filing::Skip);
        assert_eq!(runner.settings().ci.fix_attempts.cap(), Some(2));
        assert_eq!(runner.settings().pacing.kickoff_hours.get(), 6);
        assert_eq!(runner.status().merging, Merging::Auto);
    }

    #[test]
    fn a_changed_researcher_table_takes_effect_at_once() {
        let rig = Rig::new("shep");
        let runner = rig.open().unwrap();
        let next = settings_with(&rig, |s| {
            format!("{s}\n[app.dogs.kelpie.researcher]\nallowed_domains = [\"docs.rs\"]\n")
        });
        let mut runner = runner.lock().unwrap();
        let line = runner.reread(next, rig.kelpie_settings()).unwrap();
        assert_eq!(
            line.as_deref(),
            Some("settings changed: researcher now in effect")
        );
        assert_eq!(runner.settings().researcher.allowed_domains.len(), 1);
    }

    #[test]
    fn a_changed_budget_takes_effect_at_once() {
        let rig = Rig::new("shep");
        let runner = rig.open().unwrap();
        let next = settings_with(&rig, |s| s.replace("per_item_usd = 10", "per_item_usd = 4"));
        let mut runner = runner.lock().unwrap();
        assert_eq!(runner.settings().budget.per_item_usd.cents(), 1000);
        let line = runner.reread(next, rig.kelpie_settings()).unwrap();
        assert_eq!(
            line.as_deref(),
            Some("settings changed: budget now in effect")
        );
        assert_eq!(runner.settings().budget.per_item_usd.cents(), 400);
    }

    #[test]
    fn a_changed_rulings_table_takes_effect_at_once() {
        let rig = Rig::new("shep");
        let runner = rig.open().unwrap();
        let next = settings_with(&rig, |s| {
            format!(
                "{s}\n[app.dogs.kelpie.rulings]\ndecide = \"panel\"\npm_alone = [\"still-red\"]\n"
            )
        });
        let mut runner = runner.lock().unwrap();
        let line = runner.reread(next, rig.kelpie_settings()).unwrap();
        assert_eq!(
            line.as_deref(),
            Some("settings changed: rulings now in effect")
        );
        assert_eq!(
            runner.settings().rulings.decide,
            crate::settings::Decide::Panel
        );
    }

    #[test]
    fn a_reread_under_always_names_the_rulings_settings_it_ignores() {
        let rig = Rig::new("shep");
        let runner = rig.open().unwrap();
        let next = settings_with(&rig, |s| {
            format!(
                "{s}\n[app.dogs.kelpie.rulings]\ndecide = \"always\"\n\
                 pm_alone = [\"still-red\"]\nfull_panel_paths = [\"src/**\"]\n"
            )
        });
        let mut runner = runner.lock().unwrap();
        runner.take_notes();
        runner.reread(next, rig.kelpie_settings()).unwrap();
        let notes = runner.take_notes().join("\n");
        assert!(
            notes.contains("rulings.decide is \"always\"")
                && notes.contains("rulings.pm_alone")
                && notes.contains("rulings.full_panel_paths"),
            "{notes}"
        );
    }

    #[test]
    fn a_reread_under_always_with_neither_setting_says_nothing_of_them() {
        let rig = Rig::new("shep");
        let runner = rig.open().unwrap();
        let next = settings_with(&rig, |s| {
            format!("{s}\n[app.dogs.kelpie.rulings]\ndecide = \"always\"\n")
        });
        let mut runner = runner.lock().unwrap();
        runner.take_notes();
        runner.reread(next, rig.kelpie_settings()).unwrap();
        let notes = runner.take_notes().join("\n");
        assert!(!notes.contains("no effect"), "{notes}");
    }

    // A settings file with an entry the sandbox would refuse never becomes
    // `Settings`, so `reread` is not handed it and the running ones stay.
    #[test]
    fn a_reread_with_a_bad_entry_keeps_the_running_settings() {
        let bad = "productionresultssa*.blob.core.windows.net";
        let worker = "allowed_domains = [\"crates.io\", \"index.crates.io\", \"static.crates.io\"]";
        for table in ["worker", "researcher"] {
            let rig = Rig::new("shep");
            let runner = rig.open().unwrap();
            let before = runner.lock().unwrap().settings().clone();
            rig.edit_settings(|s| match table {
                "worker" => s.replace(worker, &format!("allowed_domains = [\"{bad}\"]")),
                _ => format!("{s}\n[app.dogs.kelpie.researcher]\nallowed_domains = [\"{bad}\"]\n"),
            });
            let err = rig.try_settings().expect_err("the entry should be refused");
            assert!(
                err.to_string().contains(&format!("`{bad}`")),
                "{table}: {err}"
            );
            assert_eq!(runner.lock().unwrap().settings(), &before, "{table}");
        }
    }

    #[test]
    fn a_merge_kind_added_to_pm_alone_is_refused_and_the_settings_kept() {
        let rig = Rig::new("shep");
        let runner = rig.open().unwrap();
        let next = settings_with(&rig, |s| {
            format!("{s}\n[app.dogs.kelpie.rulings]\npm_alone = [\"merge\"]\n")
        });
        let mut runner = runner.lock().unwrap();
        let err = runner.reread(next, rig.kelpie_settings()).unwrap_err();
        assert!(err.to_string().contains("rulings.pm_alone"), "{err}");
        assert!(runner.settings().rulings.pm_alone.is_empty());
    }

    #[test]
    fn the_same_settings_change_nothing() {
        let rig = Rig::new("shep");
        let runner = rig.open().unwrap();
        let mut runner = runner.lock().unwrap();
        assert_eq!(
            runner.reread(rig.settings(), rig.kelpie_settings()),
            Ok(None)
        );
    }

    #[test]
    fn the_checkout_and_the_remote_wait_for_the_next_start() {
        let rig = Rig::new("shep");
        let runner = rig.open().unwrap();
        let repo = rig.repo().display().to_string();
        let next = settings_with(&rig, |s| {
            s.replace(
                "remote = \"shep-pm/shep\"",
                "remote = \"shep-pm/elsewhere\"",
            )
            .replace(&repo, "/srv/elsewhere")
            .replace("kickoff_hours = 8", "kickoff_hours = 6")
        });
        let mut runner = runner.lock().unwrap();
        let line = runner.reread(next, rig.kelpie_settings()).unwrap();
        assert_eq!(
            line.as_deref(),
            Some(
                "settings changed: pacing now in effect; git.checkout and git.remote from the \
                 runner's next start"
            )
        );
        assert_eq!(runner.remote().as_str(), "shep-pm/shep");
        assert_eq!(runner.settings().git.checkout, rig.repo());
    }

    #[test]
    fn a_changed_maintainer_alone_takes_effect_at_once() {
        let rig = Rig::new("shep");
        let runner = rig.open().unwrap();
        rig.maintainer("octocat");
        let mut runner = runner.lock().unwrap();
        let line = runner.reread(rig.settings(), rig.kelpie_settings());
        assert_eq!(
            line.unwrap().as_deref(),
            Some("settings changed: git.maintainer now in effect")
        );
        let login = runner.settings().git.maintainer.clone().unwrap();
        assert_eq!(login.as_str(), "octocat");
    }

    #[test]
    fn a_changed_hook_alone_waits_for_the_next_start() {
        let rig = Rig::new("shep");
        let runner = rig.open().unwrap();
        rig.hook("check.sh", &["publish"], "exit 0");
        let mut runner = runner.lock().unwrap();
        let line = runner.reread(rig.settings(), rig.kelpie_settings());
        assert_eq!(
            line.unwrap().as_deref(),
            Some("settings changed; hooks from the runner's next start")
        );
        assert!(runner.settings().hooks.is_empty(), "the old hooks stay");
        assert!(!runner.hooks.has(HookPoint::Publish));
        drop(runner);

        let restarted = rig.open().unwrap();
        let restarted = restarted.lock().unwrap();
        assert_eq!(restarted.settings().hooks.len(), 1);
        assert!(restarted.hooks.has(HookPoint::Publish));
    }

    #[test]
    fn a_changed_contributions_table_waits_for_the_next_start_whatever_else_changes() {
        let rig = Rig::new("shep");
        let runner = rig.open().unwrap();
        // The App does not cover the repo, so a start would refuse `label`.
        rig.edit_settings(|s| {
            format!("{}\n[app.dogs.kelpie.contributions]\npick = \"label\"\n", s)
                .replace("per_item_usd = 10", "per_item_usd = 20")
        });
        let mut runner = runner.lock().unwrap();
        let line = runner.reread(rig.settings(), rig.kelpie_settings());
        assert_eq!(
            line.unwrap().as_deref(),
            Some(
                "settings changed: budget now in effect; contributions from the runner's next start"
            )
        );
        assert!(
            runner.settings().contributions.is_off(),
            "the old pick stays"
        );
        drop(runner);

        let err = rig.open().unwrap_err().to_string();
        assert!(err.contains("contributions.pick"), "{err}");
    }

    #[test]
    fn a_changed_triage_table_waits_for_the_next_start() {
        let rig = Rig::new("shep");
        let runner = rig.open().unwrap();
        // The App does not cover the repo, so a start would refuse `maintainer`.
        rig.edit_settings(|s| format!("{s}\n[app.dogs.kelpie.triage]\npick = \"maintainer\"\n"));
        let mut runner = runner.lock().unwrap();
        let line = runner.reread(rig.settings(), rig.kelpie_settings());
        assert_eq!(
            line.unwrap().as_deref(),
            Some("settings changed; triage from the runner's next start")
        );
        assert!(runner.settings().triage.is_off(), "the old pick stays");
        drop(runner);

        let err = rig.open().unwrap_err().to_string();
        assert!(err.contains("triage.pick"), "{err}");
    }

    #[test]
    fn a_changed_dependencies_table_waits_for_the_next_start() {
        let rig = Rig::new("shep");
        let runner = rig.open().unwrap();
        rig.edit_settings(|s| format!("{s}\n[app.dogs.kelpie.dependencies]\npick = \"on\"\n"));
        let mut runner = runner.lock().unwrap();
        let line = runner.reread(rig.settings(), rig.kelpie_settings());
        assert_eq!(
            line.unwrap().as_deref(),
            Some("settings changed; dependencies from the runner's next start")
        );
        assert!(
            runner.settings().dependencies.is_off(),
            "the old pick stays"
        );
        drop(runner);

        assert!(
            !rig.open()
                .unwrap()
                .lock()
                .unwrap()
                .settings()
                .dependencies
                .is_off()
        );
    }

    fn paddock_at(rig: &Rig, url: &str) {
        let webhook = format!(
            "[webhook]\nkind = \"ntfy\"\nurl = \"{}\"\n",
            Rig::WEBHOOK_URL
        );
        let gateway = format!("[gateways.paddock]\nurl = \"{url}\"\nkey_env = \"PADDOCK_KEY\"\n");
        rig.set_kelpie_settings(&format!("{webhook}{gateway}"));
    }

    fn url_of_paddock(runner: &crate::runner::Runner) -> String {
        let gateway = runner.gateways().get(&paddock()).unwrap();
        gateway.url.as_str().to_owned()
    }

    fn paddock() -> GatewayName {
        GatewayName::try_from("paddock".to_owned()).unwrap()
    }

    #[test]
    fn a_changed_gateway_alone_waits_for_the_next_start() {
        let rig = Rig::new("shep");
        paddock_at(&rig, "http://gpu:8700");
        let runner = rig.open().unwrap();
        paddock_at(&rig, "http://elsewhere:8700");
        let mut runner = runner.lock().unwrap();
        let line = runner.reread(rig.settings(), rig.kelpie_settings());
        assert_eq!(
            line.unwrap().as_deref(),
            Some("settings changed; gateways from the runner's next start")
        );
        assert_eq!(url_of_paddock(&runner), "http://gpu:8700");
        drop(runner);

        let restarted = rig.open().unwrap();
        assert_eq!(
            url_of_paddock(&restarted.lock().unwrap()),
            "http://elsewhere:8700"
        );
    }

    // The gateway's key is read from `CARGO_PKG_NAME`, which `cargo test` sets for its tests.
    const LOCAL: &str = "---\nrole: implementer\nharness: pi\nmodel: m\neffort: low\n\
                         gateway: paddock\ncontext: 65536\n---\n";

    fn list_local(rig: &Rig) {
        rig.write_agent("local", LOCAL);
        rig.implementers(&["sonnet-high", "local"]);
    }

    fn paddock_keyed_by_cargo(rig: &Rig) {
        let webhook = format!(
            "[webhook]\nkind = \"ntfy\"\nurl = \"{}\"\n",
            Rig::WEBHOOK_URL
        );
        let gateway =
            "[gateways.paddock]\nurl = \"http://gpu:8700\"\nkey_env = \"CARGO_PKG_NAME\"\n";
        rig.set_kelpie_settings(&format!("{webhook}{gateway}"));
    }

    #[test]
    fn a_gateway_taken_away_from_a_listed_agent_is_refused_as_a_start_refuses_it() {
        let rig = Rig::new("shep");
        paddock_keyed_by_cargo(&rig);
        list_local(&rig);
        let runner = rig.open().unwrap();
        let webhook = format!(
            "[webhook]\nkind = \"ntfy\"\nurl = \"{}\"\n",
            Rig::WEBHOOK_URL
        );
        rig.set_kelpie_settings(&webhook);
        let mut runner = runner.lock().unwrap();
        let err = runner.reread(rig.settings(), rig.kelpie_settings());
        let err = err.unwrap_err().to_string();
        assert!(
            err.contains("local: gateway paddock is not in kelpie's settings"),
            "{err}"
        );
        assert!(runner.gateways().get(&paddock()).is_some());
    }

    #[test]
    fn held_agents_are_checked_against_the_new_settings_as_a_start_checks_them() {
        let rig = Rig::new("shep");
        paddock_keyed_by_cargo(&rig);
        list_local(&rig);
        let runner = rig.open().unwrap();
        // `local` runs on pi, which a start refuses beside guard hooks.
        let hook = "[[app.dogs.kelpie.worker.guard_hooks]]\nevent = \"PreToolUse\"\n\
                    matcher = \"Bash\"\ncommand = \"~/check.sh\"\n";
        rig.edit_settings(|s| format!("{s}\n{hook}"));
        // A reviewer on `other`, a gateway the runner lacks, holds the new
        // list, which drops `local`.
        let reviewer = "---\nrole: reviewer\nharness: endpoint\ngateway: other\nmodel: coder\n\
                        context: 32768\n---\n";
        rig.write_agent("other", reviewer);
        rig.implementers(&["sonnet-high"]);
        rig.reviewers(&["claude", "other"]);
        let webhook = std::fs::read_to_string(rig.kelpie_settings_file()).unwrap();
        let other = "[gateways.other]\nurl = \"http://gpu:8701\"\nkey_env = \"CARGO_PKG_NAME\"\n";
        rig.set_kelpie_settings(&format!("{webhook}{other}"));
        let mut runner = runner.lock().unwrap();
        let err = runner.reread(rig.settings(), rig.kelpie_settings());
        let err = err.unwrap_err().to_string();
        assert!(
            err.contains("implementer local runs on pi, which cannot run `worker.guard_hooks`"),
            "{err}"
        );
        assert!(runner.settings().worker.guard_hooks.is_empty());
    }

    #[test]
    fn agents_naming_a_gateway_added_with_them_wait_with_it_for_the_next_start() {
        let rig = Rig::new("shep");
        let runner = rig.open().unwrap();
        paddock_keyed_by_cargo(&rig);
        list_local(&rig);
        let mut runner = runner.lock().unwrap();
        let line = runner.reread(rig.settings(), rig.kelpie_settings());
        assert_eq!(
            line.unwrap().as_deref(),
            Some("settings changed; gateways and agents from the runner's next start")
        );
        assert_eq!(runner.settings().agents.implementers.len(), 1);
        drop(runner);

        let restarted = rig.open().unwrap();
        let restarted = restarted.lock().unwrap();
        assert_eq!(restarted.settings().agents.implementers.len(), 2);
        assert!(restarted.gateways().get(&paddock()).is_some());
    }

    #[test]
    fn a_changed_reviewer_file_is_checked_as_a_start_checks_it_and_named() {
        let rig = Rig::new("shep");
        rig.reviewers(&["mine", "claude"]);
        let script = rig.home.path().join("review.sh");
        crate::test::write_script(&script, "#!/bin/sh\nexit 0\n");
        let define = |command: &str| {
            let text = format!("---\nrole: reviewer\nharness: command\ncommand: {command}\n---\n");
            rig.write_agent("mine", &text);
        };
        define(&script.display().to_string());
        let runner = rig.open().unwrap();
        let mut runner = runner.lock().unwrap();

        define("/nonexistent/review.sh");
        let err = runner
            .reread(rig.settings(), rig.kelpie_settings())
            .unwrap_err();
        assert!(
            err.to_string()
                .starts_with("setting `agents.reviewers`: mine: cannot run /nonexistent"),
            "{err}"
        );

        define("~/review.sh");
        let line = runner
            .reread(rig.settings(), rig.kelpie_settings())
            .unwrap();
        assert_eq!(
            line.as_deref(),
            Some("settings changed: agents now in effect"),
            "the same command, spelt from the home folder, in an edited file"
        );

        std::fs::remove_file(rig.paths().agents.join("mine.md")).unwrap();
        let err = runner
            .reread(rig.settings(), rig.kelpie_settings())
            .unwrap_err();
        assert!(
            err.to_string().contains("mine, which has no agent file"),
            "{err}"
        );
    }

    #[test]
    fn a_new_webhook_takes_effect_at_once() {
        let rig = Rig::new("shep");
        let runner = rig.open().unwrap();
        let text = "[webhook]\nkind = \"discord\"\nurl = \"https://discord.example/h\"\n";
        let kelpie = KelpieSettings::from_section(text).unwrap();
        let mut runner = runner.lock().unwrap();
        let line = runner.reread(rig.settings(), kelpie).unwrap();
        assert_eq!(
            line.as_deref(),
            Some("settings changed: webhook now in effect")
        );
        assert_eq!(runner.webhook.as_ref().unwrap().kind, WebhookKind::Discord);
    }

    #[test]
    fn a_webhook_taken_away_takes_effect_at_once() {
        let rig = Rig::new("shep");
        let runner = rig.open().unwrap();
        let mut runner = runner.lock().unwrap();
        let line = runner
            .reread(rig.settings(), KelpieSettings::default())
            .unwrap();
        assert_eq!(
            line.as_deref(),
            Some("settings changed: webhook now in effect")
        );
        assert!(runner.webhook.is_none());
    }

    #[test]
    fn a_change_that_does_not_hold_keeps_the_settings_the_runner_has() {
        let rig = Rig::new("shep");
        let runner = rig.open().unwrap();
        rig.forge.set_visibility(Visibility::Private);
        rig.coderabbit_on();
        let next = rig.settings();
        let mut runner = runner.lock().unwrap();
        let err = runner
            .reread(next, rig.kelpie_settings())
            .unwrap_err()
            .to_string();
        assert!(
            err.starts_with("setting `agents.reviewers`: coderabbit"),
            "{err}"
        );
        let listed = runner
            .settings()
            .agents
            .reviewers
            .clone()
            .unwrap_or_default();
        assert!(!listed.iter().any(|name| name.as_str() == "coderabbit"));
        let missing = settings_with(&rig, |s| {
            s.replace(
                crate::test::RIG_REVIEWERS_AND_CODERABBIT,
                crate::test::RIG_REVIEWERS,
            )
            .replace(
                "build_env = {}\n",
                "instructions_file = \"/nonexistent/w.md\"\n",
            )
        });
        let err = runner
            .reread(missing, rig.kelpie_settings())
            .unwrap_err()
            .to_string();
        assert!(err.contains("worker.instructions_file"), "{err}");
        assert_eq!(runner.settings().worker.instructions_file, None);
        assert!(Path::new(&rig.settings_file()).exists());
    }

    #[test]
    fn each_listed_implementers_agent_label_is_made_on_open_and_on_a_new_list_only() {
        let rig = Rig::new("shep");
        rig.forge.set_repo_labels(&["bug", "Agent:Sonnet-high"]);
        let runner = rig.open().unwrap();
        assert_eq!(rig.forge.repo_labels_now(), ["bug", "Agent:Sonnet-high"]);

        rig.implementers(&["sonnet-high", "opus-high"]);
        let mut runner = runner.lock().unwrap();
        runner
            .reread(rig.settings(), rig.kelpie_settings())
            .unwrap();
        let made = ["bug", "Agent:Sonnet-high", "agent:opus-high"];
        assert_eq!(rig.forge.repo_labels_now(), made);

        rig.implementers(&["opus-high"]);
        runner
            .reread(rig.settings(), rig.kelpie_settings())
            .unwrap();
        assert_eq!(rig.forge.repo_labels_now(), made, "none is removed");
    }

    #[test]
    fn a_fresh_repo_gets_the_default_implementers_label_when_the_runner_opens() {
        let rig = Rig::new("shep");
        rig.open().unwrap();
        assert_eq!(rig.forge.repo_labels_now(), ["agent:sonnet-high"]);
    }

    #[test]
    fn the_issue_writer_or_fallback_alone_is_a_change() {
        let rig = Rig::new("shep");
        let scribe = "---\nrole: issue-writer\nharness: claude-code\nmodel: claude-sonnet-5-5\n\
                      effort: low\n---\nWrite issues.\n";
        rig.write_agent("scribe", scribe);
        let runner = rig.open().unwrap();
        let named = settings_with(&rig, |s| {
            s.replace(
                "\nimplementers = ",
                "\nissue_writer = \"scribe\"\nimplementers = ",
            )
        });
        let line = runner.lock().unwrap().reread(named, rig.kelpie_settings());
        assert_eq!(
            line.unwrap().as_deref(),
            Some("settings changed: agents now in effect")
        );
        let runner_settings = runner
            .lock()
            .unwrap()
            .settings()
            .agents
            .issue_writer
            .clone();
        assert_eq!(runner_settings.as_str(), "scribe");
        let next = settings_with(&rig, |s| {
            s.replace(
                "\nimplementers = ",
                "\nfallback_after = 15\nimplementers = ",
            )
        });
        let mut runner = runner.lock().unwrap();
        let line = runner.reread(next, rig.kelpie_settings()).unwrap();
        assert_eq!(
            line.as_deref(),
            Some("settings changed: agents now in effect")
        );
        assert_eq!(
            runner.settings().agents.fallback_after.map(|m| m.get()),
            Some(15)
        );
    }
}
