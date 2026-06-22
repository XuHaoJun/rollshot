use std::time::Duration;

use rollshot_automation::{
    execute_to_proposal, validate_source, AutomationInput, CancellationFlag, ExecutionPolicy,
    ProposalContext, ProposedEditKind, Region, ValidationLimits,
};
use rollshot_automation_rquickjs::QuickJsExecutor;
use rollshot_edit_proposal::{
    EditProposal, ProposalId, ProposedEdit, Provenance, ProvenanceSource,
};
use rollshot_vision::{
    RealAutomationHost, TemplateAsset, TemplateBytes, TemplateSensitivity, TemplateSource,
    TemplateStore, VisualIndex,
};

const BOOKMARKS_JS: &str = include_str!("fixtures/hide_bookmarks.js");
const FOLDERS_JS: &str = include_str!("fixtures/hide_folders.js");

fn scene_with_size(size: u32, marks: &[(u32, u32)]) -> image::RgbaImage {
    let mut scene = image::RgbaImage::from_fn(size, size, |x, y| {
        let v = 120 + ((x * 3 + y * 5) % 23) as u8;
        image::Rgba([v, v, v, 255])
    });
    for &(ox, oy) in marks {
        for dy in 0..8 {
            for dx in 0..8 {
                let v = ((dx * 31 + dy * 17 + dx * dy * 7) % 220) as u8;
                scene.put_pixel(ox + dx, oy + dy, image::Rgba([v, v, v, 255]));
            }
        }
    }
    scene
}

fn scene_with(marks: &[(u32, u32)]) -> image::RgbaImage {
    scene_with_size(60, marks)
}

fn template_from(scene: &image::RgbaImage, x: u32, y: u32) -> TemplateBytes {
    let crop = image::imageops::crop_imm(scene, x, y, 8, 8).to_image();
    TemplateBytes::new(8, 8, crop.into_raw()).unwrap()
}

fn store_with(handle: &str, bytes: TemplateBytes) -> TemplateStore {
    let mut store = TemplateStore::new();
    store
        .insert(TemplateAsset {
            handle: handle.into(),
            sensitivity: TemplateSensitivity::Chrome,
            source: TemplateSource::UserRect,
            created_at_ms: 0,
            bounds_in_source_image: None,
            bytes,
        })
        .unwrap();
    store
}

fn run(
    js: &str,
    scene: image::RgbaImage,
    store: TemplateStore,
    handle_key: &str,
    handle_value: &str,
    query_limit: u32,
) -> EditProposal {
    let (w, h) = scene.dimensions();
    let automation = validate_source(js, &ValidationLimits::default()).unwrap();
    let mut handles = std::collections::BTreeMap::new();
    handles.insert(handle_key.to_string(), handle_value.to_string());
    let input = AutomationInput {
        image_width: w,
        image_height: h,
        region: Some(Region::Full),
        annotations: Vec::new(),
        capability_handles: handles,
    };
    let proposal = ProposalContext {
        proposal_id: ProposalId(1),
        base_document_state_id: 0,
        provenance: Provenance {
            source: ProvenanceSource::Agent { run_id: 1 },
        },
    };
    let mut policy = ExecutionPolicy::smart_redaction_default(
        Duration::from_secs(2),
        16 * 1024 * 1024,
        256 * 1024,
    );
    policy
        .allowed_edit_kinds
        .insert(ProposedEditKind::AddRedaction);

    let index = VisualIndex::build(scene).unwrap();
    let query = rollshot_automation::TemplateMatchQuery {
        template_handle: handle_value.to_string(),
        region: Region::Full,
        limit: query_limit,
    };
    let mut host = RealAutomationHost::new();
    host.prepare_template_match(&index, &store, &query).unwrap();
    let (proposal, _metrics) = execute_to_proposal(
        &QuickJsExecutor,
        &automation,
        &input,
        &proposal,
        &mut host,
        &policy,
        &CancellationFlag::new(),
    )
    .unwrap();
    proposal
}

#[test]
fn bookmark_strip_produces_one_candidate() {
    let scene = scene_with(&[(6, 4)]); // single "strip-like" mark
    let tpl = template_from(&scene, 6, 4);
    let proposal = run(
        BOOKMARKS_JS,
        scene,
        store_with("bookmarkStrip", tpl),
        "bookmarkStrip",
        "bookmarkStrip",
        40,
    );
    assert_eq!(proposal.candidates.len(), 1);
    match &proposal.candidates[0].edit {
        ProposedEdit::AddRedaction { bounds } => {
            assert!((bounds.x - 6.0).abs() <= 2.0);
            assert!((bounds.y - 4.0).abs() <= 2.0);
        }
        other => panic!("expected AddRedaction, got {other:?}"),
    }
    assert_eq!(proposal.candidates[0].label, "bookmark-strip-template");
}

#[test]
fn folder_grid_produces_candidate_per_icon() {
    let marks = [(6u32, 6u32), (40, 6), (6, 40), (40, 40)];
    let scene = scene_with_size(120, &marks);
    let tpl = template_from(&scene, 6, 6);
    let proposal = run(
        FOLDERS_JS,
        scene,
        store_with("folderIcon", tpl),
        "folderIcon",
        "folderIcon",
        80,
    );
    // One padded candidate per pasted icon.
    assert_eq!(proposal.candidates.len(), marks.len());
}

#[test]
fn blank_scene_produces_no_candidates() {
    // Distinctive template, but the scene has no instance of it.
    let template_scene = scene_with(&[(6, 6)]);
    let tpl = template_from(&template_scene, 6, 6);
    let blank = image::RgbaImage::from_pixel(60, 60, image::Rgba([200, 120, 40, 255]));
    let proposal = run(
        BOOKMARKS_JS,
        blank,
        store_with("bookmarkStrip", tpl),
        "bookmarkStrip",
        "bookmarkStrip",
        40,
    );
    assert_eq!(proposal.candidates.len(), 0);
}

#[test]
fn detection_is_deterministic() {
    let make = || {
        let scene = scene_with(&[(6, 4)]);
        let tpl = template_from(&scene, 6, 4);
        run(
            BOOKMARKS_JS,
            scene,
            store_with("bookmarkStrip", tpl),
            "bookmarkStrip",
            "bookmarkStrip",
            40,
        )
    };
    let a = make();
    let b = make();
    assert_eq!(a.candidates, b.candidates);
}

#[test]
fn translated_instance_is_still_found() {
    let scene = scene_with(&[(41, 37)]);
    let tpl = template_from(&scene, 41, 37);
    let proposal = run(
        BOOKMARKS_JS,
        scene,
        store_with("bookmarkStrip", tpl),
        "bookmarkStrip",
        "bookmarkStrip",
        40,
    );
    assert_eq!(proposal.candidates.len(), 1);
    match &proposal.candidates[0].edit {
        ProposedEdit::AddRedaction { bounds } => {
            assert!((bounds.x - 41.0).abs() <= 2.0);
            assert!((bounds.y - 37.0).abs() <= 2.0);
        }
        other => panic!("expected AddRedaction, got {other:?}"),
    }
}

#[test]
fn scaled_instance_is_a_known_miss() {
    let source = scene_with(&[(6, 6)]);
    let tpl_image = image::imageops::crop_imm(&source, 6, 6, 8, 8).to_image();
    let tpl = TemplateBytes::new(8, 8, tpl_image.clone().into_raw()).unwrap();
    let scaled = image::imageops::resize(&tpl_image, 16, 16, image::imageops::FilterType::Triangle);
    let mut scene = scene_with(&[]);
    image::imageops::replace(&mut scene, &scaled, 20, 20);
    let proposal = run(
        BOOKMARKS_JS,
        scene,
        store_with("bookmarkStrip", tpl),
        "bookmarkStrip",
        "bookmarkStrip",
        40,
    );
    assert!(proposal.candidates.is_empty());
}
