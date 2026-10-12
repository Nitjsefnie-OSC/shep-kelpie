use super::*;

// Puts `settings` on the default branch of golbat's checkout as its last fetch left it
fn settle(scene: &Scene, settings: &Value) {
    let repo = scene.home.join("golbat");
    std::fs::create_dir_all(repo.join(".claude")).unwrap();
    std::fs::write(repo.join(".claude/settings.json"), settings.to_string()).unwrap();
    git(&repo, &["add", ".claude/settings.json"]);
    git(&repo, &["commit", "-q", "-m", "plugins"]);
    git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
}

// A directory marketplace holding `plugin`, with `skill` in it
fn marketplace(scene: &Scene, plugin: &str, skill: &str) -> PathBuf {
    let dir = scene.home.join("market");
    let at = dir.join("plugins").join(plugin);
    std::fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
    let listed =
        json!({ "plugins": [{ "name": plugin, "source": format!("./plugins/{plugin}") }] });
    std::fs::write(
        dir.join(".claude-plugin/marketplace.json"),
        listed.to_string(),
    )
    .unwrap();
    std::fs::create_dir_all(at.join("skills").join(skill)).unwrap();
    std::fs::write(at.join("skills").join(skill).join("SKILL.md"), "Rules.\n").unwrap();
    dir
}

fn enabling(plugin: &str, market: &Path) -> Value {
    json!({
        "extraKnownMarketplaces": { "house": { "source": { "source": "directory", "path": market } } },
        "enabledPlugins": { plugin: true },
    })
}

#[tokio::test]
async fn repo_plugins_is_silent_where_the_repo_turns_on_none() {
    let scene = Scene::new().await;
    scene.runs("golbat", |_| {});

    let report = scene.report().await;

    assert!(!subjects(&report).contains(&"golbat: repo plugins"));
}

#[tokio::test]
async fn repo_plugins_names_each_plugin_with_its_skills() {
    let scene = Scene::new().await;
    scene.runs("golbat", |_| {});
    let market = marketplace(&scene, "style", "style");
    settle(&scene, &enabling("style@house", &market));

    let report = scene.report().await;

    assert_eq!(
        ok(&report, "golbat: repo plugins"),
        "loads style@house (style)"
    );
}

#[tokio::test]
async fn repo_plugins_says_why_a_plugin_is_skipped_without_failing_the_run() {
    let scene = Scene::new().await;
    scene.runs("golbat", |_| {});
    let market = marketplace(&scene, "style", "style");
    settle(&scene, &enabling("absent@house", &market));

    let report = scene.report().await;

    let Verdict::Unsure { what, .. } = verdict(&report, "golbat: repo plugins") else {
        panic!("{:#?}", report.render());
    };
    assert_eq!(
        what,
        "absent@house is skipped: the marketplace lists no plugin absent"
    );
    assert!(report.passed(), "{:#?}", report.render());
}
