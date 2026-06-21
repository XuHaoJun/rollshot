use rollshot_automation::{
    CapabilityApiVersion, IrSchemaVersion, LanguageSchemaVersion, OutputSchemaVersion,
    CAPABILITY_API_V1, IR_SCHEMA_V1, LANGUAGE_SCHEMA_V1, OUTPUT_SCHEMA_V1,
};

#[test]
fn installed_schema_versions_are_explicit_and_round_trip() {
    assert_eq!(LANGUAGE_SCHEMA_V1, LanguageSchemaVersion(1));
    assert_eq!(IR_SCHEMA_V1, IrSchemaVersion(1));
    assert_eq!(CAPABILITY_API_V1, CapabilityApiVersion(1));
    assert_eq!(OUTPUT_SCHEMA_V1, OutputSchemaVersion(1));

    let json = serde_json::to_string(&(
        LANGUAGE_SCHEMA_V1,
        IR_SCHEMA_V1,
        CAPABILITY_API_V1,
        OUTPUT_SCHEMA_V1,
    ))
    .unwrap();
    let decoded: (
        LanguageSchemaVersion,
        IrSchemaVersion,
        CapabilityApiVersion,
        OutputSchemaVersion,
    ) = serde_json::from_str(&json).unwrap();
    assert_eq!(
        decoded,
        (
            LANGUAGE_SCHEMA_V1,
            IR_SCHEMA_V1,
            CAPABILITY_API_V1,
            OUTPUT_SCHEMA_V1,
        )
    );
}

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use rollshot_automation::{
    AnnotationDescriptor, AutomationHost, AutomationInput, ExecutionPolicy, FakeAutomationHost,
    OcrMatch, OcrQuery, ProposedEditKind, Region,
};
use rollshot_image_document::{AnnotationId, ImagePoint, ImageRect};

#[test]
fn fake_host_enforces_query_limits() {
    let bounds = ImageRect::from_corners(ImagePoint::new(1.0, 2.0), ImagePoint::new(11.0, 12.0));
    let mut host = FakeAutomationHost {
        ocr_results: vec![
            OcrMatch {
                bounds,
                text: "one".into(),
                confidence: 0.9,
            },
            OcrMatch {
                bounds,
                text: "two".into(),
                confidence: 0.8,
            },
        ],
        ..FakeAutomationHost::default()
    };
    let results = host
        .ocr(OcrQuery {
            region: Region::Full,
            limit: 1,
        })
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn smart_redaction_policy_allows_only_add_redaction() {
    let policy = ExecutionPolicy::smart_redaction_default(
        Duration::from_millis(250),
        8 * 1024 * 1024,
        256 * 1024,
    );
    assert_eq!(
        policy.allowed_edit_kinds,
        BTreeSet::from([ProposedEditKind::AddRedaction])
    );
    assert!(policy.allowed_annotation_ids.is_empty());
}

#[test]
fn automation_input_carries_string_safe_annotation_ids() {
    let input = AutomationInput {
        image_width: 100,
        image_height: 80,
        region: Some(Region::Rect {
            bounds: ImageRect::from_corners(ImagePoint::new(0.0, 0.0), ImagePoint::new(50.0, 40.0)),
        }),
        annotations: vec![AnnotationDescriptor {
            id: AnnotationId(u64::MAX),
            kind: "redaction".into(),
            bounds: Some(ImageRect::from_corners(
                ImagePoint::new(1.0, 1.0),
                ImagePoint::new(2.0, 2.0),
            )),
        }],
        capability_handles: BTreeMap::new(),
    };
    assert_eq!(input.annotations[0].id.0.to_string(), u64::MAX.to_string());
    let value = serde_json::to_value(&input).unwrap();
    assert_eq!(
        value["annotations"][0]["id"],
        serde_json::Value::String(u64::MAX.to_string())
    );
    assert_eq!(value["region"]["kind"], "rect");
}
