use super::*;

fn vote(choice: char, why: &str, design: bool) -> Vote {
    Vote {
        choice,
        why: why.into(),
        design,
    }
}

#[test]
fn a_reply_ending_in_the_object_is_the_vote() {
    let text = "I read the diff.\n\n```json\n{\"choice\": \"B\", \"why\": \"CI is red.\", \
                \"design\": false}\n```";
    assert_eq!(read(text, 2), Ok(vote('B', "CI is red.", false)));
}

#[test]
fn the_last_object_with_a_choice_counts_and_braces_in_a_string_do_not() {
    let text = "{\"choice\": \"A\", \"why\": \"first\", \"design\": false}\n\
                On second thought:\n\
                {\"choice\": \"b\", \"why\": \"use {\\\"choice\\\": \\\"A\\\"} never\", \
                \"design\": true}";
    assert_eq!(
        read(text, 3),
        Ok(vote('B', "use {\"choice\": \"A\"} never", true))
    );
}

#[test]
fn a_choice_outside_the_options_is_no_vote() {
    let text = "{\"choice\": \"C\", \"why\": \"x\", \"design\": false}";
    let err = read(text, 2).unwrap_err();
    assert!(err.contains("there are 2 options"), "{err}");
    let text = "{\"choice\": \"AB\", \"why\": \"x\", \"design\": false}";
    assert!(read(text, 2).unwrap_err().contains("not one option's key"));
}

#[test]
fn a_reply_missing_a_field_or_with_none_is_no_vote() {
    assert!(
        read("A, I think.", 2)
            .unwrap_err()
            .contains("no JSON object")
    );
    let no_design = "{\"choice\": \"A\", \"why\": \"x\"}";
    assert!(read(no_design, 2).unwrap_err().contains("`design`"));
    let no_why = "{\"choice\": \"A\", \"design\": false}";
    assert!(read(no_why, 2).unwrap_err().contains("`why`"));
    let number = "{\"choice\": 1, \"why\": \"x\", \"design\": false}";
    assert!(read(number, 2).unwrap_err().contains("`choice`"));
}

#[test]
fn a_why_is_one_line_cut_at_350_characters() {
    let long = "word ".repeat(100);
    let text =
        serde_json::json!({"choice": "A", "why": format!("two\nlines\t{long}"), "design": false});
    let got = read(&text.to_string(), 1).unwrap();
    assert!(got.why.starts_with("two lines word word"), "{}", got.why);
    assert_eq!(got.why.chars().count(), WHY_LONGEST);
    assert!(got.why.ends_with('…'));
}
