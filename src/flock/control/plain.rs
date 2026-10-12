//! A runner's `status` answer as the short block `shep kelpie status` prints
//!
//! It reads the answer as the JSON object it is, so a field a build leaves
//! out reads as none, and the object itself stays the runner's to build.

use serde_json::Value;

/// `sheep`'s block from `status`, the runner's answer, under a header that
/// carries `marks`
pub(super) fn block(sheep: &str, marks: &[String], status: &Value) -> Vec<String> {
    let mut lines = vec![match marks {
        [] => sheep.to_owned(),
        marks => format!("{sheep} ({})", marks.join(", ")),
    }];
    let run = status["run"].as_str().unwrap_or("running");
    lines.push(format!("  run: {run}"));
    items(status, &mut lines);
    rulings(status, &mut lines);
    pacer(status, &mut lines);
    lines
}

fn items(status: &Value, lines: &mut Vec<String>) {
    let items = status["work_items"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    if items.is_empty() {
        lines.push("  work items: none".to_owned());
        return;
    }
    lines.push("  work items:".to_owned());
    for item in items {
        lines.push(format!(
            "    #{} {}: phase {}, turn {}, {}",
            item["issue"],
            item["title"].as_str().unwrap_or_default(),
            item["phase"]["state"].as_str().unwrap_or("?"),
            item["turn"]["state"].as_str().unwrap_or("?"),
            pull_request(&item["pull_request"]),
        ));
    }
}

fn pull_request(number: &Value) -> String {
    match number.as_u64() {
        Some(number) => format!("PR #{number}"),
        None => "no PR".to_owned(),
    }
}

fn rulings(status: &Value, lines: &mut Vec<String>) {
    let rulings = status["rulings"].as_array().map_or(&[][..], Vec::as_slice);
    if rulings.is_empty() {
        lines.push("  rulings waiting: none".to_owned());
        return;
    }
    lines.push(format!("  rulings waiting: {}", rulings.len()));
    for ruling in rulings {
        lines.push(format!(
            "    ruling {}: {}, {}, {}",
            ruling["id"],
            ruling["kind"]["kind"].as_str().unwrap_or("?"),
            ruling["issue"]
                .as_u64()
                .map_or("no issue".to_owned(), |issue| format!("#{issue}")),
            pull_request(&ruling["pull_request"]),
        ));
    }
}

fn pacer(status: &Value, lines: &mut Vec<String>) {
    let pacer = &status["pacer"];
    let on = pacer["enabled"].as_bool().unwrap_or(true);
    lines.push(format!("  pacer: {}", if on { "on" } else { "off" }));
    let accounts = pacer.as_object().into_iter().flatten();
    for (account, state) in accounts.filter(|(key, _)| key.as_str() != "enabled") {
        let reading = &state["reading"];
        lines.push(match reading.is_object() {
            true => format!(
                "    {account}: session {}%, week {}%, {}% of the week spent today of {}% allowed",
                reading["session"]["used_pct"],
                reading["week"]["used_pct"],
                reading["spent_today_pct"],
                reading["allowance_pct"],
            ),
            false => format!("    {account}: not read"),
        });
        let hold = &state["holding"];
        if hold.is_object() {
            lines.push(format!(
                "    {account} holding ({}): {}",
                hold["kind"].as_str().unwrap_or("?"),
                hold["reason"].as_str().unwrap_or_default(),
            ));
        }
    }
}
