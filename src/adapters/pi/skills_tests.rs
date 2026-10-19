//! The skills a pi call loads from the plugins kelpie wrote for its steps

use std::path::Path;

use super::tests::{URL, WORKER_ID, World, id, strings, values};
use super::*;
use crate::adapters::skills;

// The names of the skill folders in `dir`, sorted.
fn held(dir: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    found.sort();
    found
}

#[test]
fn a_worker_loads_only_the_skills_kelpie_wrote_for_its_steps() {
    let w = World::new();
    let skills =
        crate::skills::Skills::load(&crate::settings::StepSkills::default(), &w.path("skills"));
    let mut call = w.fenced(URL, Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));
    call.plugin_dirs = skills.plugin_dirs().to_vec();
    call.prompt = "/mattpocock:implement Implement issue #7".into();
    let argv = strings(&call);
    let given = values(&argv, "--skill");
    // pi is given every skill folder of the plugins kelpie wrote, and no
    // other, beside its own step skill's folder.
    for dir in skills::skill_dirs(&call) {
        assert!(given.contains(&dir.to_str().unwrap()), "{argv:?}");
    }
    assert_eq!(given.len(), skills::skill_dirs(&call).len() + 1, "{argv:?}");

    assert_eq!(
        held(&w.path("skills/mattpocock/skills")),
        [
            "code-review",
            "codebase-design",
            "diagnosing-bugs",
            "implement",
            "pr",
            "tdd",
        ]
    );
}
