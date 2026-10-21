//! A settings change reaching a running runner
//!
//! Every setting takes effect live, at the next step or call that reads
//! it, except `git.checkout`, `git.remote`, `git.base`, `hooks`,
//! `contributions`, `tickets`, `dependencies` and kelpie's gateways: a work
//! item's worktree and pull request belong to the first three, the forge
//! and voice ports hold the hooks built at start, the start checks and
//! picks the next three, and the adapters hold the gateways, so those wait
//! for the runner's next start. The agent files
//! are read again with them, and a change to those alone is a change too,
//! but agents that name a gateway the runner lacks wait with it. A change is
//! checked as a start checks it, and one that fails keeps the settings the
//! runner has.

use super::{
    OpenError, Runner, check_bots, check_coderabbit, check_local, instructions, open_checks, triage,
};
use crate::agents::Agents;
use crate::review_bot::Bot;
use crate::settings::{AgentName, Gateways, ListedReviewer, RoleAgents, Settings, SettingsError};
use crate::skills::{RepoPlugins, Skills, default_head};
use crate::webhook::KelpieSettings;

mod changed;

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
        // The worktrees, the pull requests and the guard's flag were cut for the base the runner started with.
        if settings.git.base != self.settings.git.base {
            waiting.push("git.base");
            settings.git.base.clone_from(&self.settings.git.base);
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
        // The ticket port was picked at start.
        if settings.tickets != self.settings.tickets {
            waiting.push("tickets");
            settings.tickets.clone_from(&self.settings.tickets);
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
        kelpie.gateways().check_triage_spam(&settings.triage.spam)?;
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
        let mut changed = changed::names((&self.settings, &self.webhook), (&settings, &webhook));
        if settings.triage != self.settings.triage {
            open_checks::triage(&settings, self.ports.voice.as_ref(), &self.remote, &book)?;
        }
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
            self.settings.git.base(),
            &self.paths.kelpie_home,
            &self.paths.skills,
            self.ports.git.as_ref(),
            &settings.repo_setup,
        );
        let plugins_head = default_head(&self.settings.git.checkout, self.settings.git.base());
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
        let e2e_changed = settings.e2e != self.settings.e2e;
        // A start stamps or clears it the same way, so it follows `pick` past `off`.
        let was_triaging = self.state.triage.clone();
        if triage::settle_since(&mut self.state, &settings, self.ports.clock.now()) {
            if let Err(e) = self.store.save(&self.state) {
                self.state.triage = was_triaging;
                return Err(e.into());
            }
            if self.state.triage.is_none() {
                // The poll that listed them stops with `pick`, so they would stay as `status` says them.
                self.triage_due.clear();
                self.triage_queue.clear();
            }
        }
        self.settings = settings;
        if e2e_changed {
            // The dog reads the host from this file, so it follows the settings.
            let file = self.paths.e2e();
            self.notes
                .extend(super::e2e::record(self.settings.e2e.as_ref(), &file));
            self.notes.extend(super::e2e::dog_cannot_find(
                self.settings.e2e.as_ref(),
                &self.paths,
            ));
        }
        self.refresh_features();
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
    // Sets which features the settings and `main`'s launch file turn on.
    fn refresh_features(&self) {
        let dev_server = super::dev_server::is_on(&self.settings);
        self.metrics.features(&self.settings, dev_server);
    }

    /// Loads the repo's plugins again if its default branch moved since they
    /// were read, which settings of kelpie's own do not announce
    ///
    /// Returns a line for the log when the plugins changed.
    pub fn reread_plugins(&mut self) -> Option<String> {
        let checkout = &self.settings.git.checkout;
        if default_head(checkout, self.settings.git.base()) == self.plugins_head {
            return None;
        }
        let planned = RepoPlugins::plan(
            checkout,
            self.settings.git.base(),
            &self.paths.kelpie_home,
            &self.paths.skills,
            self.ports.git.as_ref(),
            &self.settings.repo_setup,
        );
        // The plan fetched the branch, so this is the commit it read.
        self.plugins_head = default_head(checkout, self.settings.git.base());
        // A launch file the branch gained or lost turns the dev server on or off.
        self.refresh_features();
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
    gateways.check_listed(book, listed(agents, lineup))
}

/// Whether every listed agent names a gateway kelpie's settings hold and can reach. No key
/// is read, for a shell that holds none of them.
///
/// # Errors
///
/// A [`SettingsError`] naming the agent whose gateway is missing or cannot be reached.
pub fn check_gateway_shapes(
    gateways: &Gateways,
    book: &Agents,
    agents: &RoleAgents,
    lineup: &[ListedReviewer],
) -> Result<(), SettingsError> {
    gateways.check_listed_without_keys(book, listed(agents, lineup))
}

// Every agent a project runs a call on: its implementers, its reviewers, its advisor
// and its researcher.
fn listed<'a>(
    agents: &'a RoleAgents,
    lineup: &'a [ListedReviewer],
) -> impl Iterator<Item = &'a AgentName> + 'a {
    let implementers = agents.implementers.iter().map(|i| &i.name);
    let reviewers = lineup.iter().map(|r| &r.name);
    let experts = agents.advisor.iter().chain(agents.researcher.iter());
    implementers
        .chain(reviewers)
        .chain(experts.map(|e| &e.name))
}

#[cfg(test)]
mod tests;
