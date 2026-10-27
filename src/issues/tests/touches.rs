use super::*;
use crate::agents::ISSUE_WRITER;
use crate::settings::AgentName;

// A body whose `Touches:` line names `n` files.
fn touching(n: usize) -> String {
    listing(n, "`", "Touches: ")
}

// The same, with each path in `quote` and the line starting `lead`.
fn listing(n: usize, quote: &str, lead: &str) -> String {
    let files: Vec<String> = (1..=n)
        .map(|i| format!("{quote}src/m{i}.rs{quote}"))
        .collect();
    format!("{lead}{}\n\n{BODY}", files.join(", "))
}

// The line kelpie prints for issue 41, filed with `body`.
fn printed(body: &str) -> String {
    let s = Setup::new();
    s.filed(41, "Pause a project", body, &[HUMAN, "agent:sonnet-high"]);
    let lines = s.headless("{\"filed\": [41]}").unwrap();
    lines[1].clone()
}

#[test]
fn an_issue_touching_more_files_than_the_split_is_noted_and_still_passes() {
    assert_eq!(
        printed(&touching(14)),
        "#41 Pause a project (agent:sonnet-high, touches 14 files)"
    );
}

#[test]
fn an_issue_touching_the_split_or_fewer_files_is_not_noted() {
    assert_eq!(
        printed(&touching(13)),
        "#41 Pause a project (agent:sonnet-high)"
    );
}

#[test]
fn an_issue_with_no_touches_line_is_not_noted() {
    assert_eq!(printed(BODY), "#41 Pause a project (agent:sonnet-high)");
}

#[test]
fn paths_without_backticks_and_a_line_in_bold_or_a_list_are_counted() {
    for body in [
        listing(14, "", "Touches: "),
        listing(14, "`", "**Touches:** "),
        listing(14, "", "- Touches: "),
    ] {
        assert_eq!(
            printed(&body),
            "#41 Pause a project (agent:sonnet-high, touches 14 files)",
            "{body}"
        );
    }
    assert_eq!(
        printed(&listing(13, "", "**Touches:** ")),
        "#41 Pause a project (agent:sonnet-high)"
    );
}

#[test]
fn the_files_under_a_touches_heading_or_listed_under_the_line_are_counted() {
    let issue: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/gh-issue-view-check-1302.json"
    ))
    .unwrap();
    assert_eq!(crate::issues::touches(issue["body"].as_str().unwrap()), 5);
    let files: Vec<String> = (1..=14).map(|i| format!("- `src/m{i}.rs`\n")).collect();
    for lead in ["## Touches\n\n", "Touches:\n"] {
        let body = format!("{BODY}\n{lead}{}", files.concat());
        assert_eq!(
            printed(&body),
            "#41 Pause a project (agent:sonnet-high, touches 14 files)",
            "{lead}"
        );
    }
}

#[test]
fn only_the_touches_line_is_counted() {
    let body = format!("{}\nSee `a.rs`, `b.rs`.\n", touching(3));
    assert_eq!(printed(&body), "#41 Pause a project (agent:sonnet-high)");
}

#[test]
fn a_parent_with_sub_issues_is_not_noted() {
    let s = Setup::new();
    s.filed(
        41,
        "Pause a project",
        &touching(14),
        &[HUMAN, "agent:sonnet-high"],
    );
    s.filed(
        42,
        "Its lookout button",
        &touching(14),
        &[HUMAN, "agent:opus-high"],
    );
    s.rig.forge.link_sub_issue(41, 42);

    let lines = s.headless("{\"filed\": [41, 42]}").unwrap();

    assert_eq!(lines[1], "#41 Pause a project (agent:sonnet-high)");
    assert_eq!(
        lines[2],
        "#42 Its lookout button (agent:opus-high, sub-issue of #41, touches 14 files)"
    );
}

#[test]
fn the_issue_writers_prompt_names_the_same_split_as_the_note() {
    let agents = Agents::embedded().bind(
        ISSUE_WRITER,
        "harness: claude-code\nmodel: claude-opus-5-5\neffort: high\n",
    );
    let writer = agents
        .get(&AgentName::try_from(ISSUE_WRITER.to_owned()).unwrap())
        .unwrap();
    let rule = format!("passes {TOUCHES_SPLIT} files");
    assert!(writer.prompt.as_deref().unwrap().contains(&rule), "{rule}");
}
