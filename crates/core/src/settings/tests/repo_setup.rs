//! `[app.dogs.kelpie.repo_setup]`, which narrows the repo's plugins and adds to them

use super::{EXAMPLE, parse, parse_err};
use crate::ports::Role;
use crate::settings::Part;

fn with(keys: &str) -> String {
    format!("{EXAMPLE}\n[app.dogs.kelpie.repo_setup]\n{keys}\n")
}

#[test]
fn left_out_the_repos_plugins_reach_every_call() {
    let setup = parse(EXAMPLE).unwrap().repo_setup;
    assert!(setup.plugins && setup.own);
    for role in [
        Role::Worker,
        Role::Reviewer,
        Role::IssueWriter,
        Role::Pm,
        Role::Advisor,
        Role::Researcher,
    ] {
        assert!(setup.reaches(role), "{role:?}");
    }
    assert!(!setup.skips("style@house"));
    assert!(setup.extra.is_empty());
}

#[test]
fn an_unknown_key_is_refused_by_name() {
    let err = parse_err(&with("plugin = false"));
    assert!(
        err.contains("`repo_setup.plugin = false`: unknown field `plugin`"),
        "{err}"
    );
}

#[test]
fn roles_name_the_calls_that_get_the_plugins() {
    let setup = parse(&with("roles = [\"worker\", \"pm\"]"))
        .unwrap()
        .repo_setup;
    assert!(setup.reaches(Role::Worker) && setup.reaches(Role::Pm));
    assert!(!setup.reaches(Role::Reviewer));
    assert!(!setup.reaches(Role::Advisor));
}

#[test]
fn a_role_kelpie_has_no_call_for_is_refused() {
    let err = parse_err(&with("roles = [\"implementer\"]"));
    assert!(err.contains("repo_setup.roles"), "{err}");
    assert!(err.contains("worker, reviewer"), "{err}");
}

#[test]
fn a_plugin_is_written_name_at_marketplace() {
    for bad in ["style", "style@", "@house", "a@b@c", "../x@house"] {
        let err = parse_err(&with(&format!("skip = [{bad:?}]")));
        assert!(err.contains("name@marketplace"), "{bad}: {err}");
    }
    let setup = parse(&with("skip = [\"style@house\"]")).unwrap().repo_setup;
    assert!(setup.skips("style@house"));
    assert!(!setup.skips("quiet@house"));
}

#[test]
fn extra_gives_each_plugin_its_marketplace() {
    let keys = "extra = { \"quiet@lab\" = \"github:acme/lab#v2\" }";
    let setup = parse(&with(keys)).unwrap().repo_setup;
    let entries: Vec<_> = setup
        .extra
        .iter()
        .map(|(id, source)| (id.as_str().to_owned(), source.entry()))
        .collect();
    assert_eq!(
        entries,
        [(
            "quiet@lab".to_owned(),
            serde_json::json!({
                "source": { "source": "github", "repo": "acme/lab", "ref": "v2" }
            })
        )]
    );
}

#[test]
fn a_marketplace_is_written_kind_colon_address() {
    for bad in ["acme/lab", "svn:acme/lab", "github:", "github:#v2"] {
        let err = parse_err(&with(&format!("extra = {{ \"quiet@lab\" = {bad:?} }}")));
        assert!(err.contains("github:owner/name"), "{bad}: {err}");
    }
    let keys = "extra = { \"a@x\" = \"git:https://h/r.git\", \"b@y\" = \"directory:plugins/y\" }";
    let setup = parse(&with(keys)).unwrap().repo_setup;
    let entries: Vec<_> = setup.extra.values().map(|s| s.entry()).collect();
    assert_eq!(
        entries,
        [
            serde_json::json!({ "source": { "source": "git", "url": "https://h/r.git" } }),
            serde_json::json!({ "source": { "source": "directory", "path": "plugins/y" } }),
        ]
    );
}

#[test]
fn every_part_of_a_plugin_loads_unless_parts_narrows_it() {
    let setup = parse(EXAMPLE).unwrap().repo_setup;
    for part in [
        Part::Skills,
        Part::Agents,
        Part::Commands,
        Part::Hooks,
        Part::Mcp,
    ] {
        assert!(setup.loads(part), "{part:?}");
    }
    let setup = parse(&with("parts = [\"skills\", \"agents\"]"))
        .unwrap()
        .repo_setup;
    assert!(setup.loads(Part::Skills) && setup.loads(Part::Agents));
    assert!(!setup.loads(Part::Hooks) && !setup.loads(Part::Commands) && !setup.loads(Part::Mcp));
}

#[test]
fn a_part_a_plugin_lacks_is_refused() {
    let err = parse_err(&with("parts = [\"themes\"]"));
    assert!(err.contains("repo_setup.parts"), "{err}");
    assert!(
        err.contains("skills, agents, commands, hooks or mcp"),
        "{err}"
    );
}

#[test]
fn own_false_leaves_the_repos_own_files_out() {
    let setup = parse(&with("own = false")).unwrap().repo_setup;
    assert!(!setup.own);
    assert!(setup.plugins);
}
