//! The plugins the repo turns on, as a runner would load them

use super::{Here, Line, Probes};
use crate::plugins::{RepoPlugins, SKIPPED_BY_SETTING, Source};
use crate::settings::{ForgeSlug, Settings};

/// A line for each plugin the repo turns on or the project asks for: what
/// the runner would load of it, or why it would skip it
///
/// Nothing is copied. The repo's file is read from what the checkout has, so
/// a plugin turned on in a commit it has not fetched yet shows up once it has.
pub(super) fn lines(
    at: &dyn Fn(&str) -> String,
    repo: &ForgeSlug,
    settings: &Settings,
    here: Here<'_>,
    probes: Probes<'_>,
) -> Vec<Line> {
    let branch = probes.forge.default_branch(repo).ok();
    let marketplaces = here.kelpie_home.join("marketplaces");
    let source = Source {
        checkout: &settings.git.checkout,
        branch: branch.as_deref(),
        marketplaces: &marketplaces,
        git: probes.marketplaces,
    };
    describe(at, &RepoPlugins::load(&settings.plugins, &source, None))
}

fn describe(at: &dyn Fn(&str) -> String, loaded: &RepoPlugins) -> Vec<Line> {
    let mut lines: Vec<Line> = loaded
        .status()
        .iter()
        .map(|plugin| {
            let subject = at(&format!("plugin {}", plugin.plugin));
            match &plugin.skipped {
                None => Line::ok(subject, format!("loads {}", plugin.loaded.join(", "))),
                Some(reason) if reason == SKIPPED_BY_SETTING => Line::ok(subject, reason.clone()),
                Some(reason) => Line::missing(
                    subject,
                    format!("is skipped: {reason}"),
                    "correct what it names, or leave the plugin out with `plugins.skip`",
                ),
            }
        })
        .collect();
    lines.extend(loaded.problems().iter().map(|problem| {
        Line::unsure(
            at("plugins"),
            problem.clone(),
            "the runner goes on without it",
        )
    }));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Plugins;
    use crate::test::{FakeMarketplaces, git, write_in};

    #[test]
    fn a_plugin_is_a_line_with_what_loads_and_a_missing_one_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let market = dir.path().join("market");
        let listing = r#"{"name":"market","plugins":[{"name":"house","source":"./house"}]}"#;
        write_in(&market, ".claude-plugin/marketplace.json", listing);
        write_in(&market, "house/skills/rules/SKILL.md", "Follow.\n");
        let repo = dir.path().join("repo");
        let file = format!(
            r#"{{"enabledPlugins":{{"house@market":true,"gone@market":true}},
            "extraKnownMarketplaces":{{"market":{{"source":{{"source":"directory","path":{:?}}}}}}}}}"#,
            market
        );
        write_in(&repo, ".claude/settings.json", &file);
        git(&repo, &["init", "--quiet", "-b", "main"]);
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "--quiet", "-m", "plugins"]);

        let source = Source {
            checkout: &repo,
            branch: Some("main"),
            marketplaces: &dir.path().join("marketplaces"),
            git: &FakeMarketplaces::default(),
        };
        let loaded = RepoPlugins::load(&Plugins::default(), &source, None);
        let lines = describe(&|what| format!("koji: {what}"), &loaded);
        let shown: Vec<String> = lines.iter().map(ToString::to_string).collect();
        let line = |plugin: &str| {
            let subject = format!("koji: plugin {plugin}@market");
            shown.iter().find(|l| l.contains(&subject)).unwrap().clone()
        };
        assert!(line("house").starts_with("ok "), "{shown:?}");
        assert!(line("house").contains("loads skills"), "{shown:?}");
        assert!(line("gone").starts_with("MISSING "), "{shown:?}");
        assert!(line("gone").contains("lists no plugin gone"), "{shown:?}");
        assert_eq!(lines.len(), 2);
        assert!(
            !dir.path().join("marketplaces").exists(),
            "a dry load writes nothing"
        );
    }
}
