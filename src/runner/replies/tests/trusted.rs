//! A trusted topic: only the maintainer's token can post to it, so a reply
//! needs no code except a yes on a merge ruling

use super::*;

// The rig's kelpie settings with its ntfy webhook locked to a token
fn trusted(rig: &Rig) {
    rig.set_kelpie_settings(&format!(
        "[webhook]\nkind = \"ntfy\"\nurl = \"{}\"\ntoken = \"tk_s3cr3t\"\ntrusted = true\n",
        Rig::WEBHOOK_URL
    ));
}

// A merge ruling alerted on the trusted topic
fn alerted_trusted(project: &str) -> (Rig, Mutex<Runner>, String) {
    let (rig, runner, head) = Rig::parked_set(project, trusted);
    assert!(rig.webhook().trusted);
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    (rig, runner, head)
}

#[test]
fn a_no_and_a_rework_need_no_code_on_a_trusted_topic() {
    let (rig, runner, _) = alerted_trusted("koji");
    rig.alerts.reply("1 no rename the flag", rig.clock.now());
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::ReplyAnswered { id: 1 })
    );
    assert_ne!(phase(&rig, &runner), json!({ "state": "ruling", "id": 1 }));
    assert_eq!(lines(&rig), [""; 0]);

    let (rig, runner, _) = alerted_trusted("koji");
    rig.alerts
        .reply("koji 1 Rework split the parser out", rig.clock.now());
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::ReplyAnswered { id: 1 })
    );
    rig.claude.script([Scripted::Push("split.txt", "split\n")]);
    step(&runner).unwrap();
    let noted = rig.claude.calls().pop().unwrap();
    assert!(
        noted.prompt.contains("split the parser out"),
        "{}",
        noted.prompt
    );
}

#[test]
fn a_workers_answer_needs_no_code_on_a_trusted_topic() {
    let rig = Rig::new("rotom");
    trusted(&rig);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude.script([Scripted::Say(
        "Done.\n\n<kelpie-question>\nShould it be `--dry-run` or `--check`?\n</kelpie-question>\n",
    )]);
    step(&runner).unwrap();
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));

    rig.alerts
        .reply("1 use --dry-run, it matches shep.", rig.clock.now());
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::ReplyAnswered { id: 1 })
    );
    rig.claude.script([Scripted::Say("renamed")]);
    step(&runner).unwrap();
    let [_, answered] = rig.claude.calls().try_into().unwrap();
    assert_eq!(
        answered.prompt,
        "The maintainer answered your question:\n\nuse --dry-run, it matches shep.\n"
    );
}

#[test]
fn a_yes_on_a_merge_ruling_is_refused_without_a_code_and_taken_with_one() {
    let (rig, runner, head) = alerted_trusted("koji");
    rig.alerts.reply("1 yes", rig.clock.now());
    let reason = "a yes on a merge ruling takes the authenticator code: `1 yes <code>`";
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::ReplyRefused {
            id: 1,
            reason: reason.into(),
            line_failed: None
        })
    );
    assert_eq!(
        lines(&rig),
        [format!("Ruling 1 was not answered: {reason}.")]
    );
    assert_eq!(phase(&rig, &runner), json!({ "state": "ruling", "id": 1 }));
    assert_eq!(rig.forge.merges(), []);

    rig.clock.advance(READ_EVERY);
    rig.reply("1 yes");
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::ReplyAnswered { id: 1 })
    );
    assert_eq!(
        phase(&rig, &runner),
        json!({ "state": "merge", "head": head, "readied": null })
    );
}

#[test]
fn a_yes_to_any_other_ruling_needs_no_code_on_a_trusted_topic() {
    let rig = Rig::new("rotom");
    trusted(&rig);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude.script([Scripted::Say(
        "Done.\n\n<kelpie-question>\nShall I keep the old flag?\n</kelpie-question>\n",
    )]);
    step(&runner).unwrap();
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    rig.alerts.reply("1 yes", rig.clock.now());
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::ReplyAnswered { id: 1 })
    );
}

// Chatter on the topic is not an answer, and not answered with a line.
#[test]
fn text_that_names_no_ruling_here_is_left_alone_on_a_trusted_topic() {
    let (rig, runner, _) = alerted_trusted("koji");
    for text in [
        "hello from the phone",
        "shep kelpie rule yes",
        "2 no not ours",
        "other 1 no not ours",
    ] {
        rig.alerts.reply(text, rig.clock.now());
        assert_eq!(
            step(&runner).unwrap(),
            Some(StepReport::ReplyIgnored),
            "{text}"
        );
        rig.clock.advance(READ_EVERY);
    }
    assert_eq!(phase(&rig, &runner), json!({ "state": "ruling", "id": 1 }));
    assert_eq!(lines(&rig), [""; 0]);
}

#[test]
fn an_untrusted_topic_still_takes_no_reply_without_a_code() {
    let (rig, runner, _) = alerted("koji");
    rig.alerts.reply("1 no rename the flag", rig.clock.now());
    assert_eq!(step(&runner).unwrap(), Some(StepReport::ReplyIgnored));
    assert_eq!(phase(&rig, &runner), json!({ "state": "ruling", "id": 1 }));
}

#[test]
fn five_wrong_codes_turn_coded_answers_off_and_leave_uncoded_ones() {
    let (rig, runner, _) = alerted_trusted("koji");
    let now = rig.clock.now();
    let right = rig.code_at(now);
    let wrong = |n: u32| format!("{:06}", (right.parse::<u32>().unwrap() + n) % 1_000_000);
    for n in 1..5 {
        rig.alerts.reply(&format!("koji 1 yes {}", wrong(n)), now);
        assert_eq!(step(&runner).unwrap(), Some(StepReport::ReplyIgnored));
        rig.clock.advance(READ_EVERY);
    }
    rig.alerts.reply(&format!("koji 1 yes {}", wrong(5)), now);
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::RepliesLocked { line_failed: None })
    );
    assert_eq!(
        lines(&rig),
        [
            "Coded answers from ntfy are off after 5 wrong codes, and replies that need no \
          code still work. Turn them back on with `shep kelpie totp --unlock` on the \
          terminal."
        ]
    );
    assert!(lock(&runner).awaits_reply(), "the topic is still read");

    // The right code answers nothing now, but an uncoded no still does.
    rig.clock.advance(STEP);
    rig.reply("koji 1 yes");
    rig.clock.advance(READ_EVERY);
    assert_eq!(step(&runner).unwrap(), Some(StepReport::ReplyIgnored));
    assert_eq!(phase(&rig, &runner), json!({ "state": "ruling", "id": 1 }));

    rig.alerts.reply("1 no rename the flag", rig.clock.now());
    rig.clock.advance(READ_EVERY);
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::ReplyAnswered { id: 1 })
    );
}

// A coded yes is dropped while coded answers are off, so a merge ruling
// alerted then must not offer one, and a bare yes is told how to proceed.
#[test]
fn a_merge_alerted_after_the_lock_does_not_offer_a_coded_yes() {
    let (rig, runner, _) = Rig::parked_set("koji", |rig| {
        trusted(rig);
        let answers = crate::totp::answers::Answers::in_folder(rig.paths().totp);
        for n in 0..5 {
            answers.fail(&format!("wrong{n}")).unwrap();
        }
        assert!(answers.locked());
    });
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    let alert = &rig.alerts.posts()[0].1;
    assert_eq!(
        alert.reply.as_ref().map(|r| r.takes),
        Some(Takes::MergeCodedOff)
    );

    rig.alerts.reply("1 yes", rig.clock.now());
    let StepReport::ReplyRefused { reason, .. } = step(&runner).unwrap().unwrap() else {
        panic!("a bare yes on a merge ruling is refused");
    };
    assert!(reason.contains("shep kelpie totp --unlock"), "{reason}");
    assert_eq!(phase(&rig, &runner), json!({ "state": "ruling", "id": 1 }));
}
