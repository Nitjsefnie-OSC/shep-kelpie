//! The kind of work a pull request's title names

/// The kind of work a pull request titled `title` is, as the ledger and the
/// retro split by it
///
/// A conventional title is `type(scope)!: summary`. The kind is its type,
/// `breaking` for one marked with `!`, or `untyped` for a title that is not
/// conventional.
pub fn work_kind(title: &str) -> String {
    let untyped = "untyped".to_owned();
    let Some((head, _)) = title.trim().split_once(':') else {
        return untyped;
    };
    let (head, breaking) = match head.strip_suffix('!') {
        Some(head) => (head, true),
        None => (head, false),
    };
    let (kind, scope) = match head.split_once('(') {
        Some((kind, scope)) => (kind, Some(scope)),
        None => (head, None),
    };
    let scoped = scope.is_none_or(|scope| scope.ends_with(')'));
    let word = !kind.is_empty() && kind.chars().all(|c| c.is_ascii_lowercase());
    match (word && scoped, breaking) {
        (false, _) => untyped,
        (true, true) => "breaking".to_owned(),
        (true, false) => kind.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_type_is_named_for_the_ledger() {
        assert_eq!(work_kind("fix(guard): x"), "fix");
        assert_eq!(work_kind("docs: x"), "docs");
        assert_eq!(work_kind("feat!: x"), "breaking");
        assert_eq!(work_kind("Add a thing"), "untyped");
        assert_eq!(work_kind("fix(unclosed: x"), "untyped");
        assert_eq!(work_kind(""), "untyped");
    }
}
