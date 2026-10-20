use std::collections::{BTreeMap, BTreeSet};

use super::*;

#[test]
fn every_category_lists_exactly_the_providers_the_code_has() {
    let matrix = read_matrix();
    let mut problems = Vec::new();
    for (category, code) in code_providers() {
        let listed: BTreeSet<String> = matrix
            .providers
            .get(category)
            .map(|p| p.iter().cloned().collect())
            .unwrap_or_default();
        for missing in code.difference(&listed) {
            problems.push(format!(
                "{MATRIX}: `{category}.providers` lacks {missing}, which the code has: add it, \
                 and a cell for it in every `{category}` row"
            ));
        }
        for gone in listed.difference(&code) {
            problems.push(format!(
                "{MATRIX}: `{category}.providers` lists {gone}, which the code does not have"
            ));
        }
    }
    let known = code_providers();
    for category in matrix.providers.keys() {
        if !known.contains_key(category.as_str()) {
            problems.push(format!(
                "{MATRIX}: `{category}` is no category `crates/core/src/parity.rs` knows: add it to \
                 `code_providers` and `enum_paths`"
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn every_row_has_a_cell_for_each_provider_and_evidence_for_each_it_lacks() {
    let matrix = read_matrix();
    assert!(matrix.problems.is_empty(), "{}", matrix.problems.join("\n"));
}

#[test]
fn every_agents_default_is_a_harness_row_whose_cells_match_the_adapters() {
    let matrix = read_matrix();
    let rows: Vec<&Row> = matrix
        .rows
        .iter()
        .filter(|r| r.category == "harness")
        .collect();
    let mut problems = Vec::new();
    for method in agents_defaults() {
        let Some(row) = rows.iter().find(|r| r.methods.contains(&method)) else {
            problems.push(format!(
                "`Agents::{method}` has a default some adapter may keep: add a `harness` row \
                 to {MATRIX} with `methods = [\"{method}\"]`"
            ));
            continue;
        };
        for (provider, cell) in &row.cells {
            let overrides = adapter_files(provider).iter().any(|f| {
                code_lines(f)
                    .iter()
                    .any(|l| l.trim_start().starts_with(&format!("fn {method}(")))
            });
            if overrides != (*cell == Cell::Yes) {
                let has = if overrides {
                    "overrides"
                } else {
                    "keeps the default of"
                };
                problems.push(format!(
                    "{MATRIX}: {} says {cell:?} for {provider}, whose adapter {has} \
                     `Agents::{method}`",
                    row.id()
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

// Whether a bot's profile offers a row's feature.
type Offers = fn(&dyn Profile) -> bool;

#[test]
fn the_review_bot_rows_match_what_each_profile_offers() {
    let matrix = read_matrix();
    let checks: [(&str, Offers); 2] = [
        ("review-bot.summon-by-label", |p| p.label().is_some()),
        ("review-bot.full-review", |p| p.full_review().is_some()),
    ];
    let mut problems = Vec::new();
    for (id, offers) in checks {
        let row = matrix.row(id).expect("the row is in the matrix");
        for bot in Bot::ALL {
            let says_yes = row.cells.get(bot.as_str()) == Some(&Cell::Yes);
            if offers(&*profile(bot)) != says_yes {
                problems.push(format!(
                    "{MATRIX}: {id} for {} disagrees with its profile",
                    bot.as_str()
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn every_branch_on_some_providers_names_its_row() {
    let matrix = read_matrix();
    let mut problems = Vec::new();
    for (place, category, window) in branch_sites(&matrix) {
        let named: Vec<String> = window.iter().flat_map(|l| tags(l)).collect();
        for tag in &named {
            if matrix.row(tag).is_none() {
                problems.push(format!("{place}: `parity: {tag}` names no row in {MATRIX}"));
            }
        }
        if !named.iter().any(|t| t.starts_with(&format!("{category}."))) {
            problems.push(format!(
                "{place} branches on some `{category}` providers without naming the feature: \
                 add `// parity: {category}.<row>` on that line or the {TAG_REACH} above it, and \
                 the row to {MATRIX} with a cell for every provider"
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn every_refusal_names_rows_its_harness_lacks() {
    let matrix = read_matrix();
    let mut problems = Vec::new();
    for provider in matrix.providers.get("harness").into_iter().flatten() {
        for file in adapter_files(provider) {
            let lines = code_lines(&file);
            let Some(at) = lines
                .iter()
                .position(|l| l.starts_with("fn refuse_unsupported("))
            else {
                continue;
            };
            let place = format!("{}:{}", relative(&file), at + 1);
            let named: Vec<String> = lines[at.saturating_sub(TAG_REACH)..at]
                .iter()
                .flat_map(|l| tags(l))
                .collect();
            if named.is_empty() {
                problems.push(format!(
                    "{place}: name each row it refuses with `// parity: harness.<row>` above it"
                ));
            }
            for tag in named {
                match matrix.row(&tag).and_then(|r| r.cells.get(provider)) {
                    None => {
                        problems.push(format!("{place}: `parity: {tag}` names no row in {MATRIX}"))
                    }
                    Some(Cell::Yes) => problems.push(format!(
                        "{place} refuses {tag}, which {MATRIX} says {provider} has"
                    )),
                    Some(_) => {}
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn a_branch_without_its_row_is_found() {
    let matrix = Matrix {
        providers: BTreeMap::from([("webhook".into(), vec!["discord".into(), "ntfy".into()])]),
        ..Matrix::default()
    };
    let line = "    if w.kind == WebhookKind::Ntfy {";
    assert!(names_provider(
        line,
        "webhook",
        &matrix.providers["webhook"]
    ));
    assert!(branches(line));
    assert!(!names_provider(
        "WebhookKind::NtfyTopic",
        "webhook",
        &matrix.providers["webhook"]
    ));
    assert_eq!(
        tags("// parity: webhook.replies, webhook.token"),
        ["webhook.replies", "webhook.token"]
    );
    assert_eq!(variant("claude-code"), "ClaudeCode");
}
