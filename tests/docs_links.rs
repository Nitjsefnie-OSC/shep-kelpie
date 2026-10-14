//! `scripts/docs-links.py` against small doc trees: what it accepts, and for
//! each way a link can be wrong, that it fails and names the page and the link.

use std::fs;
use std::path::Path;
use std::process::Command;

// Runs the script over a tree of `(path, text)` files, with no `git` repo
// around it, and returns whether it passed and what it printed.
fn check(files: &[(&str, &str)]) -> (bool, String) {
    let root = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let path = root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/docs-links.py");
    let output = Command::new("python3")
        .arg("-I")
        .arg(script)
        .arg(root.path())
        .output()
        .unwrap();
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), text)
}

const GUIDE_INDEX: &str = "# Guide\n\n- [Page](page.md)\n";

#[test]
fn a_linked_guide_page_with_good_links_passes() {
    let (ok, out) = check(&[
        (
            "README.md",
            "# Dog\n\nSee [the guide](docs/guide/README.md#guide).\n",
        ),
        ("docs/guide/README.md", GUIDE_INDEX),
        (
            "docs/guide/page.md",
            "# Page\n\n[Back](../../README.md#dog) [up](#page)\n",
        ),
    ]);
    assert!(ok, "{out}");
}

#[test]
fn a_relative_link_to_no_file_names_the_page_and_the_link() {
    let (ok, out) = check(&[
        (
            "README.md",
            "# Dog\n\n[guide](docs/guide/README.md) [gone](docs/guide/gone.md)\n",
        ),
        ("docs/guide/README.md", "# Guide\n"),
    ]);
    assert!(!ok);
    assert!(out.contains("README.md:3"), "{out}");
    assert!(out.contains("docs/guide/gone.md"), "{out}");
    assert!(out.contains("no such file"), "{out}");
}

#[test]
fn an_anchor_naming_no_heading_on_another_page_fails() {
    let (ok, out) = check(&[
        (
            "README.md",
            "# Dog\n\n[guide](docs/guide/README.md#nowhere)\n",
        ),
        ("docs/guide/README.md", "# Guide\n"),
    ]);
    assert!(!ok);
    assert!(out.contains("README.md:3"), "{out}");
    assert!(out.contains("docs/guide/README.md#nowhere"), "{out}");
    assert!(out.contains("no heading"), "{out}");
}

#[test]
fn an_anchor_naming_no_heading_on_the_same_page_fails() {
    let (ok, out) = check(&[("README.md", "# Dog\n\n[up](#cat)\n")]);
    assert!(!ok);
    assert!(out.contains("README.md:3"), "{out}");
    assert!(out.contains("#cat"), "{out}");
}

#[test]
fn anchors_follow_githubs_rule_for_a_heading() {
    let (ok, out) = check(&[(
        "README.md",
        "# Dog\n\n\
         [a](#whats-the-score-with-code-and-a-link) [b](#snake_case-key) [c](#a--b)\n\
         [d](#again) [e](#again-1) [f](#again-2) [g](#ünï-code)\n\n\
         ## What's the `score`, with *code* and [a link](https://example.com)?\n\
         ## `snake_case` key\n\
         ## A / B\n\
         ## Again\n\
         ## Again\n\
         ## Again\n\
         ## Ünï code ##\n",
    )]);
    assert!(ok, "{out}");
}

#[test]
fn a_heading_in_a_code_fence_is_not_a_heading() {
    let (ok, out) = check(&[(
        "README.md",
        "# Dog\n\n[x](#hidden)\n\n```sh\n# hidden\n```\n",
    )]);
    assert!(!ok);
    assert!(out.contains("#hidden"), "{out}");
}

#[test]
fn a_link_in_code_is_not_checked() {
    let (ok, out) = check(&[(
        "README.md",
        "# Dog\n\n`[x](gone.md)`\n\n```md\n[y](gone.md)\n```\n",
    )]);
    assert!(ok, "{out}");
}

#[test]
fn external_links_are_not_checked() {
    let (ok, out) = check(&[(
        "README.md",
        "# Dog\n\n[a](https://example.invalid/x#y) [b](mailto:a@b.c) [c](//example.invalid)\n",
    )]);
    assert!(ok, "{out}");
}

#[test]
fn a_guide_page_the_readme_cannot_reach_is_named() {
    let (ok, out) = check(&[
        ("README.md", "# Dog\n\n[guide](docs/guide/README.md)\n"),
        ("docs/guide/README.md", GUIDE_INDEX),
        ("docs/guide/page.md", "# Page\n"),
        ("docs/guide/lost.md", "# Lost\n"),
    ]);
    assert!(!ok);
    assert!(out.contains("docs/guide/lost.md"), "{out}");
    assert!(out.contains("not reachable"), "{out}");
    assert!(!out.contains("docs/guide/page.md"), "{out}");
}

#[test]
fn a_page_reached_only_through_another_guide_page_is_reachable() {
    let (ok, out) = check(&[
        ("README.md", "# Dog\n\n[guide](docs/guide/a.md)\n"),
        ("docs/guide/a.md", "# A\n\n[b](b.md)\n"),
        ("docs/guide/b.md", "# B\n"),
    ]);
    assert!(ok, "{out}");
}

#[test]
fn a_link_out_of_the_readme_and_the_guide_names_the_page_and_the_link() {
    let (ok, out) = check(&[
        (
            "README.md",
            "# Dog\n\n[glossary](GLOSSARY.md) [img](docs/logo.png)\n",
        ),
        ("GLOSSARY.md", "# Glossary\n"),
        ("docs/logo.png", "png"),
    ]);
    assert!(!ok);
    assert!(out.contains("README.md:3"), "{out}");
    assert!(out.contains("GLOSSARY.md"), "{out}");
    assert!(out.contains("docs/logo.png"), "{out}");
    assert!(out.contains("outside README.md and docs/guide/"), "{out}");
}

#[test]
fn a_guide_page_linking_out_of_the_guide_fails() {
    let (ok, out) = check(&[
        ("README.md", "# Dog\n\n[guide](docs/guide/a.md)\n"),
        ("docs/guide/a.md", "# A\n\n[design](../design/README.md)\n"),
        ("docs/design/README.md", "# Design\n"),
    ]);
    assert!(!ok);
    assert!(out.contains("docs/guide/a.md:3"), "{out}");
    assert!(out.contains("../design/README.md"), "{out}");
}

#[test]
fn a_link_that_climbs_out_of_the_repo_fails() {
    let (ok, out) = check(&[("README.md", "# Dog\n\n[x](../../etc/passwd)\n")]);
    assert!(!ok);
    assert!(out.contains("../../etc/passwd"), "{out}");
}

#[test]
fn a_reference_definition_and_an_html_link_are_checked() {
    let (ok, out) = check(&[(
        "README.md",
        "# Dog\n\n[x][ref] <a href=\"gone.md\">y</a>\n\n[ref]: missing.md\n",
    )]);
    assert!(!ok);
    assert!(out.contains("missing.md"), "{out}");
    assert!(out.contains("gone.md"), "{out}");
}

#[test]
fn the_links_of_an_unreachable_guide_page_are_checked_too() {
    let (ok, out) = check(&[
        ("README.md", "# Dog\n"),
        (
            "docs/guide/lost.md",
            "# Lost\n\n[gone](gone.md) [out](../../README.md#nope)\n",
        ),
    ]);
    assert!(!ok);
    assert!(out.contains("docs/guide/lost.md: not reachable"), "{out}");
    assert!(
        out.contains("docs/guide/lost.md:3: [gone.md]: no such file"),
        "{out}"
    );
    assert!(out.contains("[../../README.md#nope]"), "{out}");
}
