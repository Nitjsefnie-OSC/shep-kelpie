use super::*;

const PI: &str = "role: implementer\nharness: pi\nmodel: qwen3.8:27b\neffort: low\n\
                  url: http://box:11434/v1\ncontext: 65536\n";
const ENDPOINT: &str = "role: reviewer\nharness: endpoint\nurl: http://box:11434/v1\n\
                        model: coder\ncontext: 32768\n";
const COMMAND: &str = "role: reviewer\nharness: command\ncommand: ~/bin/review\n";

fn file(keys: &str) -> String {
    format!("---\n{keys}---\n")
}

#[test]
fn an_agent_file_that_names_lease_gpu_is_refused_with_the_gateway_to_use() {
    let pi_reviewer = PI.replace("implementer", "reviewer");
    for (what, keys) in [
        ("an implementer", PI),
        ("a pi reviewer", pi_reviewer.as_str()),
        ("an endpoint reviewer", ENDPOINT),
        ("a command reviewer", COMMAND),
    ] {
        let text = if what == "a pi reviewer" {
            format!("---\n{keys}lease: gpu\n---\nLook.\n")
        } else {
            file(&format!("{keys}lease: gpu\n"))
        };
        let refused = parse(&text).unwrap_err();
        assert!(refused.contains("`lease`"), "{what}: {refused}");
        assert!(refused.contains("`gateway`"), "{what}: {refused}");
    }
}

#[test]
fn any_other_lease_name_is_refused_the_same_way() {
    let refused = parse(&file(&format!("{PI}lease: gpu-box\n"))).unwrap_err();
    assert!(refused.contains("`gateway`"), "{refused}");
}

#[test]
fn a_model_server_reached_by_url_is_held_back_by_nothing_of_kelpies() {
    let agent = parse(&file(PI)).unwrap();
    assert_eq!(agent.runs.session().unwrap().1, &Limit::Unqueued);
}
