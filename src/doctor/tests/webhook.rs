use super::*;

#[tokio::test]
async fn rulings_with_no_webhook_are_fine_and_say_where_they_appear() {
    let scene = Scene::new().await;
    scene.shepherd.holds_section("");
    let found = ok(&scene.report().await, "koji: rulings");
    assert!(found.contains("only in the log"), "{found}");
    assert!(found.contains("shep kelpie rule"), "{found}");
}

#[tokio::test]
async fn an_old_settings_file_with_no_table_or_section_is_missing_and_named() {
    let scene = Scene::new().await;
    scene.shepherd.holds_section("");
    let project = scene.kelpie_home.join("golbat");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("settings.toml"), "forge = \"a/b\"\n").unwrap();
    std::fs::write(scene.kelpie_home.join("settings.toml"), WEBHOOK).unwrap();
    let report = scene.report().await;

    let (what, fix) = missing(&report, "golbat: settings");
    assert_eq!(
        what,
        format!(
            "there is no [app.dogs.kelpie] table on the golbat sheep, and kelpie no longer \
             reads {}: move its keys into the table",
            project.join("settings.toml").display()
        )
    );
    assert!(fix.contains("`[app.dogs.kelpie]` table"), "{fix}");
    let (what, _) = missing(&report, "kelpie settings");
    assert!(
        what.contains("kelpie no longer reads") && what.ends_with("move its keys into the section"),
        "{what}"
    );
    assert!(!report.passed());

    // Where the table and the section are set, the old files are not mentioned.
    scene.shepherd.holds_section(WEBHOOK);
    std::fs::remove_dir_all(&project).unwrap();
    let report = scene.report().await;
    assert!(!subjects(&report).contains(&"golbat: settings"));
    assert!(!subjects(&report).contains(&"kelpie settings"));
}

#[tokio::test]
async fn a_test_alert_posts_once_to_the_webhook_and_only_when_asked() {
    let scene = Scene::new().await;
    scene.report().await;
    assert_eq!(scene.alerts.posts(), []);

    let ask = Ask {
        test_alert: true,
        ..Ask::default()
    };
    let report = scene.check(ask).await;

    assert!(ok(&report, "test alert").starts_with("posted"));
    let posts = scene.alerts.posts();
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert_eq!(posts[0].0.url.expose(), SECRET);
    assert_eq!(posts[0].1.title, "kelpie doctor");
    assert!(posts[0].1.reply.is_none());
    let printed = report.render().join("\n");
    assert!(!printed.contains("s3cr3t"), "{printed}");
}

#[tokio::test]
async fn a_test_alert_the_webhook_refuses_is_missing_and_never_prints_its_url() {
    let scene = Scene::new().await;
    scene.alerts.set_down(true);
    let ask = Ask {
        test_alert: true,
        ..Ask::default()
    };
    let report = scene.check(ask).await;
    let (what, fix) = missing(&report, "test alert");
    assert!(what.contains("HTTP 503"), "{what}");
    assert!(fix.contains("webhook.url"), "{fix}");
    assert!(!report.render().join("\n").contains("s3cr3t"));
    assert!(!report.passed());
}

#[tokio::test]
async fn a_test_alert_with_no_webhook_says_there_is_none_and_posts_nothing() {
    let scene = Scene::new().await;
    scene.shepherd.holds_section("");
    let ask = Ask {
        test_alert: true,
        ..Ask::default()
    };
    let report = scene.check(ask).await;
    let Verdict::Unsure { what, .. } = verdict(&report, "test alert") else {
        panic!("{:#?}", report.render());
    };
    assert_eq!(what, "there is no webhook to post to");
    assert_eq!(scene.alerts.posts(), []);
}

#[tokio::test]
async fn a_trusted_topic_is_said_to_take_replies_without_a_code() {
    let scene = Scene::new().await;
    let report = scene.report().await;
    assert!(
        !subjects(&report).contains(&"replies"),
        "not said unless set"
    );

    let trusted = format!("{WEBHOOK}token = \"tk_s3cr3t\"\ntrusted = true\n");
    scene.shepherd.holds_section(&trusted);
    let report = scene.report().await;
    let found = ok(&report, "replies");
    assert!(found.contains("trusted without a code"), "{found}");
    assert!(found.contains("yes on a merge"), "{found}");
    assert!(report.passed(), "{:#?}", report.render());
    let printed = report.render().join("\n");
    assert!(!printed.contains("s3cr3t"), "{printed}");

    // Discord's webhook is never read for replies, so nothing is trusted there.
    let discord = "[webhook]\nkind = \"discord\"\nurl = \"https://discord.com/api/webhooks/1/abc\"\n\
                   token = \"tk_s3cr3t\"\ntrusted = true\n";
    scene.shepherd.holds_section(discord);
    let report = scene.report().await;
    assert!(!subjects(&report).contains(&"replies"));

    // Without a token the settings are refused, not trusted.
    let no_token = format!("{WEBHOOK}trusted = true\n");
    scene.shepherd.holds_section(&no_token);
    let report = scene.report().await;
    let (what, _) = missing(&report, "kelpie settings");
    assert!(what.contains("`webhook.trusted`"), "{what}");
    assert!(!subjects(&report).contains(&"replies"));
}

#[tokio::test]
async fn kelpie_settings_that_do_not_parse_are_missing_and_never_quote_the_url() {
    let scene = Scene::new().await;
    scene
        .shepherd
        .holds_section(&format!("{WEBHOOK}oops = 1\n"));
    let report = scene.report().await;
    let (what, _) = missing(&report, "kelpie settings");
    assert!(!what.contains("s3cr3t"), "{what}");
    assert!(subjects(&report).contains(&"koji: labels"));
    assert!(!subjects(&report).contains(&"koji: rulings"));
}
