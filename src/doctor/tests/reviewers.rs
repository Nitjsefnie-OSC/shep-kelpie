use super::*;

#[tokio::test]
async fn a_listed_shipped_reviewer_whose_gateway_kelpie_lacks_is_missing() {
    let scene = Scene::new().await;
    scene.runs("golbat", |t| {
        t["agents"]["reviewers"] = json!(["iq3-xxs-review", "defect-hunter-lite"]);
    });
    let report = scene.report().await;

    let (what, fix) = missing(&report, "golbat: reviewers");
    assert!(
        what.contains("iq3-xxs-review") && what.contains("paddock"),
        "{what}"
    );
    assert!(fix.contains("`agents.reviewers`"), "{fix}");
    assert!(!report.passed());
    // The sheep that lists none runs the default, which leaves out what kelpie cannot route.
    ok(&report, "koji: reviewers");
}
