use super::*;

// Puts `settings` and any marketplace beside it on the default branch of golbat's checkout as its
// last fetch left it
fn settle(scene: &Scene, settings: &Value) {
    let repo = scene.home.join("golbat");
    std::fs::create_dir_all(repo.join(".claude")).unwrap();
    std::fs::write(repo.join(".claude/settings.json"), settings.to_string()).unwrap();
    git(&repo, &["add", "--all"]);
    git(&repo, &["commit", "-q", "-m", "plugins"]);
    git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
}

// A directory marketplace in golbat's checkout holding `plugin`, with `skill` in it, and its
// path from the checkout
fn marketplace(scene: &Scene, plugin: &str, skill: &str) -> PathBuf {
    let dir = scene.home.join("golbat/market");
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
    PathBuf::from("market")
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

#[tokio::test]
async fn repo_plugins_shows_what_loads_beside_what_is_skipped() {
    let scene = Scene::new().await;
    scene.runs("golbat", |_| {});
    let market = marketplace(&scene, "style", "style");
    let both = json!({
        "extraKnownMarketplaces": { "house": { "source": { "source": "directory", "path": market } } },
        "enabledPlugins": { "absent@house": true, "style@house": true },
    });
    settle(&scene, &both);

    let report = scene.report().await;

    let Verdict::Unsure { what, .. } = verdict(&report, "golbat: repo plugins") else {
        panic!("{:#?}", report.render());
    };
    assert_eq!(
        what,
        "absent@house is skipped: the marketplace lists no plugin absent; style@house (style)"
    );
}

// Puts an MCP server `search` on `url` in the `.mcp.json` of plugin `style`, which
// `marketplace` made, before `settle` lands it
fn with_server(scene: &Scene, url: &str) {
    let server = json!({ "mcpServers": { "search": { "type": "http", "url": url } } });
    let at = scene.home.join("golbat/market/plugins/style/.mcp.json");
    std::fs::write(at, server.to_string()).unwrap();
}

#[tokio::test]
async fn repo_mcp_names_a_server_whose_host_worker_allowed_domains_lacks() {
    let scene = Scene::new().await;
    scene.runs("golbat", |_| {});
    let market = marketplace(&scene, "style", "style");
    with_server(&scene, "https://search.example.com/mcp");
    settle(&scene, &enabling("style@house", &market));

    let report = scene.report().await;

    let Verdict::Unsure { what, next } = verdict(&report, "golbat: repo mcp servers") else {
        panic!("{:#?}", report.render());
    };
    assert!(
        what.contains("plugin_style_search") && what.contains("search.example.com"),
        "{what}"
    );
    assert!(next.contains("worker.allowed_domains"), "{next}");
    assert!(report.passed(), "{:#?}", report.render());
}

#[tokio::test]
async fn repo_mcp_is_content_with_a_host_the_allowed_domains_cover() {
    let scene = Scene::new().await;
    scene.runs("golbat", |table| {
        table["worker"]["allowed_domains"] = json!(["*.example.com"]);
    });
    let market = marketplace(&scene, "style", "style");
    with_server(&scene, "https://search.example.com/mcp");
    settle(&scene, &enabling("style@house", &market));

    let report = scene.report().await;

    let said = ok(&report, "golbat: repo mcp servers");
    assert!(said.contains("search.example.com"), "{said}");
}

#[tokio::test]
async fn repo_mcp_is_silent_where_no_server_names_a_host() {
    let scene = Scene::new().await;
    scene.runs("golbat", |_| {});
    let market = marketplace(&scene, "style", "style");
    settle(&scene, &enabling("style@house", &market));

    let report = scene.report().await;

    assert!(!subjects(&report).contains(&"golbat: repo mcp servers"));
}

#[tokio::test]
async fn repo_mcp_names_the_host_of_a_server_in_the_default_branchs_mcp_json_too() {
    let scene = Scene::new().await;
    scene.runs("golbat", |_| {});
    let server = json!({ "docs": { "type": "sse", "url": "https://docs.example.com/sse" } });
    std::fs::create_dir_all(scene.home.join("golbat")).unwrap();
    std::fs::write(scene.home.join("golbat/.mcp.json"), server.to_string()).unwrap();
    settle(&scene, &json!({}));

    let report = scene.report().await;

    let Verdict::Unsure { what, .. } = verdict(&report, "golbat: repo mcp servers") else {
        panic!("{:#?}", report.render());
    };
    assert_eq!(
        what,
        "docs reaches docs.example.com, which worker.allowed_domains lacks"
    );
}
