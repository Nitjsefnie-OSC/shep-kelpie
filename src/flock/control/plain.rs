//! A runner's `status` answer as the short block `shep kelpie status` prints
//!
//! It reads the answer as the JSON object it is, so a field a build leaves
//! out reads as none, and the object itself stays the runner's to build.

use serde_json::Value;

use crate::state::TriageHeld;

/// `sheep`'s block from `status`, the runner's answer, under a header that
/// carries `marks`
pub(super) fn block(sheep: &str, marks: &[String], status: &Value) -> Vec<String> {
    let mut lines = vec![match marks {
        [] => sheep.to_owned(),
        marks => format!("{sheep} ({})", marks.join(", ")),
    }];
    let run = status["run"].as_str().unwrap_or("running");
    lines.push(format!("  run: {run}"));
    refused(status, &mut lines);
    items(status, &mut lines);
    adopting(status, &mut lines);
    red_workflows(status, &mut lines);
    contributions(status, &mut lines);
    bumps(status, &mut lines);
    triage(status, &mut lines);
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

// A settings change the runner refused, which stands until a reread holds.
fn refused(status: &Value, lines: &mut Vec<String>) {
    let refusal = &status["settings_refused"];
    let Some(reason) = refusal["reason"].as_str() else {
        return;
    };
    let when = match refusal["at"].as_u64() {
        Some(at) => format!(
            "{} UTC",
            crate::board::briefing::date_time(crate::ports::Timestamp(at))
        ),
        None => "an unknown time".to_owned(),
    };
    lines.push(format!(
        "  settings change refused at {when}, so the old settings stay: {reason}"
    ));
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

// One line for each adopted pull request the board could not start on its
// last poll, which has failed every poll since the time shown
fn adopting(status: &Value, lines: &mut Vec<String>) {
    let skipped = status["skipped"].as_array().map_or(&[][..], Vec::as_slice);
    for skip in skipped.iter().filter(|s| s["reason"] == "adopt") {
        let since = match skip["since"].as_u64() {
            Some(at) => format!(
                "{} UTC",
                crate::board::briefing::date_time(crate::ports::Timestamp(at))
            ),
            None => "an unknown time".to_owned(),
        };
        lines.push(format!(
            "  adopted, cannot start: {} since {since}: {}",
            pull_request(&skip["pull_request"]),
            skip["error"].as_str().unwrap_or("?"),
        ));
    }
}

fn red_workflows(status: &Value, lines: &mut Vec<String>) {
    let red = status["red_workflows"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    if red.is_empty() {
        return;
    }
    lines.push(format!("  red on main: {}", red.len()));
    for workflow in red {
        lines.push(format!(
            "    {}: red for {}, {}",
            workflow["workflow"].as_str().unwrap_or("?"),
            crate::board::briefing::elapsed(workflow["red_for"].as_u64().unwrap_or(0)),
            workflow["url"].as_str().unwrap_or("no link"),
        ));
    }
}

// One line for each pull request from someone else the board follows, and
// none when it follows none
fn contributions(status: &Value, lines: &mut Vec<String>) {
    let followed = status["contributions"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    if followed.is_empty() {
        return;
    }
    lines.push(format!("  contributions: {}", followed.len()));
    for contribution in followed {
        let author = match contribution["author"].as_str() {
            Some("") | None => "a deleted account",
            Some(login) => login,
        };
        let head = contribution["head"].as_str().unwrap_or_default();
        lines.push(format!(
            "    PR #{} by {author}, head {}",
            contribution["pull_request"],
            head.get(..7).unwrap_or(head),
        ));
        if let Some(clock) = contribution.get("clock") {
            lines.push(format!("      {}", waiting(clock)));
        }
    }
}

// One line for each bump from a bot the board follows, and none when it
// follows none
fn bumps(status: &Value, lines: &mut Vec<String>) {
    let followed = status["bumps"].as_array().map_or(&[][..], Vec::as_slice);
    if followed.is_empty() {
        return;
    }
    lines.push(format!("  bumps: {}", followed.len()));
    for bump in followed {
        let head = bump["head"].as_str().unwrap_or_default();
        lines.push(format!(
            "    PR #{} by {}, head {}: {}",
            bump["pull_request"],
            bump["author"].as_str().unwrap_or("?"),
            head.get(..7).unwrap_or(head),
            bump["standing"].as_str().unwrap_or("?"),
        ));
    }
}

// The call in flight, one line for each issue triage would pick up and one for
// each it commented on, and nothing when it has none of them
fn triage(status: &Value, lines: &mut Vec<String>) {
    let triage = &status["triage"];
    if let Some(issue) = triage["running"].as_u64() {
        lines.push(format!("  triage running: #{issue}"));
    }
    daily(triage, lines);
    let records = triage["issues"].as_array().map_or(&[][..], Vec::as_slice);
    for record in records {
        let (issue, comment) = (&record["issue"], &record["comment"]);
        // A limit held it: no call was made and no comment was posted, so the
        // line says which limit rather than a comment that is not there.
        if let Some(why) = held(record) {
            lines.push(format!("  triaged #{issue}: no call, {why}"));
            continue;
        }
        match record["duplicate_of"].as_u64() {
            // The step says whether the close the comment announced is already
            // done, so a close still owed is never shown as one that happened.
            Some(of) if record["step"].as_str() == Some("done") => lines.push(format!(
                "  triaged #{issue}: comment {comment}, closed as a duplicate of #{of}"
            )),
            Some(of) => lines.push(format!(
                "  triaged #{issue}: comment {comment}, to close as a duplicate of #{of}"
            )),
            None => lines.push(format!(
                "  triaged #{issue}: comment {comment}, {}",
                record["step"].as_str().unwrap_or("?"),
            )),
        }
    }
    let due = triage["due"].as_array().map_or(&[][..], Vec::as_slice);
    if due.is_empty() {
        return;
    }
    lines.push(format!("  triage due: {}", due.len()));
    for issue in due {
        let author = match issue["author"].as_str() {
            Some("") | None => "a deleted account",
            Some(login) => login,
        };
        lines.push(format!(
            "    #{} {} by {author}",
            issue["issue"],
            issue["title"].as_str().unwrap_or_default(),
        ));
    }
}

// The day's triage calls against `triage.per_day`, and when the next place
// opens once they fill it
fn daily(triage: &Value, lines: &mut Vec<String>) {
    let daily = &triage["daily"];
    let Some(cap) = daily["cap"].as_u64() else {
        return;
    };
    let used = daily["used"].as_u64().unwrap_or(0);
    let waiting = match daily["wait"].as_u64() {
        // Rounded up, so a place that opens in nine minutes is never shown as
        // one that opens in none.
        Some(seconds) => format!(", next place in {}h", seconds.div_ceil(3_600)),
        None => String::new(),
    };
    lines.push(format!("  triage daily: {used} of {cap} used{waiting}"));
}

// The limit a record says held its issue, in the words the state gives it, or
// none for a record that triaged one
fn held(record: &Value) -> Option<String> {
    let named: TriageHeld = serde_json::from_value(record["held"].clone()).ok()?;
    Some(named.said().to_owned())
}

// What a contribution's clock says: whose move it is and when the nudge is
fn waiting(clock: &Value) -> String {
    let days = |key: &str| clock[key].as_u64().unwrap_or(0);
    let due = clock["due"].as_str();
    let nudge = match due {
        Some(_) => "nudge due".to_owned(),
        None => format!("nudge in {}d", days("days_to_nudge")),
    };
    let quiet = match due {
        Some("quiet") => "quiet".to_owned(),
        _ => format!("quiet in {}d", days("days_to_quiet")),
    };
    let waited = days("waiting_for") / 86_400;
    format!("waiting on the contributor for {waited}d, {nudge}, {quiet}")
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
