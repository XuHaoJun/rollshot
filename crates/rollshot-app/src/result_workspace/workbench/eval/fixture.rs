use super::render::{
    render_account_ids, render_bookmarks, render_desktop_folders, render_emails, render_names,
    render_url_bar, RenderedFixture,
};
use super::scoring::ExpectedRect;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RequiredCapability {
    RegionFeatures,
    Ocr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FixtureMeta {
    pub intent: String,
    pub provider: String,
    pub model: String,
    pub required_capability: RequiredCapability,
    /// True only when both `golden_source.js` and `cassette.json` are committed
    /// and expected to gate CI for this fixture.
    pub seeded: bool,
}

pub(crate) struct IntentSpec {
    pub name: &'static str,
    pub required_capability: RequiredCapability,
    pub render: fn() -> RenderedFixture,
}

pub(crate) fn intent_specs() -> Vec<IntentSpec> {
    use RequiredCapability::*;
    vec![
        IntentSpec { name: "url_bar", required_capability: Ocr, render: render_url_bar },
        IntentSpec { name: "bookmarks", required_capability: Ocr, render: render_bookmarks },
        IntentSpec { name: "desktop_folders", required_capability: Ocr, render: render_desktop_folders },
        IntentSpec { name: "emails", required_capability: Ocr, render: render_emails },
        IntentSpec { name: "names", required_capability: Ocr, render: render_names },
        IntentSpec { name: "account_ids", required_capability: Ocr, render: render_account_ids },
    ]
}

pub(crate) fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/eval/fixtures")
}

pub(crate) fn load_expected(intent: &str) -> Vec<ExpectedRect> {
    let path = fixtures_root().join(intent).join("expected_rects.json");
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&data).expect("valid expected_rects.json")
}

pub(crate) fn load_meta(intent: &str) -> FixtureMeta {
    let path = fixtures_root().join(intent).join("meta.json");
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&data).expect("valid meta.json")
}

pub(crate) fn load_image(intent: &str) -> image::RgbaImage {
    let path = fixtures_root().join(intent).join("image.png");
    image::open(&path)
        .unwrap_or_else(|e| panic!("open {}: {e}", path.display()))
        .to_rgba8()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_intents_are_specified() {
        assert_eq!(intent_specs().len(), 6);
    }

    #[test]
    fn rendered_expected_rects_match_committed_json() {
        for spec in intent_specs() {
            let rendered = (spec.render)();
            let committed = load_expected(spec.name);
            assert_eq!(
                rendered.expected, committed,
                "expected_rects.json for {} is stale; re-run the regeneration test",
                spec.name
            );
        }
    }

    /// Regenerates committed fixture images + expected_rects + meta.
    /// Run manually: `cargo test -p rollshot-app eval::fixture::tests::regenerate_fixtures -- --ignored`
    #[test]
    #[ignore]
    fn regenerate_fixtures() {
        for spec in intent_specs() {
            let dir = fixtures_root().join(spec.name);
            std::fs::create_dir_all(&dir).unwrap();
            let rendered = (spec.render)();
            rendered
                .image
                .save(dir.join("image.png"))
                .expect("save image.png");
            std::fs::write(
                dir.join("expected_rects.json"),
                serde_json::to_string_pretty(&rendered.expected).unwrap(),
            )
            .unwrap();
            let meta = FixtureMeta {
                intent: spec.name.to_string(),
                provider: "anthropic".into(),
                model: "claude-opus-4-8".into(),
                required_capability: spec.required_capability,
                seeded: false,
            };
            std::fs::write(
                dir.join("meta.json"),
                serde_json::to_string_pretty(&meta).unwrap(),
            )
            .unwrap();
        }
    }
}
