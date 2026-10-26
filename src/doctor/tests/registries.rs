use super::*;

// Puts `files` in golbat's checkout
fn keeps(scene: &Scene, files: &[&str]) {
    for file in files {
        std::fs::write(scene.home.join("golbat").join(file), "").unwrap();
    }
}

#[tokio::test]
async fn package_registries_names_each_manager_reached_by_the_setting() {
    let scene = Scene::new().await;
    scene.runs("golbat", |table| {
        table["worker"]["allowed_domains"] = json!([]);
    });
    keeps(&scene, &["bun.lock", "Cargo.lock"]);

    let report = scene.report().await;

    assert_eq!(
        ok(&report, "golbat: package registries"),
        "bun by package_registries, cargo by package_registries"
    );
}

#[tokio::test]
async fn package_registries_names_each_manager_reached_by_allowed_domains_when_the_setting_is_off()
{
    let scene = Scene::new().await;
    scene.runs("golbat", |table| {
        table["worker"]["package_registries"] = json!(false);
        table["worker"]["allowed_domains"] = json!(["crates.io", "*.crates.io"]);
    });
    keeps(&scene, &["Cargo.lock"]);

    let report = scene.report().await;

    assert_eq!(
        ok(&report, "golbat: package registries"),
        "cargo by allowed_domains"
    );
}

#[tokio::test]
async fn package_registries_is_missing_the_hosts_neither_setting_reaches() {
    let scene = Scene::new().await;
    scene.runs("golbat", |table| {
        table["worker"]["package_registries"] = json!(false);
        table["worker"]["allowed_domains"] =
            json!(["crates.io", "index.crates.io", "static.crates.io"]);
    });
    keeps(&scene, &["Cargo.lock", "bun.lock"]);

    let report = scene.report().await;

    let (what, fix) = missing(&report, "golbat: package registries");
    assert_eq!(
        what,
        "cargo by allowed_domains; bun lacks registry.npmjs.org"
    );
    assert_eq!(
        fix,
        "add registry.npmjs.org to `worker.allowed_domains`, or turn on `worker.package_registries`"
    );
    assert!(!report.passed(), "{:#?}", report.render());
}

#[tokio::test]
async fn package_registries_is_silent_where_the_checkout_keeps_no_lockfile_or_manifest() {
    let scene = Scene::new().await;
    scene.runs("golbat", |_| {});

    let report = scene.report().await;

    assert!(!subjects(&report).contains(&"golbat: package registries"));
}
