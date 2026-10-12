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

/// What `sheep`'s runner's answer `body` to `action` with `params` says as
/// plain lines, for the verbs that print one: the answer itself when it is
/// not the object that action answers with
pub(super) fn reply(sheep: &str, action: &str, params: Option<&str>, body: &str) -> Vec<String> {
    let Ok(answer) = serde_json::from_str::<Value>(body) else {
        return vec![body.to_owned()];
    };
    let of = || match params {
        Some(issue) => format!("#{issue}"),
        None => "the open work item".to_owned(),
    };
    match action {
        "timings" => match answer["table"].as_str() {
            Some(table) => table.lines().map(str::to_owned).collect(),
            None => vec![body.to_owned()],
        },
        // An answer that contradicts the action is printed as it came.
        "drain" if answer["draining"].is_object() => {
            vec![draining(sheep, &answer["draining"])]
        }
        "undrain" if answer["draining"].is_null() => {
            vec![format!("{sheep} is not draining: calls start again")]
        }
        "drain" | "undrain" => vec![body.to_owned()],
        "board" => board(sheep, &answer["board"]),
        "gate" => vec![format!("{sheep}: sent {} to the review gate", of())],
        "drop" => vec![format!("{sheep}: dropped {}", of())],
        "add" => vec![format!(
            "{sheep}: opened a work item for #{}",
            params.unwrap_or("?")
        )],
        _ => vec![body.to_owned()],
    }
}

// The board's list, one numbered line an issue.
fn board(sheep: &str, list: &Value) -> Vec<String> {
    let Some(places) = list.as_array() else {
        return vec![list.to_string()];
    };
    if places.is_empty() {
        return vec![format!("{sheep}: no issue is ready on the board")];
    }
    places
        .iter()
        .map(|place| {
            let mut notes = vec![place["priority"].as_str().unwrap_or("no priority")];
            notes.extend(
                place["blocked"]
                    .as_bool()
                    .unwrap_or(false)
                    .then_some("blocked"),
            );
            notes.extend(place["held"].as_bool().unwrap_or(false).then_some("held"));
            format!(
                "{}. #{} {} ({})",
                place["position"],
                place["issue"],
                place["title"].as_str().unwrap_or_default(),
                notes.join(", ")
            )
        })
        .collect()
}

fn draining(sheep: &str, draining: &Value) -> String {
    let calls = draining["calls"].as_array().map_or(&[][..], Vec::as_slice);
    if calls.is_empty() {
        return format!("{sheep} is draining: no new call starts, and none is running");
    }
    let running: Vec<String> = calls
        .iter()
        .map(|call| match call["issue"].as_u64() {
            Some(issue) => format!("{} #{issue}", call["role"].as_str().unwrap_or("?")),
            None => call["role"].as_str().unwrap_or("?").to_owned(),
        })
        .collect();
    format!(
        "{sheep} is draining: no new call starts; still running: {}",
        running.join(", ")
    )
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
