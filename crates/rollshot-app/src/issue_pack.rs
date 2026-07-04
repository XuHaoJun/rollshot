use chrono::{DateTime, Local};
use image::RgbaImage;
use serde::Serialize;
use std::path::{Path, PathBuf};

pub(crate) const SCHEMA_VERSION: u32 = 1;
pub(crate) const EXPORT_MODE: &str = "local_issue_pack";
pub(crate) const TARGET_ISSUE_PACK_EXPORT: &str = "rollshot::issue_pack::export";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlatformInfo {
    pub os: String,
    pub arch: String,
}

impl PlatformInfo {
    pub(crate) fn current() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EvidenceReviewSummary {
    pub required: bool,
    pub completed: bool,
    pub result_workspace_images_reviewed: bool,
    pub action_guide_keyframes_reviewed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RedactionSummary {
    pub review_required: bool,
    pub review_completed: bool,
    pub result_workspace_images_are_flattened: bool,
    pub original_pixels_included: bool,
    pub redaction_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SafeImageAsset {
    pub file_name: String,
    pub pixels: RgbaImage,
    pub derived_from_original: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OcrSnippet {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IssuePackStep {
    pub index: usize,
    pub title: String,
    pub keyframe_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActionGuideIssueAssets {
    pub steps: Vec<IssuePackStep>,
    pub include_gif: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IssuePackInput {
    pub title: Option<String>,
    pub created_at: DateTime<Local>,
    pub rollshot_version: String,
    pub platform: PlatformInfo,
    pub final_image: Option<SafeImageAsset>,
    pub action_guide: Option<ActionGuideIssueAssets>,
    pub ocr_snippets: Vec<OcrSnippet>,
    pub evidence_review: EvidenceReviewSummary,
    pub redaction: RedactionSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AssetEntry {
    pub kind: String,
    pub path: String,
}

pub(crate) fn issue_pack_folder_name(created_at: DateTime<Local>) -> String {
    format!("rollshot-issue-pack-{}", created_at.format("%Y-%m-%d-%H%M"))
}

pub(crate) fn render_issue_markdown(input: &IssuePackInput) -> String {
    let mut md = String::from("# Bug Report\n\n");
    md.push_str("## Summary\n\n[Write a short summary]\n\n");
    md.push_str("## Steps to reproduce\n\n");
    if let Some(action) = &input.action_guide {
        for step in &action.steps {
            md.push_str(&format!(
                "{}. {}\n\n   ![]({})\n\n",
                step.index, step.title, step.keyframe_path
            ));
        }
    } else {
        md.push_str("[Write the steps to reproduce]\n\n");
    }
    md.push_str("## Actual result\n\n");
    if let Some(image) = &input.final_image {
        md.push_str("The UI reached this state:\n\n");
        md.push_str(&format!("![](images/{})\n\n", image.file_name));
    } else {
        md.push_str("[Describe what happened]\n\n");
    }
    md.push_str("## Expected result\n\n[Write what should have happened]\n\n");
    if !input.ocr_snippets.is_empty() {
        md.push_str("## OCR snippets\n\n");
        for snippet in &input.ocr_snippets {
            md.push_str(&format!("- {}\n", snippet.text));
        }
        md.push('\n');
    }
    md.push_str("## Environment\n\n");
    md.push_str(&format!("- OS: {}\n", input.platform.os));
    md.push_str(&format!("- Architecture: {}\n", input.platform.arch));
    md.push_str(&format!(
        "- Rollshot version: {}\n\n",
        input.rollshot_version
    ));
    md.push_str("## Attachments\n\n");
    if input.action_guide.is_some() {
        md.push_str("- `action-guide/steps.md`\n");
        md.push_str("- `action-guide/session.json`\n");
    }
    md.push_str("- `manifest.json`\n");
    md
}

pub(crate) fn manifest_assets(input: &IssuePackInput, include_gif: bool) -> Vec<AssetEntry> {
    let mut assets = vec![
        AssetEntry {
            kind: "issue_markdown".to_string(),
            path: "issue.md".to_string(),
        },
        AssetEntry {
            kind: "manifest".to_string(),
            path: "manifest.json".to_string(),
        },
    ];
    if let Some(image) = &input.final_image {
        assets.push(AssetEntry {
            kind: "final_redacted_image".to_string(),
            path: format!("images/{}", image.file_name),
        });
    }
    if let Some(action) = &input.action_guide {
        assets.push(AssetEntry {
            kind: "action_steps".to_string(),
            path: "action-guide/steps.md".to_string(),
        });
        assets.push(AssetEntry {
            kind: "action_session".to_string(),
            path: "action-guide/session.json".to_string(),
        });
        for step in &action.steps {
            assets.push(AssetEntry {
                kind: "action_keyframe".to_string(),
                path: step.keyframe_path.clone(),
            });
        }
        if include_gif {
            assets.push(AssetEntry {
                kind: "action_gif".to_string(),
                path: "action-guide/guide.gif".to_string(),
            });
        }
    }
    assets
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use image::{Rgba, RgbaImage};

    pub(super) fn base_input() -> IssuePackInput {
        IssuePackInput {
            title: None,
            created_at: Local.with_ymd_and_hms(2026, 7, 4, 15, 30, 0).unwrap(),
            rollshot_version: "0.1.0".to_string(),
            platform: PlatformInfo {
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
            },
            final_image: Some(SafeImageAsset {
                file_name: "final-redacted.png".to_string(),
                pixels: RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255])),
                derived_from_original: true,
            }),
            action_guide: None,
            ocr_snippets: vec![],
            evidence_review: EvidenceReviewSummary {
                required: true,
                completed: true,
                result_workspace_images_reviewed: true,
                action_guide_keyframes_reviewed: false,
            },
            redaction: RedactionSummary {
                review_required: true,
                review_completed: true,
                result_workspace_images_are_flattened: true,
                original_pixels_included: false,
                redaction_count: 0,
            },
        }
    }

    #[test]
    fn folder_name_is_deterministic() {
        assert_eq!(
            issue_pack_folder_name(base_input().created_at),
            "rollshot-issue-pack-2026-07-04-1530"
        );
    }

    #[test]
    fn renders_screenshot_only_markdown_with_relative_link() {
        let md = render_issue_markdown(&base_input());
        assert!(md.contains("![](images/final-redacted.png)"), "md = {md}");
        assert!(
            !md.contains("/tmp/"),
            "md must not contain absolute paths: {md}"
        );
        assert!(md.contains("- `manifest.json`"), "md = {md}");
    }

    #[test]
    fn renders_action_guide_steps_and_omits_missing_ocr() {
        let mut input = base_input();
        input.final_image = None;
        input.action_guide = Some(ActionGuideIssueAssets {
            include_gif: false,
            steps: vec![
                IssuePackStep {
                    index: 1,
                    title: "Open Settings".to_string(),
                    keyframe_path: "action-guide/keyframes/001.png".to_string(),
                },
                IssuePackStep {
                    index: 2,
                    title: "Click Save".to_string(),
                    keyframe_path: "action-guide/keyframes/002.png".to_string(),
                },
            ],
        });
        let md = render_issue_markdown(&input);
        assert!(md.contains("1. Open Settings"), "md = {md}");
        assert!(
            md.contains("![](action-guide/keyframes/001.png)"),
            "md = {md}"
        );
        assert!(!md.contains("## OCR snippets"), "md = {md}");
    }

    #[test]
    fn renders_ocr_snippets_when_available() {
        let mut input = base_input();
        input.ocr_snippets = vec![OcrSnippet {
            text: "Failed to save settings".to_string(),
        }];
        let md = render_issue_markdown(&input);
        assert!(md.contains("## OCR snippets"), "md = {md}");
        assert!(md.contains("- Failed to save settings"), "md = {md}");
    }

    #[test]
    fn manifest_assets_list_every_expected_relative_path() {
        let mut input = base_input();
        input.action_guide = Some(ActionGuideIssueAssets {
            include_gif: true,
            steps: vec![IssuePackStep {
                index: 1,
                title: "Open Settings".to_string(),
                keyframe_path: "action-guide/keyframes/001.png".to_string(),
            }],
        });
        let assets = manifest_assets(&input, true);
        let paths: Vec<_> = assets.iter().map(|a| a.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "issue.md",
                "manifest.json",
                "images/final-redacted.png",
                "action-guide/steps.md",
                "action-guide/session.json",
                "action-guide/keyframes/001.png",
                "action-guide/guide.gif",
            ]
        );
    }
}
