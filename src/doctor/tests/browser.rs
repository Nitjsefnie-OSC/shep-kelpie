use super::*;

#[tokio::test]
async fn a_project_with_the_browser_on_is_missing_tools_that_are_not_installed() {
    let scene = Scene::new().await;
    scene.runs("golbat", |t| t["browser"] = json!({ "enabled": true }));
    let report = scene.report().await;
    let (what, fix) = missing(&report, "golbat: browser");
    assert!(what.contains("Playwright"), "{what}");
    assert_eq!(
        fix,
        "run `shep kelpie tools install`, or turn `browser.enabled` off"
    );
    assert!(
        !subjects(&report).contains(&"koji: browser"),
        "off is not checked"
    );

    let tools = Tools::under(&scene.kelpie_home);
    std::fs::create_dir_all(tools.playwright_mcp().parent().unwrap()).unwrap();
    std::fs::write(tools.playwright_mcp(), "").unwrap();
    std::fs::create_dir_all(tools.browsers().join("chromium-1200")).unwrap();
    ok(&scene.report().await, "golbat: browser");
}
