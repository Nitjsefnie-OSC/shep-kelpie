//! Whether the line asks for help, read the way `split_project` reads it

/// Whether `--help` or `-h` is a whole argument of `command`'s line before a `--`
///
/// A project's name after `-p`, `--project` or `--project=` is not a word, and a
/// ruling's answer, after its first two words, is all words.
pub(super) fn asked(command: &str, args: &[String]) -> bool {
    let words_from = (command == "rule").then_some(2);
    let mut words = 0;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if words_from.is_some_and(|from| words >= from) {
            return false;
        }
        match arg.as_str() {
            "--" => return false,
            "--help" | "-h" => return true,
            "-p" | "--project" => {
                args.next();
            }
            other if other.starts_with("--project=") => {}
            _ => words += 1,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flock::VERBS;

    fn line(words: &str) -> Vec<String> {
        words.split(' ').map(str::to_owned).collect()
    }

    #[test]
    fn help_anywhere_before_a_double_dash_is_asked_for_every_verb() {
        for verb in VERBS {
            for flag in ["--help", "-h"] {
                for words in [
                    flag.to_owned(),
                    format!("{flag} koji"),
                    format!("koji {flag}"),
                    format!("one {flag} two"),
                    format!("-p koji {flag}"),
                    format!("{flag} --project=koji"),
                ] {
                    assert!(asked(verb, &line(&words)), "{verb} {words}");
                }
            }
        }
    }

    #[test]
    fn help_after_a_double_dash_is_a_word() {
        assert!(!asked("tell", &line("-- see --help")));
        assert!(!asked("add", &line("koji -- -h")));
    }

    #[test]
    fn a_ruling_s_answer_keeps_help_as_its_own_words() {
        for words in ["-h", "14 --help", "-p koji 14 -h"] {
            assert!(asked("rule", &line(words)), "{words}");
        }
        assert!(!asked("rule", &line("14 no try -h")));
    }

    #[test]
    fn a_line_without_help_is_not_help() {
        for words in ["--helpful", "-hx", "--json", "-p koji"] {
            assert!(!asked("status", &line(words)), "{words}");
        }
        assert!(!asked("status", &[]));
    }
}
