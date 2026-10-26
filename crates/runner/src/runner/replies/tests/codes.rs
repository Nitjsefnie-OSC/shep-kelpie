//! Whether a reply's code is spent, and what a reply that spends none costs
//!
//! Probed in review: a code in a reply that answered nothing stayed
//! unclaimed, and a reader of the topic reused it to merge.

use super::*;

#[test]
fn a_right_code_is_spent_even_when_its_reply_answers_nothing() {
    let (rig, runner, _) = alerted("koji");
    let code = rig.reply("koji 71");
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::ReplyUnreadable { line_failed: None })
    );
    rig.alerts
        .reply(&format!("koji 71 yes {code}"), rig.clock.now());
    rig.clock.advance(READ_EVERY);
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::ReplyCodeUsed {
            id: ticket(71),
            line_failed: None
        })
    );
    assert_eq!(
        phase(&rig, &runner),
        json!({ "state": "ruling", "id": "71" })
    );
}

// Every project reads the one topic, and a reply may name another project.
#[test]
fn a_reply_for_another_project_answers_nothing_here() {
    let (rig, runner, _) = alerted("koji");
    let code = rig.reply("rotom 71 yes");
    assert_eq!(step(&runner).unwrap(), Some(StepReport::ReplyIgnored));
    assert_eq!(
        phase(&rig, &runner),
        json!({ "state": "ruling", "id": "71" })
    );
    assert_eq!(lines(&rig), [""; 0]);

    // Its code is spent all the same.
    rig.alerts
        .reply(&format!("koji 71 yes {code}"), rig.clock.now());
    rig.clock.advance(READ_EVERY);
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::ReplyCodeUsed { id, .. }) if id == ticket(71)
    ));
}

#[test]
fn five_wrong_codes_turn_answers_off_until_the_terminal_turns_them_on() {
    let (rig, runner, _) = alerted("koji");
    let now = rig.clock.now();
    let right = rig.code_at(now);
    let wrong = |n: u32| format!("{:06}", (right.parse::<u32>().unwrap() + n) % 1_000_000);
    for n in 1..5 {
        rig.alerts.reply(&format!("koji 71 yes {}", wrong(n)), now);
        assert_eq!(
            step(&runner).unwrap(),
            Some(StepReport::ReplyIgnored),
            "{n}"
        );
        rig.clock.advance(READ_EVERY);
    }
    rig.alerts.reply(&format!("koji 71 yes {}", wrong(5)), now);
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::RepliesLocked { line_failed: None })
    );
    assert_eq!(
        lines(&rig),
        ["Answers from ntfy are off after 5 wrong codes. \
          Turn them back on with `shep kelpie totp --unlock` on the terminal. \
          Anyone can post to a topic whose name they know, so think \
          about moving to a new one."]
    );
    assert!(!lock(&runner).awaits_reply());

    // The right code of the moment answers nothing now, and is not read.
    rig.clock.advance(STEP);
    rig.reply("koji 71 yes");
    rig.clock.advance(READ_EVERY);
    step(&runner).unwrap();
    assert_eq!(
        phase(&rig, &runner),
        json!({ "state": "ruling", "id": "71" })
    );

    crate::totp::answers::Answers::in_folder(rig.paths().totp)
        .unlock()
        .unwrap();
    rig.clock.advance(STEP);
    rig.reply("koji 71 yes");
    rig.clock.advance(READ_EVERY);
    let mut answered = false;
    for _ in 0..4 {
        answered |= step(&runner).unwrap() == Some(StepReport::ReplyAnswered { id: ticket(71) });
    }
    assert!(answered);
}

#[cfg(unix)]
#[test]
fn a_secret_others_may_read_stops_the_runner() {
    use std::os::unix::fs::PermissionsExt;
    let rig = Rig::new("koji");
    let secret = rig.paths().totp.join("secret");
    std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o644)).unwrap();
    let Err(OpenError::Settings(e)) = rig.open() else {
        panic!("the runner started");
    };
    assert!(e.to_string().contains("may be read by others"), "{e}");
}

// Probed in review: a right code in any shape but the exact one was never
// claimed, so a reader of the topic could reuse it with the exact shape.
#[test]
fn a_right_code_in_any_shape_is_spent_before_a_replay() {
    for shape in [
        "koji 71 no rename it {code}.",
        "{code} koji 71 no rename it",
        "koji 71 no rename it {spaced}",
        "{code}",
        "koji 71 no rename it {wide}",
        "koji 71 no rename it {dashed}",
        "koji 71 no rename it {arabic}",
    ] {
        let (rig, runner, _) = alerted("koji");
        let now = rig.clock.now();
        let code = rig.code_at(now);
        let spaced = format!("{} {}", &code[..3], &code[3..]);
        let wide: String = code
            .chars()
            .map(|c| char::from_u32(u32::from(c) - u32::from('0') + 0xff10).unwrap())
            .collect();
        let dashed = format!("{}-{}", &code[..3], &code[3..]);
        let arabic: String = code
            .chars()
            .map(|c| char::from_u32(u32::from(c) - u32::from('0') + 0x660).unwrap())
            .collect();
        let sent = shape
            .replace("{dashed}", &dashed)
            .replace("{arabic}", &arabic)
            .replace("{code}", &code)
            .replace("{spaced}", &spaced)
            .replace("{wide}", &wide);
        rig.alerts.reply(&sent, now);
        assert_eq!(
            step(&runner).unwrap(),
            Some(StepReport::ReplyIgnored),
            "{sent}"
        );
        rig.alerts.reply(&format!("koji 71 yes {code}"), now);
        rig.clock.advance(READ_EVERY);
        assert!(
            matches!(
                step(&runner).unwrap(),
                Some(StepReport::ReplyCodeUsed { id, .. }) if id == ticket(71)
            ),
            "{sent}"
        );
        assert_eq!(
            phase(&rig, &runner),
            json!({ "state": "ruling", "id": "71" }),
            "{sent}"
        );
    }
}

// A reply just too old to act on still spends its code, so a replay just
// inside the window finds it used.
#[test]
fn a_reply_older_than_the_window_is_read_only_to_spend_its_code() {
    let (rig, runner, _) = alerted("koji");
    let now = rig.clock.now();
    let sent = Timestamp(now.0 - WINDOW - 10);
    let code = rig.code_at(sent);
    rig.alerts
        .reply(&format!("koji 71 no not this {code}"), sent);
    rig.alerts
        .reply(&format!("koji 71 yes {code}"), Timestamp(sent.0 + 20));
    assert_eq!(step(&runner).unwrap(), Some(StepReport::ReplyIgnored));
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::ReplyCodeUsed { id, .. }) if id == ticket(71)
    ));
    assert_eq!(
        phase(&rig, &runner),
        json!({ "state": "ruling", "id": "71" })
    );
}

// Probed in review: an attacker's fifth wrong code, read with the
// maintainer's reply, turned answers off before that reply's code was
// spent, and a replay after `--unlock` merged.
#[test]
fn a_code_is_spent_even_when_answers_are_off() {
    let (rig, runner, _) = alerted("koji");
    let now = rig.clock.now();
    let right = rig.code_at(now);
    let wrong = |n: u32| format!("{:06}", (right.parse::<u32>().unwrap() + n) % 1_000_000);
    for n in 1..=5 {
        rig.alerts.reply(&format!("koji 71 yes {}", wrong(n)), now);
    }
    rig.alerts
        .reply(&format!("koji 71 no rename it {right}"), now);
    for _ in 1..5 {
        assert_eq!(step(&runner).unwrap(), Some(StepReport::ReplyIgnored));
    }
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::RepliesLocked { .. })
    ));
    step(&runner).unwrap();

    rig.clock.advance(25);
    rig.alerts
        .reply(&format!("koji 71 yes {right}"), rig.clock.now());
    crate::totp::answers::Answers::in_folder(rig.paths().totp)
        .unlock()
        .unwrap();
    rig.clock.advance(READ_EVERY);
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::ReplyCodeUsed { id, .. }) if id == ticket(71)
    ));
    assert_eq!(
        phase(&rig, &runner),
        json!({ "state": "ruling", "id": "71" })
    );
}

// Only a message's text answers, but a code anywhere in a post is spent:
// in its title, or in a post kelpie reads as its own.
#[test]
fn a_code_in_a_title_or_a_tagged_post_is_spent() {
    for (text, others) in [
        (Some("koji 71 no rename it"), vec!["{code}"]),
        (None, vec!["koji 71 no {code}", "kelpie"]),
    ] {
        let (rig, runner, _) = alerted("koji");
        let now = rig.clock.now();
        let code = rig.code_at(now);
        let others: Vec<String> = others.iter().map(|o| o.replace("{code}", &code)).collect();
        let others: Vec<&str> = others.iter().map(String::as_str).collect();
        rig.alerts.post_raw(text, &others, now);
        rig.alerts.reply(&format!("koji 71 yes {code}"), now);
        let mut used = false;
        for _ in 0..3 {
            used |= matches!(
                step(&runner).unwrap(),
                Some(StepReport::ReplyCodeUsed { id, .. }) if id == ticket(71)
            );
        }
        assert!(used, "{text:?} {others:?}");
        assert_eq!(
            phase(&rig, &runner),
            json!({ "state": "ruling", "id": "71" })
        );
    }
}

// ntfy turns a message over 4,095 bytes into an attachment, whose text
// kelpie never reads, so any code in it would stay unspent.
#[test]
fn a_post_too_long_to_read_spends_every_step_it_could_name() {
    let (rig, runner, _) = alerted("koji");
    let now = rig.clock.now();
    let code = rig.code_at(now);
    rig.alerts.post_cut(
        Some("You received a file: attachment.txt"),
        &["attachment.txt"],
        true,
        now,
    );
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::ReplyTooLong { line_failed: None })
    );
    assert_eq!(
        lines(&rig),
        [
            "That reply was too long to read, so it answered nothing and any code \
          in it is spent. Send a shorter reply with a new code."
        ]
    );
    for sent in [now, Timestamp(now.0 + STEP)] {
        rig.alerts.reply(&format!("koji 71 yes {code}"), sent);
        rig.clock.advance(READ_EVERY);
        assert!(
            matches!(
                step(&runner).unwrap(),
                Some(StepReport::ReplyCodeUsed { id, .. }) if id == ticket(71)
            ),
            "{sent:?}"
        );
    }
    assert_eq!(
        phase(&rig, &runner),
        json!({ "state": "ruling", "id": "71" })
    );
}

// A phone whose clock runs ahead sends the next step's code, which a
// reader could replay once that step comes.
#[test]
fn a_code_for_the_next_step_is_spent_before_it_comes() {
    let (rig, runner, _) = alerted("koji");
    let now = rig.clock.now();
    let ahead = rig.code_at(Timestamp(now.0 + STEP));
    rig.alerts
        .reply(&format!("koji 71 no rename it {ahead}"), now);
    assert_eq!(step(&runner).unwrap(), Some(StepReport::ReplyIgnored));
    rig.clock.advance(STEP);
    rig.alerts
        .reply(&format!("koji 71 yes {ahead}"), rig.clock.now());
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::ReplyCodeUsed { id, .. }) if id == ticket(71)
    ));
    assert_eq!(
        phase(&rig, &runner),
        json!({ "state": "ruling", "id": "71" })
    );
}

// Probed in review: a secret that could not be read, or a claim that could
// not be written, let the read move past the reply with its code unspent,
// and a replay behind it answered.
#[cfg(unix)]
#[test]
fn a_code_that_cannot_be_spent_holds_the_reply_until_it_can() {
    use std::os::unix::fs::PermissionsExt;
    let set = |path: &std::path::Path, mode| {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    };
    for fault in ["the secret others may read", "no claim can be written"] {
        let (rig, runner, _) = alerted("koji");
        let totp = rig.paths().totp;
        let (path, broken, fixed) = if fault.starts_with("the secret") {
            (totp.join("secret"), 0o644, 0o600)
        } else {
            crate::totp::private_dir(&totp.join("used")).unwrap();
            (totp.join("used"), 0o500, 0o700)
        };
        let now = rig.clock.now();
        let code = rig.code_at(now);
        set(&path, broken);
        rig.alerts
            .reply(&format!("koji 71 no rename it {code}"), now);
        rig.alerts.reply(&format!("koji 71 yes {code}"), now);
        let held = step(&runner).unwrap();
        assert!(
            matches!(held, Some(StepReport::RepliesFailed { .. })),
            "{fault}: {held:?}"
        );
        assert_eq!(step(&runner).unwrap(), None, "{fault}: held, not skipped");
        assert_eq!(lines(&rig), [""; 0], "{fault}: nothing says a code is live");

        set(&path, fixed);
        rig.clock.advance(READ_EVERY);
        let mut seen = Vec::new();
        for _ in 0..4 {
            seen.extend(step(&runner).unwrap());
        }
        assert!(
            seen.contains(&StepReport::ReplyAnswered { id: ticket(71) }),
            "{fault}: {seen:?}"
        );
        assert!(
            seen.iter()
                .any(|r| matches!(r, StepReport::ReplyCodeUsed { id, .. } if *id == ticket(71))),
            "{fault}: {seen:?}"
        );
        let phase = phase(&rig, &runner);
        assert_ne!(phase["state"], "merge", "{fault}: {phase}");
    }
}

// Probed in review: a reply spends the next step's code, which a replay
// can still send three steps later. With kelpie down, a read that starts
// only two steps before the window missed the reply but read the replay.
#[test]
fn a_replay_of_a_next_step_code_after_a_gap_finds_it_spent() {
    let (rig, runner, _) = alerted("koji");
    // The start of a step to come, so the replay lands two steps later.
    let sent = (rig.clock.now().0 / STEP + 1) * STEP;
    let ahead = rig.code_at(Timestamp(sent + STEP));
    rig.alerts
        .reply(&format!("koji 71 no rename it {ahead}"), Timestamp(sent));
    rig.alerts
        .reply(&format!("koji 71 yes {ahead}"), Timestamp(sent + 85));
    // Kelpie comes back with the replay just inside the window.
    let back = sent + WINDOW + 80;
    rig.clock.advance(back - rig.clock.now().0);
    let mut seen = Vec::new();
    for _ in 0..3 {
        seen.extend(step(&runner).unwrap());
    }
    assert!(
        seen.iter()
            .any(|r| matches!(r, StepReport::ReplyCodeUsed { id, .. } if *id == ticket(71))),
        "{seen:?}"
    );
    assert_eq!(
        phase(&rig, &runner),
        json!({ "state": "ruling", "id": "71" })
    );
}
