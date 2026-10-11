//! What a call's fence lets it read, judged on the settings it saw and the
//! sandbox policy its fence makes, the way each layer would judge a path

use std::path::Path;

use serde_json::Value;

use super::Seen;

/// `path` as given and with its links resolved, as each tool may name it
pub(crate) fn spellings(path: &Path) -> Vec<String> {
    let resolved = path.canonicalize().ok().filter(|c| c != path);
    std::iter::once(path.to_owned())
        .chain(resolved)
        .map(|p| p.display().to_string())
        .collect()
}

/// Whether Claude Code's Read rules in the call's settings refuse `path`
pub(crate) fn tools_refuse(seen: &Seen, path: &str) -> bool {
    let deny = seen.settings["permissions"]["deny"].as_array().unwrap();
    deny.iter().filter_map(Value::as_str).any(|rule| {
        let Some(glob) = rule
            .strip_prefix("Read(/")
            .and_then(|r| r.strip_suffix(')'))
        else {
            return false;
        };
        match glob.strip_suffix("/**") {
            Some(folder) => path.starts_with(&format!("{folder}/")),
            None => path == glob,
        }
    })
}

/// Whether the sandbox refuses `path`: the narrowest rule covering it wins,
/// as `srt` emits a deny nested in an allow after the allow
pub(crate) fn sandbox_refuses(seen: &Seen, path: &str) -> bool {
    let files = &seen.sandbox["filesystem"];
    let narrowest = |key: &str| {
        let rules = files[key]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str);
        rules
            .map(|rule| rule.strip_suffix("/**").unwrap_or(rule))
            .filter(|folder| path == *folder || path.starts_with(&format!("{folder}/")))
            .map(str::len)
            .max()
    };
    match (narrowest("denyRead"), narrowest("allowRead")) {
        (Some(denied), Some(allowed)) => denied >= allowed,
        (denied, _) => denied.is_some(),
    }
}

/// Asserts the call reads `path` in both layers. Its tools are judged on
/// `path` as given and with its links resolved; `srt` resolves its rules'
/// links itself, so the sandbox is judged as given.
pub(crate) fn assert_reads(seen: &Seen, path: &Path) {
    for spelled in spellings(path) {
        assert!(!tools_refuse(seen, &spelled), "its tools refuse {spelled}");
    }
    let spelled = path.display().to_string();
    assert!(
        !sandbox_refuses(seen, &spelled),
        "its sandbox refuses {spelled}"
    );
}

/// Asserts the call is refused `path` in both layers
pub(crate) fn assert_refuses(seen: &Seen, path: &Path) {
    for spelled in spellings(path) {
        assert!(tools_refuse(seen, &spelled), "its tools read {spelled}");
    }
    let spelled = path.display().to_string();
    assert!(
        sandbox_refuses(seen, &spelled),
        "its sandbox reads {spelled}"
    );
}
