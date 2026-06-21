use rollshot_automation::{
    CapabilityApiVersion, CapabilityName, IrNodeKind, IrSchemaVersion, LanguageSchemaVersion,
    OutputSchemaVersion, CAPABILITY_API_V1, IR_SCHEMA_V1, LANGUAGE_SCHEMA_V1, OUTPUT_SCHEMA_V1,
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

use rollshot_automation::{validate_source, DiagnosticCode, ValidationLimits};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn assert_has_code(source: &str, expected: DiagnosticCode) {
    let diagnostics = validate_source(source, &ValidationLimits::default()).unwrap_err();
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == expected),
        "missing {expected:?}: {diagnostics:#?}"
    );
}

#[test]
fn accepts_explicit_main_and_pure_helper_shape() {
    let validated =
        validate_source(&fixture("valid_main.js"), &ValidationLimits::default()).unwrap();
    assert_eq!(validated.source, fixture("valid_main.js"));
}

#[test]
fn rejects_missing_duplicate_and_malformed_main() {
    assert_has_code(&fixture("missing_main.js"), DiagnosticCode::MissingMain);
    assert_has_code(&fixture("duplicate_main.js"), DiagnosticCode::DuplicateMain);
    assert_has_code(
        &fixture("invalid_main_signature.js"),
        DiagnosticCode::InvalidMainSignature,
    );
}

#[test]
fn rejects_non_function_top_level_statement() {
    assert_has_code(
        &fixture("invalid_top_level.js"),
        DiagnosticCode::InvalidTopLevel,
    );
}

#[test]
fn rejects_mutation_loops_dynamic_access_and_ambient_globals() {
    assert_has_code(
        &fixture("reject_mutation.js"),
        DiagnosticCode::UnsupportedSyntax,
    );
    assert_has_code(
        &fixture("reject_loop.js"),
        DiagnosticCode::UnsupportedSyntax,
    );
    assert_has_code(
        &fixture("reject_dynamic_access.js"),
        DiagnosticCode::UnsupportedSyntax,
    );
    assert_has_code(
        &fixture("reject_ambient.js"),
        DiagnosticCode::UnknownIdentifier,
    );
}

#[test]
fn rejects_impure_recursive_and_escaping_helpers() {
    assert_has_code(
        &fixture("reject_helper_capability.js"),
        DiagnosticCode::HelperImpurity,
    );
    assert_has_code(
        &fixture("reject_indirect_recursion.js"),
        DiagnosticCode::RecursiveHelper,
    );
    assert_has_code(
        &fixture("reject_escaping_closure.js"),
        DiagnosticCode::EscapingClosure,
    );
}

#[test]
fn rejects_duplicate_object_keys_before_runtime() {
    assert_has_code(
        &fixture("reject_duplicate_key.js"),
        DiagnosticCode::DuplicateObjectKey,
    );
}

#[test]
fn language_schema_v1_denylist_is_complete() {
    let cases = [
        ("function main(input){ return eval('1'); }", DiagnosticCode::UnknownIdentifier),
        ("function main(input){ return Function('return 1')(); }", DiagnosticCode::UnknownIdentifier),
        ("function main(input){ return Reflect.get(input, 'region'); }", DiagnosticCode::UnknownIdentifier),
        ("function main(input){ return new Proxy({}, {}); }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ return input?.region; }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ return input['region']; }", DiagnosticCode::UnsupportedSyntax),
        ("import value from 'x'; function main(input){ return value; }", DiagnosticCode::InvalidTopLevel),
        ("export function main(input){ return { candidates: [] }; }", DiagnosticCode::InvalidTopLevel),
        ("function main(input){ return import('x'); }", DiagnosticCode::UnsupportedSyntax),
        ("async function main(input){ return { candidates: [] }; }", DiagnosticCode::InvalidMainSignature),
        ("function main(input){ return Promise.resolve([]); }", DiagnosticCode::UnknownIdentifier),
        ("function main(input){ setTimeout(() => {}, 1); return { candidates: [] }; }", DiagnosticCode::UnknownIdentifier),
        ("function* main(input){ return { candidates: [] }; }", DiagnosticCode::InvalidMainSignature),
        ("class X {} function main(input){ return { candidates: [] }; }", DiagnosticCode::InvalidTopLevel),
        ("function main(input){ return new Array(); }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ try { return { candidates: [] }; } catch (error) { return { candidates: [] }; } }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ for (;;) {} return { candidates: [] }; }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ do {} while (true); return { candidates: [] }; }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ for (const value of []) {} return { candidates: [] }; }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ const { region } = input; return { candidates: [] }; }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ return { ...input, candidates: [] }; }", DiagnosticCode::UnsupportedSyntax),
        ("function helper(...values){ return values; } function main(input){ return { candidates: [] }; }", DiagnosticCode::UnsupportedSyntax),
        ("function helper(value = 1){ return value; } function main(input){ return { candidates: [] }; }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ return { candidates: [].reduce((a,b) => a, []) }; }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ return { candidates: [].flatMap((x) => x) }; }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ return { candidates: [].sort() }; }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ return { candidates: helper.call(null, input) }; } function helper(x){ return x; }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ return unknownGlobal; }", DiagnosticCode::UnknownIdentifier),
        ("function main(input){ return Math.random(); }", DiagnosticCode::UnsupportedSyntax),
        ("function main(input){ const arr = []; arr.push(1); return { candidates: arr }; }", DiagnosticCode::UnsupportedSyntax),
    ];
    for (source, code) in cases {
        assert_has_code(source, code);
    }
}

#[test]
fn valid_source_normalizes_to_deterministic_ir_and_costs() {
    let source = fixture("valid_main.js");
    let first = validate_source(&source, &ValidationLimits::default()).unwrap();
    let second = validate_source(&source, &ValidationLimits::default()).unwrap();
    assert_eq!(first.workflow_ir, second.workflow_ir);
    assert!(first
        .workflow_ir
        .nodes
        .iter()
        .any(|node| matches!(node.kind, IrNodeKind::CapabilityCall(_))));
    assert!(first
        .workflow_ir
        .capability_manifest
        .calls
        .iter()
        .any(|call| call.capability == CapabilityName::Ocr));
    assert_eq!(first.workflow_ir.static_cost.max_output_candidates, 10);
}

#[test]
fn rejects_collection_without_provable_bound() {
    let source = r#"
function main(input) {
  return {
    candidates: input.unknown.map((value) => value),
  };
}
"#;
    assert_has_code(source, DiagnosticCode::UnboundedCollection);
}
