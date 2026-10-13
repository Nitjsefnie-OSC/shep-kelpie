use shep_client::shep_core::config::{AppConfig, DogTable};

use super::*;
use crate::settings::table_of;
use crate::shepherd;
use crate::test::{CALL_PATIENCE, FakeShepherd};

const TABLE: &str = r#"
[git]
checkout = "/work/koji"
merging = "ask"
issues = "ask"

[ci]
block = true
fix_attempts = -1

[concurrency]
active_items = 1
pending_rulings = 2

[budget]
per_item_usd = 10

[agents]
implementers = ["sonnet-high"]
reviewers = ["qwen"]

[pacing]
enabled = true
kickoff_hours = 8

[worker]
allowed_domains = []
build_env = {}
guard_hooks = []
turn_timeout = 60
"#;

/// A flock holding the runner `koji` with `TABLE`, and `rotom` with none
struct Scene {
    shepherd: FakeShepherd,
    name: ProjectName,
}

impl Scene {
    async fn new() -> Self {
        let shepherd = FakeShepherd::new().await;
        let mut koji = AppConfig::minimal("koji", "/opt/kelpie");
        koji.dogs
            .insert(DOG.to_owned(), DogTable::from(table_of(TABLE).unwrap()));
        shepherd.holds(koji, false);
        shepherd.holds(AppConfig::minimal("rotom", "/opt/kelpie"), false);
        Self {
            shepherd,
            name: ProjectName::try_from("koji").unwrap(),
        }
    }

    async fn set(&self, key: &str, value: &str) -> Result<Vec<String>, String> {
        let client = shepherd::connect(self.shepherd.home()).await.unwrap();
        let home = self.shepherd.scratch("home");
        let place = Place {
            home: &home,
            folder: &home,
        };
        let done = set(&client, &self.name, place, key, value);
        tokio::time::timeout(CALL_PATIENCE, done)
            .await
            .expect("set neither ended nor failed in time")
    }

    async fn get(&self, name: &str, key: Option<&str>) -> Result<Vec<String>, String> {
        let client = shepherd::connect(self.shepherd.home()).await.unwrap();
        let name = ProjectName::try_from(name).unwrap();
        let done = get(&client, &name, key);
        tokio::time::timeout(CALL_PATIENCE, done)
            .await
            .expect("get neither ended nor failed in time")
    }

    /// The table the shepherd holds for `koji`
    fn table(&self) -> Map<String, Value> {
        let (koji, _) = self.shepherd.sheep("koji").unwrap();
        koji.dogs.get(DOG).unwrap().as_map().clone()
    }
}

// Real sockets under a real clock: a paused one would time out the
// handshake while the fake's socket is merely waiting.
#[tokio::test]
async fn set_writes_a_word_and_a_list_and_get_shows_both() {
    let mut scene = Scene::new().await;
    scene.set("git.merging", "auto").await.unwrap();
    scene
        .set("worker.allowed_domains", r#"["crates.io"]"#)
        .await
        .unwrap();

    let writes = scene.shepherd.writes();
    assert_eq!(writes.len(), 2, "{writes:?}");
    assert_eq!(
        scene.get("koji", Some("git.merging")).await.unwrap(),
        [r#""auto""#]
    );
    assert_eq!(
        scene
            .get("koji", Some("worker.allowed_domains"))
            .await
            .unwrap(),
        [r#"["crates.io"]"#]
    );
    let whole = scene.get("koji", None).await.unwrap().join("\n");
    assert!(whole.contains("merging = \"auto\""), "{whole}");
    assert!(
        whole.contains("allowed_domains = [\"crates.io\"]"),
        "{whole}"
    );
}

#[tokio::test]
async fn set_reads_a_number_a_boolean_and_a_word_as_what_they_are() {
    let scene = Scene::new().await;
    scene.set("ci.fix_attempts", "3").await.unwrap();
    scene.set("ci.block", "true").await.unwrap();
    scene.set("agents.reviewers", "[]").await.unwrap();
    let table = scene.table();
    assert_eq!(table["ci"]["fix_attempts"], 3);
    assert_eq!(table["ci"]["block"], true);
    assert_eq!(table["agents"]["reviewers"], serde_json::json!([]));
}

#[tokio::test]
async fn set_leaves_every_other_key_as_it_was() {
    let scene = Scene::new().await;
    let before = scene.table();
    scene.set("git.merging", "auto").await.unwrap();
    let mut expected = before;
    expected["git"]["merging"] = "auto".into();
    assert_eq!(scene.table(), expected);
}

#[tokio::test]
async fn set_makes_the_table_a_key_is_in_when_the_settings_have_none() {
    let scene = Scene::new().await;
    scene
        .set("researcher.allowed_domains", r#"["crates.io"]"#)
        .await
        .unwrap();
    assert_eq!(
        scene.table()["researcher"]["allowed_domains"],
        serde_json::json!(["crates.io"])
    );
}

#[tokio::test]
async fn a_value_that_does_not_fit_is_refused_with_the_settings_reason_and_nothing_is_written() {
    let mut scene = Scene::new().await;
    let before = scene.table();
    for (key, value, reason) in [
        ("git.merging", "sometimes", "merging"),
        ("git.nonsense", "1", "nonsense"),
        ("nonsense", "1", "nonsense"),
        ("git.merging.deeper", "1", "git.merging"),
    ] {
        let refused = scene.set(key, value).await.unwrap_err();
        assert!(refused.contains(reason), "{key}: {refused}");
    }
    let refused = scene.set("planning", "true").await.unwrap_err();
    assert!(refused.contains("planning"), "{refused}");
    assert_eq!(scene.shepherd.writes(), []);
    assert_eq!(scene.table(), before);
}

#[tokio::test]
async fn the_refusal_is_the_one_from_settings_from_table() {
    let scene = Scene::new().await;
    let mut table = scene.table();
    table["git"]["merging"] = "sometimes".into();
    let home = scene.shepherd.scratch("home");
    let why = Settings::from_table(&table, "koji", &home, &home).unwrap_err();
    let refused = scene.set("git.merging", "sometimes").await.unwrap_err();
    assert_eq!(refused, why.to_string());
}

#[tokio::test]
async fn get_of_a_key_the_table_does_not_set_is_refused_naming_it() {
    let scene = Scene::new().await;
    for key in ["git.review", "nothing", "git.merging.deeper"] {
        let refused = scene.get("koji", Some(key)).await.unwrap_err();
        assert!(refused.contains(&format!("`{key}`")), "{refused}");
    }
}

#[tokio::test]
async fn a_project_with_no_table_is_refused_pointing_at_add() {
    let mut scene = Scene::new().await;
    let home = scene.shepherd.scratch("home");
    let rotom = ProjectName::try_from("rotom").unwrap();
    let client = shepherd::connect(scene.shepherd.home()).await.unwrap();
    let place = Place {
        home: &home,
        folder: &home,
    };
    let refused = set(&client, &rotom, place, "git.merging", "auto")
        .await
        .unwrap_err();
    assert!(refused.contains("shep kelpie add rotom"), "{refused}");
    let refused = scene.get("rotom", None).await.unwrap_err();
    assert!(refused.contains("shep kelpie add rotom"), "{refused}");
    let refused = scene.get("absent", None).await.unwrap_err();
    assert!(refused.contains("shep kelpie add absent"), "{refused}");
    assert_eq!(scene.shepherd.writes(), []);
}
