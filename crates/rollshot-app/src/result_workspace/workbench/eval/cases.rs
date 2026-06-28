use super::fixture::{load_expected, load_image, load_meta};
use super::layer2::run_golden_source;
use super::scoring::{score_candidates, Thresholds};

const SELFTEST: &str = "selftest_region";

fn golden_for(intent: &str) -> String {
    let path = super::fixture::fixtures_root()
        .join(intent)
        .join("golden_source.js");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn layer2_selftest_golden_passes_scoring() {
    let image = load_image(SELFTEST);
    let expected = load_expected(SELFTEST);
    let cands = run_golden_source(&image, &golden_for(SELFTEST)).expect("layer2 runs");
    let report = score_candidates(&expected, &cands, &Thresholds::lenient());
    assert!(
        report.passed(),
        "selftest golden failed scoring: {:?}",
        report.gate_failures
    );
}

#[test]
fn layer2_bad_golden_fails_scoring() {
    let image = load_image(SELFTEST);
    let expected = load_expected(SELFTEST);
    let bad = r#"function main(input){return {candidates:[{kind:'addRedaction',bounds:{x:0,y:150,width:10,height:10},confidence:0.5,label:'x'}]};}"#;
    let cands = run_golden_source(&image, bad).expect("layer2 runs");
    let report = score_candidates(&expected, &cands, &Thresholds::lenient());
    assert!(!report.passed(), "bad golden unexpectedly passed");
}

#[test]
#[ignore]
fn regenerate_selftest_fixture() {
    use super::fixture::{fixtures_root, FixtureMeta, RequiredCapability};
    use super::render::render_url_bar;
    let dir = fixtures_root().join(SELFTEST);
    std::fs::create_dir_all(&dir).unwrap();
    let rendered = render_url_bar();
    rendered.image.save(dir.join("image.png")).unwrap();
    std::fs::write(
        dir.join("expected_rects.json"),
        serde_json::to_string_pretty(&rendered.expected).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join("meta.json"),
        serde_json::to_string_pretty(&FixtureMeta {
            intent: "url_bar".into(),
            provider: "anthropic".into(),
            model: "claude-opus-4-8".into(),
            required_capability: RequiredCapability::RegionFeatures,
            seeded: true,
        })
        .unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn layer1_selftest_replay_reaches_ready_and_scores() {
    use super::cassette::load_cassette;
    use super::fixture::load_meta;
    use super::layer1::replay_full_loop;
    use super::scoring::{score_candidates, Thresholds};

    let image = load_image(SELFTEST);
    let meta = load_meta(SELFTEST);
    let cassette = load_cassette(SELFTEST);
    let cands = replay_full_loop(&image, &meta, &cassette)
        .await
        .expect("layer1 replay reaches ReadyForReview");
    let report = score_candidates(&load_expected(SELFTEST), &cands, &Thresholds::lenient());
    assert!(
        report.passed(),
        "layer1 scoring failed: {:?}",
        report.gate_failures
    );
}

#[test]
fn layer2_gate_over_all_present_fixtures() {
    use super::fixture::{intent_specs, RequiredCapability};
    use super::scoring::{score_candidates, Thresholds};

    let ocr_enabled = cfg!(feature = "ocr");
    for spec in intent_specs() {
        let meta = load_meta(spec.name);
        if spec.required_capability == RequiredCapability::Ocr && !ocr_enabled {
            eprintln!("SKIP eval fixture {} (ocr feature disabled)", spec.name);
            continue;
        }
        let golden_path = super::fixture::fixtures_root()
            .join(spec.name)
            .join("golden_source.js");
        if !golden_path.exists() {
            if meta.seeded || std::env::var_os("CI").is_some() {
                panic!(
                    "seeded eval fixture {} is missing golden_source.js",
                    spec.name
                );
            }
            eprintln!("SKIP eval fixture {} (golden not yet seeded)", spec.name);
            continue;
        }
        let image = load_image(spec.name);
        let expected = load_expected(spec.name);
        let golden = std::fs::read_to_string(&golden_path).unwrap();
        let cands = run_golden_source(&image, &golden)
            .unwrap_or_else(|e| panic!("{} layer2: {e}", spec.name));
        let report = score_candidates(&expected, &cands, &Thresholds::lenient());
        assert!(
            report.passed(),
            "{} failed gate: {:?}",
            spec.name,
            report.gate_failures
        );
    }
}
