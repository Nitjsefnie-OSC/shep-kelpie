//! The folders and files of a repo's default branch

use serde::Deserialize;

use super::{gh, unreadable};
use crate::ports::{FolderEntry, ForgeError};
use crate::settings::ForgeSlug;

pub(super) fn folder(repo: &ForgeSlug, path: &str) -> Result<Option<Vec<FolderEntry>>, ForgeError> {
    match gh(&["api", &endpoint(repo, path)]) {
        Ok(stdout) => parse_folder(&stdout).map(Some),
        Err(e) if is_not_found(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

pub(super) fn file_text(repo: &ForgeSlug, path: &str) -> Result<String, ForgeError> {
    let stdout = gh(&[
        "api",
        "--header",
        "Accept: application/vnd.github.raw+json",
        &endpoint(repo, path),
    ])?;
    String::from_utf8(stdout).map_err(|e| unreadable(e.as_bytes()))
}

fn endpoint(repo: &ForgeSlug, path: &str) -> String {
    format!("repos/{}/contents/{path}", repo.as_str())
}

// GitHub answers a repo or path it does not have with a 404, which `gh` reports on stderr.
fn is_not_found(error: &ForgeError) -> bool {
    matches!(error, ForgeError::Failed(stderr) if stderr.contains("HTTP 404"))
}

// A path that is a file answers with one object and not a list, and is no folder.
fn parse_folder(stdout: &[u8]) -> Result<Vec<FolderEntry>, ForgeError> {
    #[derive(Deserialize)]
    struct Entry {
        name: String,
        #[serde(rename = "type")]
        kind: String,
    }
    let entries: Vec<Entry> = serde_json::from_slice(stdout).map_err(|_| unreadable(stdout))?;
    Ok(entries
        .into_iter()
        .map(|e| FolderEntry {
            is_dir: e.kind == "dir",
            name: e.name,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The shape of GitHub's answer for a folder, as `gh api repos/{repo}/contents/.github` prints it
    const FOLDER: &str = r#"[
        {"name":"PULL_REQUEST_TEMPLATE","path":".github/PULL_REQUEST_TEMPLATE","sha":"1d2f","size":0,"type":"dir"},
        {"name":"pull_request_template.md","path":".github/pull_request_template.md","sha":"9a3c","size":58,"type":"file"}
    ]"#;

    #[test]
    fn a_folder_lists_its_files_and_folders() {
        assert_eq!(
            parse_folder(FOLDER.as_bytes()).unwrap(),
            vec![
                FolderEntry {
                    name: "PULL_REQUEST_TEMPLATE".into(),
                    is_dir: true
                },
                FolderEntry {
                    name: "pull_request_template.md".into(),
                    is_dir: false
                },
            ]
        );
    }

    #[test]
    fn a_file_answers_no_folder_listing() {
        let file = r#"{"name":"README.md","path":"README.md","type":"file","content":"IyBoaQ=="}"#;
        assert!(matches!(
            parse_folder(file.as_bytes()),
            Err(ForgeError::Unreadable(_))
        ));
    }

    #[test]
    fn only_a_404_is_a_missing_path() {
        let failed = |said: &str| ForgeError::Failed(said.into());
        assert!(is_not_found(&failed("gh: Not Found (HTTP 404)\n")));
        assert!(!is_not_found(&failed("gh: Bad credentials (HTTP 401)\n")));
        assert!(!is_not_found(&ForgeError::Spawn("HTTP 404".into())));
    }
}
