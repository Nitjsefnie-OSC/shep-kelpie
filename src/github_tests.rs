//! The one test of the GitHub App that drives a runner through the rig

use serde_json::json;

use crate::github::FOLDER;
use crate::ports::{Cost, Usage};
use crate::runner::step;
use crate::test::{FakeGithub, Rig, Scripted};

// The App's key sits in kelpie's home, which a worker's sandbox denies
// around its own folders; it never enters a variable or the worktree.
#[test]
fn the_apps_key_never_reaches_a_worker() {
    let rig = Rig::new("koji");
    let kelpie = rig.home.path().join("shep/kelpie");
    FakeGithub::new(rig.clock.clone(), "shep-pm").registered(&kelpie, "shep-pm/koji");
    let runner = rig.open().unwrap();
    assert_eq!(
        rig.ask(&runner, "add", Some("7"))["work_item"]["issue"],
        "7"
    );
    let usage = Usage {
        input: 1,
        cache_write: 0,
        cache_write_5m: 0,
        cache_read: 0,
        output: 1,
    };
    rig.claude.script([Scripted::Reply(usage, Cost(1))]);
    step(&runner).unwrap();
    let [seen] = rig.claude.seen().try_into().unwrap();

    let key = "kelpie-test-app-key-s3cr3t";
    assert!(!seen.settings.to_string().contains(key));
    assert!(!seen.sandbox.to_string().contains(key));
    let fence = seen
        .call
        .reach
        .fence
        .as_deref()
        .expect("a worker is fenced");
    assert!(fence.env.values().all(|v| !v.contains(key)));
    let shep = rig.home.path().join("shep");
    let denied = seen.sandbox["filesystem"]["denyRead"].as_array().unwrap();
    assert!(
        denied.contains(&json!(format!("{}/**", shep.display()))),
        "{denied:?}"
    );
    let allowed = seen.sandbox["filesystem"]["allowRead"].to_string();
    assert!(!allowed.contains("github"), "{allowed}");
    let deny = seen.settings["permissions"]["deny"].to_string();
    let rule = format!("Read(/{}/**)", kelpie.join(FOLDER).display());
    assert!(deny.contains(&rule), "{rule} not in {deny}");

    let mut folders = vec![seen.call.cwd.clone()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(folder).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if let Ok(text) = std::fs::read(&path) {
                assert!(!String::from_utf8_lossy(&text).contains(key), "{path:?}");
            }
        }
    }
}
