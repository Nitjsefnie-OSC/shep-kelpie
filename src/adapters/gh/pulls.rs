//! The pull request kelpie opens and edits for a worker whose token cannot
//!
//! A worker on a repo kelpie's App covers holds a token that pushes and
//! does nothing else, so it hands over its pull request's title and body in
//! a file and kelpie opens it, as the login `gh` acts as, the same one the
//! worker opened it with before.

use super::gh;
use crate::ports::ForgeError;
use crate::settings::ForgeSlug;

pub(super) fn create(
    repo: &ForgeSlug,
    head: &str,
    base: &str,
    title: &str,
    body: &str,
) -> Result<u64, ForgeError> {
    let out = run(&create_args(repo, head, base, title, body))?;
    let text = String::from_utf8_lossy(&out);
    text.trim()
        .parse()
        .map_err(|_| ForgeError::Unreadable(text.into_owned()))
}

pub(super) fn edit(
    repo: &ForgeSlug,
    number: u64,
    title: &str,
    body: &str,
) -> Result<(), ForgeError> {
    run(&edit_args(repo, number, title, body)).map(drop)
}

fn run(args: &[String]) -> Result<Vec<u8>, ForgeError> {
    gh(&args.iter().map(String::as_str).collect::<Vec<_>>())
}

// A draft, always: kelpie marks it ready itself, when its gates pass.
fn create_args(repo: &ForgeSlug, head: &str, base: &str, title: &str, body: &str) -> Vec<String> {
    let path = format!("repos/{}/pulls", repo.as_str());
    let fields = [
        format!("title={title}"),
        format!("body={body}"),
        format!("head={head}"),
        format!("base={base}"),
    ];
    let mut args = ["api", "--method", "POST", &path]
        .map(str::to_owned)
        .to_vec();
    for field in fields {
        args.extend(["-f".to_owned(), field]);
    }
    args.extend(["-F", "draft=true", "--jq", ".number"].map(str::to_owned));
    args
}

fn edit_args(repo: &ForgeSlug, number: u64, title: &str, body: &str) -> Vec<String> {
    let path = format!("repos/{}/pulls/{number}", repo.as_str());
    let mut args = ["api", "--method", "PATCH", &path]
        .map(str::to_owned)
        .to_vec();
    for field in [format!("title={title}"), format!("body={body}")] {
        args.extend(["-f".to_owned(), field]);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> ForgeSlug {
        ForgeSlug::try_from("shep-pm/koji".to_owned()).unwrap()
    }

    // `-f` sends the text as it is, where `--title` or a body flag could be
    // read as a flag when the text begins with a dash; `-F draft=true` is
    // the one typed field.
    #[test]
    fn a_pull_request_is_opened_as_a_draft_with_its_text_sent_as_fields() {
        let args = create_args(
            &repo(),
            "kelpie/7",
            "main",
            "feat: x",
            "- body\n\nResolves #7",
        );

        assert_eq!(
            args,
            [
                "api",
                "--method",
                "POST",
                "repos/shep-pm/koji/pulls",
                "-f",
                "title=feat: x",
                "-f",
                "body=- body\n\nResolves #7",
                "-f",
                "head=kelpie/7",
                "-f",
                "base=main",
                "-F",
                "draft=true",
                "--jq",
                ".number",
            ]
        );
    }

    #[test]
    fn an_edit_changes_the_title_and_body_of_the_pull_request_alone() {
        assert_eq!(
            edit_args(&repo(), 12, "fix: y", "body"),
            [
                "api",
                "--method",
                "PATCH",
                "repos/shep-pm/koji/pulls/12",
                "-f",
                "title=fix: y",
                "-f",
                "body=body"
            ]
        );
    }
}
