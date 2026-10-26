//! The paths an issue names that `main` does not hold, in the first prompt

use super::*;

// The first prompt of issue 7, whose body is `body`, in a project whose `main` keeps `files`
fn prompt_for(body: &str, files: &[&str]) -> String {
    let rig = Rig::new("shep");
    for file in files {
        rig.land(file, "");
    }
    rig.forge.open_issue(7, "Title of #7", body);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude
        .script([Scripted::Reply(usage(1), Cost(20_085_300))]);
    step(&runner).unwrap();
    let [seen] = rig.claude.seen().try_into().unwrap();
    seen.call.prompt
}

#[test]
fn a_renamed_file_is_named_in_the_first_prompt_with_the_closest_match() {
    let prompt = prompt_for(
        "Change `src/route.rs`.",
        &["src/dogs/route.rs", "src/store.rs"],
    );

    assert!(prompt.contains("may have moved"), "{prompt}");
    assert!(
        prompt.contains("`src/route.rs` (closest on `main`: `src/dogs/route.rs`)"),
        "{prompt}"
    );
}

#[test]
fn a_missing_file_with_no_close_match_is_named_alone() {
    let prompt = prompt_for("Change `dogs_kv.rs`.", &["src/store.rs"]);

    assert!(prompt.contains("may have moved"), "{prompt}");
    assert!(prompt.contains("`dogs_kv.rs`"), "{prompt}");
    assert!(!prompt.contains("closest"), "{prompt}");
}

#[test]
fn paths_that_resolve_add_no_note() {
    let prompt = prompt_for("Change `src/store.rs` and `store.rs`.", &["src/store.rs"]);

    assert!(!prompt.contains("moved"), "{prompt}");
}

#[test]
fn a_folder_on_main_and_a_field_in_code_add_no_note() {
    let prompt = prompt_for(
        "Read `item.base` in `src/`, `src/dogs` and `src/dogs/`.",
        &["src/dogs/route.rs"],
    );

    assert!(!prompt.contains("moved"), "{prompt}");
}
