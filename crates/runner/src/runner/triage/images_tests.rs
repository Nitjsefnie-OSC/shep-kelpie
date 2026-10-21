//! The images of an issue reaching the triager, through the runner's stand-ins

use serde_json::json;

use std::sync::Mutex;

use super::images::{MOST_BYTES, MOST_IMAGES};
use super::pick_tests::{list, rig_with};
use super::research_tests::{WORDS, calls, crash, reply};
use crate::ports::Role;
use crate::runner::{Runner, step};
use crate::test::{Rig, Scripted, ticket};

const ASSET: &str =
    "https://github.com/user-attachments/assets/3f2a9c1e-0b7d-4c55-9a53-1d2e4f6a8b90";
const OLD: &str = "https://user-images.githubusercontent.com/1234/5678-abcd.png";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

fn researched(rig: &Rig) -> crate::ports::AgentCall {
    let mut asked = calls(rig, Role::Triager);
    assert_eq!(asked.len(), 1, "one research call");
    asked.remove(0).call
}

// A rig whose App covers the repo with open issue 30 untouched, whose body is `body`
fn opened(body: &str) -> (Rig, Mutex<Runner>) {
    let rig = rig_with("pick = \"all\"");
    let runner = rig.open().unwrap();
    list(&rig, crash(&rig, body));
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(WORDS)]);
    (rig, runner)
}

#[test]
fn an_issue_with_a_user_attachments_image_gives_the_triager_its_local_path() {
    let (rig, runner) = opened(&format!("It crashes.\n\n![screenshot]({ASSET})"));
    rig.forge.images.serve_png(ASSET, PNG);
    rig.claude.script_for(Role::Triager, [reply(json!({}))]);

    assert_eq!(step(&runner).unwrap(), None);

    let call = researched(&rig);
    let folder = rig.paths().triage_images(ticket(30));
    let path = folder.join("1.png");
    assert!(
        call.prompt.contains(&path.display().to_string()),
        "{}",
        call.prompt
    );
    assert!(call.reach.read.contains(&folder), "the triager may read it");
    assert!(!call.cwd.starts_with(&folder) && !folder.starts_with(&call.cwd));
    assert!(!folder.exists(), "the images go once the call ends");
    assert_eq!(rig.github.comments().len(), 1, "the triage posted");
}

#[test]
fn the_old_attachment_host_is_fetched_too_and_a_link_is_fetched_once() {
    let body = format!("![a]({OLD}) and again <img src=\"{OLD}\"> and {ASSET}.");
    let (rig, runner) = opened(&body);
    rig.forge.images.serve_png(OLD, PNG);
    rig.forge.images.serve_png(ASSET, PNG);
    rig.claude.script_for(Role::Triager, [reply(json!({}))]);

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(rig.forge.images.asked(), [OLD, ASSET]);
    let prompt = researched(&rig).prompt;
    let folder = rig.paths().triage_images(ticket(30));
    for name in ["1.png", "2.png"] {
        assert!(prompt.contains(&folder.join(name).display().to_string()));
    }
}

#[test]
fn a_link_to_any_other_host_is_not_fetched() {
    let body = "![a](https://example.com/user-attachments/assets/aaaa)\n\
                ![b](https://github.com.evil.example/user-attachments/assets/bbbb)\n\
                ![c](https://github.com@evil.example/user-attachments/assets/cccc)\n\
                ![d](https://github.com/someone/repo/raw/main/shot.png)\n\
                ![e](http://github.com/user-attachments/assets/eeee)";
    let (rig, runner) = opened(body);
    rig.claude.script_for(Role::Triager, [reply(json!({}))]);

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(rig.forge.images.asked(), Vec::<String>::new());
    let call = researched(&rig);
    assert!(!call.prompt.contains("images"), "{}", call.prompt);
    assert!(!rig.paths().triage_images(ticket(30)).exists());
    assert_eq!(rig.github.comments().len(), 1, "the triage still ran");
}

#[test]
fn the_authors_comments_are_read_for_images_and_nobody_else_s() {
    let (rig, runner) = opened("It crashes.");
    let mine = "https://github.com/user-attachments/assets/mine";
    let theirs = "https://github.com/user-attachments/assets/theirs";
    rig.forge
        .threads
        .write_now(30, "newcomer", &format!("Here: ![x]({mine})"));
    rig.forge
        .threads
        .write_now(30, "someone-else", &format!("Me too: ![x]({theirs})"));
    rig.forge.images.serve_png(mine, PNG);
    rig.forge.images.serve_png(theirs, PNG);
    rig.claude.script_for(Role::Triager, [reply(json!({}))]);

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(rig.forge.images.asked(), [mine]);
    let folder = rig.paths().triage_images(ticket(30));
    let prompt = researched(&rig).prompt;
    assert!(prompt.contains(&folder.join("1.png").display().to_string()));
}

#[test]
fn an_image_over_the_size_cap_is_skipped_and_the_triage_runs() {
    let big = vec![0_u8; usize::try_from(MOST_BYTES).unwrap() + 1];
    let small = "https://github.com/user-attachments/assets/small";
    let (rig, runner) = opened(&format!("![big]({ASSET}) ![small]({small})"));
    rig.forge.images.serve_png(ASSET, &big);
    rig.forge.images.serve_png(small, PNG);
    rig.claude.script_for(Role::Triager, [reply(json!({}))]);

    assert_eq!(step(&runner).unwrap(), None);

    let folder = rig.paths().triage_images(ticket(30));
    let prompt = researched(&rig).prompt;
    assert!(prompt.contains(&folder.join("1.png").display().to_string()));
    assert!(!prompt.contains(&folder.join("2.png").display().to_string()));
    assert_eq!(rig.github.comments().len(), 1, "the triage still ran");
}

#[test]
fn images_past_the_count_cap_are_not_fetched() {
    let links: Vec<String> = (0..MOST_IMAGES + 3)
        .map(|n| format!("https://github.com/user-attachments/assets/img{n}"))
        .collect();
    let body = links
        .iter()
        .map(|link| format!("![x]({link})"))
        .collect::<Vec<_>>()
        .join("\n");
    let (rig, runner) = opened(&body);
    for link in &links {
        rig.forge.images.serve_png(link, PNG);
    }
    rig.claude.script_for(Role::Triager, [reply(json!({}))]);

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(rig.forge.images.asked(), links[..MOST_IMAGES]);
    let prompt = researched(&rig).prompt;
    let folder = rig.paths().triage_images(ticket(30));
    let last = folder.join(format!("{MOST_IMAGES}.png"));
    assert!(prompt.contains(&last.display().to_string()));
    assert!(
        !prompt.contains(
            &folder
                .join(format!("{}.png", MOST_IMAGES + 1))
                .display()
                .to_string()
        )
    );
    assert_eq!(rig.github.comments().len(), 1, "the triage still ran");
}

#[test]
fn a_download_that_fails_or_is_no_image_leaves_the_triage_running_without_it() {
    let missing = "https://github.com/user-attachments/assets/missing";
    let video = "https://github.com/user-attachments/assets/video";
    let (rig, runner) = opened(&format!("{missing} {video} {ASSET}"));
    rig.forge.images.serve_other(video, "video/mp4");
    rig.forge.images.serve_png(ASSET, PNG);
    rig.claude.script_for(Role::Triager, [reply(json!({}))]);

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(rig.forge.images.asked(), [missing, video, ASSET]);
    let folder = rig.paths().triage_images(ticket(30));
    let prompt = researched(&rig).prompt;
    assert!(prompt.contains(&folder.join("1.png").display().to_string()));
    assert!(!prompt.contains(&folder.join("2.png").display().to_string()));
    assert_eq!(rig.github.comments().len(), 1, "the triage still ran");
}

#[test]
fn a_call_with_no_image_to_show_reads_as_it_did() {
    let (rig, runner) = opened("It crashes when I run it.");
    rig.claude.script_for(Role::Triager, [reply(json!({}))]);

    assert_eq!(step(&runner).unwrap(), None);

    let call = researched(&rig);
    assert!(!call.prompt.contains("downloaded"), "{}", call.prompt);
    assert!(rig.forge.images.asked().is_empty());
    assert!(!(call.reach.read.iter()).any(|p| p.ends_with("30-images")));
}
