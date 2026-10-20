//! `docs/parity.toml`, the provider parity matrix, checked against the code
//!
//! Each provider in a category has each row's feature, has an open issue
//! that adds it, or has a file in the repo that says why it cannot. A
//! category's providers come from the code, so a new one fails here until
//! every row says what it can do.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::review_bot::{Bot, Profile};
use crate::settings::{GatewayKind, Harness};
use crate::webhook::WebhookKind;

const MATRIX: &str = "docs/parity.toml";

// What each cell may say, for the messages that ask for one.
const CELL_FORMS: &str = "\"yes\", { gap = <the open issue that adds it> }, or \
     { cannot = \"why\", doc = \"<repo path>\", says = \"<words that file holds>\" }";

// How far above a branch its `parity:` comment may sit.
const TAG_REACH: usize = 3;

// How far above a wildcard `_ =>` arm a provider's arm makes it a branch.
const ARM_REACH: usize = 12;

#[derive(Debug, PartialEq, Eq)]
enum Cell {
    Yes,
    Gap,
    Cannot,
}

#[derive(Debug)]
struct Row {
    category: String,
    name: String,
    methods: Vec<String>,
    cells: BTreeMap<String, Cell>,
}

impl Row {
    fn id(&self) -> String {
        format!("{}.{}", self.category, self.name)
    }
}

#[derive(Debug, Default)]
struct Matrix {
    providers: BTreeMap<String, Vec<String>>,
    rows: Vec<Row>,
    // What is wrong with the file itself, found while reading it.
    problems: Vec<String>,
}

impl Matrix {
    fn row(&self, id: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.id() == id)
    }
}

// The workspace root, two folders above this crate's own.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

// The `src` of the root package and of every crate under `crates/`.
fn source_dirs() -> Vec<PathBuf> {
    let mut crates: Vec<_> = std::fs::read_dir(root().join("crates"))
        .expect("crates reads")
        .map(|e| e.expect("an entry").path().join("src"))
        .filter(|src| src.is_dir())
        .collect();
    crates.sort();
    let mut dirs = vec![root().join("src")];
    dirs.extend(crates);
    dirs
}

fn read_matrix() -> Matrix {
    let text = std::fs::read_to_string(root().join(MATRIX)).expect("docs/parity.toml reads");
    let table: toml::Table = text.parse().expect("docs/parity.toml is TOML");
    let mut matrix = Matrix::default();
    for (category, value) in &table {
        let Some(category_table) = value.as_table() else {
            matrix
                .problems
                .push(format!("{MATRIX}: `{category}` is not a table"));
            continue;
        };
        let providers: Vec<String> = category_table
            .get("providers")
            .and_then(toml::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|p| p.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        for (name, row) in category_table {
            if name == "providers" {
                continue;
            }
            let Some(row) = row.as_table() else {
                matrix
                    .problems
                    .push(format!("{MATRIX}: `{category}.{name}` is not a table"));
                continue;
            };
            matrix.rows.push(read_row(
                category,
                name,
                row,
                &providers,
                &mut matrix.problems,
            ));
        }
        matrix.providers.insert(category.clone(), providers);
    }
    matrix
}

fn read_row(
    category: &str,
    name: &str,
    row: &toml::Table,
    providers: &[String],
    problems: &mut Vec<String>,
) -> Row {
    let id = format!("{category}.{name}");
    let mut cells = BTreeMap::new();
    for (key, value) in row {
        match key.as_str() {
            "what" => {}
            "methods" if category == "harness" => {}
            _ if providers.contains(key) => match read_cell(value) {
                Ok(cell) => {
                    cells.insert(key.clone(), cell);
                }
                Err(why) => problems.push(format!("{MATRIX}: {id}.{key} {why}")),
            },
            _ => problems.push(format!(
                "{MATRIX}: {id} has `{key}`, which is not a provider of `{category}`"
            )),
        }
    }
    if !row.get("what").is_some_and(toml::Value::is_str) {
        problems.push(format!("{MATRIX}: {id} says in `what` what the feature is"));
    }
    for provider in providers.iter().filter(|p| !row.contains_key(*p)) {
        problems.push(format!(
            "{MATRIX}: {id} has no cell for {provider}: say {CELL_FORMS}"
        ));
    }
    let methods = row
        .get("methods")
        .and_then(toml::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|m| m.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Row {
        category: category.to_owned(),
        name: name.to_owned(),
        methods,
        cells,
    }
}

fn read_cell(value: &toml::Value) -> Result<Cell, String> {
    if value.as_str() == Some("yes") {
        return Ok(Cell::Yes);
    }
    let Some(table) = value.as_table() else {
        return Err(format!("is neither of {CELL_FORMS}"));
    };
    if let Some(issue) = table.get("gap") {
        return match issue.as_integer() {
            Some(n) if n > 0 && table.len() == 1 => Ok(Cell::Gap),
            _ => Err("names its issue by number, and nothing else: { gap = <n> }".into()),
        };
    }
    let field = |key: &str| table.get(key).and_then(toml::Value::as_str);
    let (Some(_), Some(doc), Some(says)) = (field("cannot"), field("doc"), field("says")) else {
        return Err(format!("is neither of {CELL_FORMS}"));
    };
    let Ok(text) = std::fs::read_to_string(root().join(doc)) else {
        return Err(format!("cites {doc}, which is not a file in the repo"));
    };
    if !text.contains(says) {
        return Err(format!("quotes \"{says}\", which {doc} does not say"));
    }
    Ok(Cell::Cannot)
}

// Every string a schema allows: its `enum` values and `const`s.
fn schema_names(value: &Value, names: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(s)) = map.get("const") {
                names.insert(s.clone());
            }
            if let Some(Value::Array(values)) = map.get("enum") {
                names.extend(values.iter().filter_map(|v| v.as_str().map(str::to_owned)));
            }
            map.values().for_each(|v| schema_names(v, names));
        }
        Value::Array(values) => values.iter().for_each(|v| schema_names(v, names)),
        _ => {}
    }
}

fn named_by_schema<T: schemars::JsonSchema>() -> BTreeSet<String> {
    let schema = schemars::SchemaGenerator::default().into_root_schema_for::<T>();
    let mut names = BTreeSet::new();
    schema_names(schema.as_value(), &mut names);
    names
}

// Each category's providers as the code has them, by the names settings and
// agent files use.
fn code_providers() -> BTreeMap<&'static str, BTreeSet<String>> {
    BTreeMap::from([
        ("harness", named_by_schema::<Harness>()),
        (
            "review-bot",
            Bot::ALL.iter().map(|b| b.as_str().to_owned()).collect(),
        ),
        ("webhook", named_by_schema::<WebhookKind>()),
        ("gateway", named_by_schema::<GatewayKind>()),
    ])
}

// The path every variant of a category's enum is named by in the code.
fn enum_paths(category: &str) -> &'static [&'static str] {
    match category {
        // `Harness::` also matches `AgentHarness::`.
        "harness" => &["Harness::"],
        "review-bot" => &["Bot::"],
        "webhook" => &["WebhookKind::"],
        "gateway" => &["GatewayKind::"],
        _ => &[],
    }
}

// `claude-code` is `ClaudeCode` in the code.
fn variant(provider: &str) -> String {
    provider
        .split('-')
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|c| c.to_ascii_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        })
        .collect()
}

fn harness(provider: &str) -> Harness {
    serde_json::from_value(Value::String(provider.to_owned())).expect("a harness name")
}

fn profile(bot: Bot) -> Box<dyn Profile> {
    match bot {
        Bot::Coderabbit => Box::new(crate::coderabbit::CodeRabbit),
        Bot::Cubic => Box::new(crate::cubic::Cubic),
        Bot::Codex => Box::new(crate::codex::Codex),
    }
}

// Every `.rs` file under `dir`, test files left out: a `tests` folder, a
// `test` folder or file in a crate's `src` (the stand-ins), and this file.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .expect("src reads")
        .map(|e| e.expect("an entry").path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if name != "tests" && !is_stand_ins(&path) {
                sources(&path, out);
            }
        } else if name.ends_with(".rs")
            && name != "tests.rs"
            && !name.ends_with("_tests.rs")
            && !is_stand_ins(&path)
            && path != root().join("crates/core/src/parity/mod.rs")
        {
            out.push(path);
        }
    }
}

// Whether `path` is a crate's `src/test` or `src/test.rs`.
fn is_stand_ins(path: &Path) -> bool {
    let in_src = path
        .parent()
        .is_some_and(|dir| dir.file_name().is_some_and(|n| n == "src"));
    in_src && path.file_stem().is_some_and(|stem| stem == "test")
}

// A file's lines, its inline test modules blanked so line numbers hold.
fn code_lines(path: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(path).expect("a source file reads");
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let mut i = 0;
    while i < lines.len() {
        let opens_tests = lines[i].trim() == "#[cfg(test)]"
            && lines
                .get(i + 1)
                .is_some_and(|l| l.trim_start().starts_with("mod ") && l.ends_with('{'));
        if !opens_tests {
            i += 1;
            continue;
        }
        let mut depth = 0_i32;
        for line in &mut lines[i..] {
            depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
            let closed = depth == 0 && line.contains('}');
            line.clear();
            i += 1;
            if closed {
                break;
            }
        }
    }
    lines
}

fn relative(path: &Path) -> String {
    path.strip_prefix(root())
        .unwrap_or(path)
        .display()
        .to_string()
}

// The rows a `parity:` comment on `line` names.
fn tags(line: &str) -> Vec<String> {
    let Some((_, rest)) = line.split_once("parity:") else {
        return Vec::new();
    };
    rest.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| t.contains('.'))
        .map(|t| t.trim_end_matches(['.', ';']).to_owned())
        .collect()
}

// Whether `line` names one of `category`'s providers.
fn names_provider(line: &str, category: &str, providers: &[String]) -> bool {
    enum_paths(category).iter().any(|path| {
        providers.iter().any(|p| {
            let whole = format!("{path}{}", variant(p));
            line.match_indices(&whole).any(|(at, _)| {
                let after = line[at + whole.len()..].chars().next();
                !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
            })
        })
    })
}

fn branches(line: &str) -> bool {
    ["==", "!=", "matches!", "if let"]
        .iter()
        .any(|b| line.contains(b))
        || (line.contains("let ") && line.contains(" else"))
}

// Each branch on some of a category's providers: its place, its category,
// and the lines its `parity:` comment may sit on.
fn branch_sites(matrix: &Matrix) -> Vec<(String, String, Vec<String>)> {
    let mut files = Vec::new();
    for dir in source_dirs() {
        sources(&dir, &mut files);
    }
    let mut sites = Vec::new();
    for path in files {
        let lines = code_lines(&path);
        for (category, providers) in &matrix.providers {
            if category == "harness" && is_adapter(&path, providers) {
                continue;
            }
            for (i, line) in lines.iter().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                let reach = if code.trim_start().starts_with("_ =>") {
                    let from = i.saturating_sub(ARM_REACH);
                    if !lines[from..i]
                        .iter()
                        .any(|l| names_provider(l, category, providers))
                    {
                        continue;
                    }
                    from.saturating_sub(TAG_REACH)
                } else if names_provider(code, category, providers) && branches(code) {
                    i.saturating_sub(TAG_REACH)
                } else {
                    continue;
                };
                let place = format!("{}:{}", relative(&path), i + 1);
                sites.push((place, category.clone(), lines[reach..=i].to_vec()));
            }
        }
    }
    sites
}

// Whether `path` is one harness's own adapter, whose branches on the
// harness only tell its calls from another's.
fn is_adapter(path: &Path, providers: &[String]) -> bool {
    providers
        .iter()
        .any(|p| adapter_files(p).iter().any(|f| f == path))
}

// The trait methods of `Agents` that have a default, which refuses or
// tracks nothing.
fn agents_defaults() -> Vec<String> {
    let lines = code_lines(&root().join("crates/core/src/ports/agent.rs"));
    let start = lines
        .iter()
        .position(|l| l.starts_with("pub trait Agents"))
        .expect("the Agents trait");
    let mut defaults = Vec::new();
    let mut open: Option<String> = None;
    for line in &lines[start + 1..] {
        if line == "}" {
            break;
        }
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("fn ") {
            open = rest.split('(').next().map(str::to_owned);
        }
        let signed = trimmed.ends_with('{') || trimmed.ends_with(';');
        if let Some(name) = open.take_if(|_| signed)
            && trimmed.ends_with('{')
        {
            defaults.push(name);
        }
    }
    defaults
}

// The adapter's own files, which define its `impl Agents`.
fn adapter_files(provider: &str) -> Vec<PathBuf> {
    let command = harness(provider).command();
    let adapters = root().join("crates/adapters/src/adapters");
    let mut files = vec![];
    let file = adapters.join(format!("{command}.rs"));
    if file.is_file() {
        files.push(file);
    }
    let folder = adapters.join(command);
    if folder.is_dir() {
        sources(&folder, &mut files);
    }
    files
}

mod tests;
