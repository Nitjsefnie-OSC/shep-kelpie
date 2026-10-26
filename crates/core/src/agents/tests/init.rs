//! The models file kelpie ships, and the rows it holds

use std::collections::BTreeMap;

use super::name;
use crate::agents::init::{self, MODELS};
use crate::agents::{Agents, Role, baseline, legacy};
use crate::settings::{EndpointUrl, Harness, ModelHost};

// Every baseline `harness` has a row for, as (file name, model, effort, second
// look). The implementer is left out: it has no baseline, so no table of them.
fn bound(file: &init::File, harness: Harness) -> BTreeMap<&str, (&str, &str, bool)> {
    file.rows_for(harness)
        .into_iter()
        .filter(|row| row.baseline.is_some())
        .map(|row| {
            (
                row.file(),
                (row.model.as_str(), row.effort.as_str(), row.second_look),
            )
        })
        .collect()
}

#[test]
fn every_row_names_a_real_harness_role_and_baseline() {
    let file = init::read(MODELS).unwrap();
    assert!(!file.rows.is_empty(), "the file holds rows");
    for harness in Harness::ALL {
        assert!(
            file.order.contains(&harness),
            "{} is left out of `order`",
            harness.as_str()
        );
    }
    assert_eq!(
        file.order.len(),
        Harness::ALL.len(),
        "`order` names each once"
    );
    for row in &file.rows {
        assert!(Harness::ALL.contains(&row.harness), "{row:?}");
        assert!(Role::ALL.contains(&row.role), "{row:?}");
        let own = baseline::of_role(row.role);
        match row.baseline {
            Some(name) => assert!(
                own.iter().any(|baseline| baseline.name == name),
                "`{name}` is no baseline of {}",
                row.role.as_str()
            ),
            None => assert!(
                own.is_empty(),
                "{} has baselines, so its row names one: {row:?}",
                row.role.as_str()
            ),
        }
        let server = match row.needs_server() {
            true => Some(ModelHost::Url(
                EndpointUrl::try_from("http://box:11434/v1".to_owned()).unwrap(),
            )),
            false => None,
        };
        baseline::check(row.file(), &row.text(server.as_ref()))
            .unwrap_or_else(|e| panic!("{}: {e}", row.file()));
    }
}

#[test]
fn the_claude_code_rows_equal_the_legacy_table() {
    let file = init::read(MODELS).unwrap();
    let rows = bound(&file, Harness::ClaudeCode);
    let legacy: BTreeMap<&str, (&str, &str, bool)> = legacy::TABLE
        .iter()
        .map(|row| (row.name, (row.model, row.effort, row.second_look)))
        .collect();
    assert_eq!(rows, legacy, "the same binding `agents migrate` writes");
    assert_eq!(
        file.rows_for(Harness::ClaudeCode).len(),
        legacy::TABLE.len() + 1,
        "the ten baselines and the implementer, and nothing else"
    );

    let shipped = Agents::embedded();
    let sonnet = shipped.get(&name("sonnet-high")).unwrap();
    let (model, _) = sonnet.runs.session().unwrap();
    let implementer = file
        .row(Harness::ClaudeCode, Role::Implementer, None)
        .expect("the file binds the implementer");
    assert_eq!(implementer.file(), "implementer");
    assert_eq!(
        (implementer.model.as_str(), implementer.effort),
        (model.model.as_str(), model.effort),
        "the implementer row is the one kelpie ships as the default"
    );
}

#[test]
fn a_row_that_names_no_baseline_of_its_role_is_refused_naming_the_file() {
    let bad = MODELS.replace(
        "[claude-code.reviewer.spec]",
        "[claude-code.reviewer.hunter]",
    );
    let err = init::read(&bad).unwrap_err();
    assert!(
        err.contains(
            "agents/models.toml: `[claude-code.reviewer.hunter]` is no baseline of reviewer"
        ),
        "{err}"
    );
    let role = MODELS.replace("[claude-code.pm]", "[claude-code.judge]");
    let err = init::read(&role).unwrap_err();
    assert!(err.contains("`[claude-code.judge]` is no role"), "{err}");
    let harness = MODELS.replace("[claude-code.pm]", "[gemini.pm]");
    let err = init::read(&harness).unwrap_err();
    assert!(err.contains("`gemini` is no harness"), "{err}");
    let key = MODELS.replace(
        "effort = \"medium\"\n\n[claude-code.triager]",
        "effort = \"medium\"\nsecond_look = true\n\n[claude-code.triager]",
    );
    let err = init::read(&key).unwrap_err();
    assert!(err.contains("`second_look` is a reviewer's"), "{err}");
}

#[test]
fn a_server_harness_row_needs_context_and_another_takes_none() {
    let pi = "order = [\"pi\"]\n\n[pi.implementer]\nmodel = \"qwen3.8:27b\"\neffort = \"high\"\n";
    let err = init::read(pi).unwrap_err();
    assert!(err.contains("`[pi.implementer]` needs `context`"), "{err}");
    let read = init::read(&format!("{pi}context = 65536\n")).unwrap();
    let row = read.rows_for(Harness::Pi)[0];
    assert_eq!(row.context, Some(65536));
    assert!(row.needs_server());

    let extra = MODELS.replace(
        "[claude-code.pm]\nmodel = \"claude-opus-5-5\"",
        "[claude-code.pm]\ncontext = 65536\nmodel = \"claude-opus-5-5\"",
    );
    let err = init::read(&extra).unwrap_err();
    assert!(
        err.contains("`[claude-code.pm]` takes no `context`: claude-code runs no model server"),
        "{err}"
    );
}
