//! Durable persistence for Smart Redaction presets and immutable automation
//! revisions (Sub-project 5). File-based JSON under an injected root. No UI,
//! agent, provider, or capture code.

mod domain;
mod error;
mod io;
mod store;

pub use domain::{
    AutomationRevision, Preset, PresetId, PresetSummary, RevisionCapabilityMetadata,
    RevisionCapabilityRequirement, RevisionId, RevisionOrigin, RevisionProvenance, RevisionSummary,
    TemplateHandleMetadata, STORE_SCHEMA_VERSION,
};
pub use error::{EntityKind, Result, StoreError};
pub use store::PresetStore;

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_automation::{validate_source, ValidatedAutomation, ValidationLimits};

    /// A source verified to validate by the rollshot-automation test suite
    /// (`crates/rollshot-automation/tests/fixtures/valid_main.js`).
    pub(crate) const SAMPLE_SOURCE: &str = r#"function expandBounds(rect, padding) {
  return {
    x: rect.x - padding,
    y: rect.y - padding,
    width: rect.width + padding * 2,
    height: rect.height + padding * 2,
  };
}

function main(input) {
  const matches = rollshot.ocr({ region: input.region, limit: 10 });
  return {
    candidates: matches.map((match) => ({
      kind: "addRedaction",
      bounds: expandBounds(match.bounds, 8),
      confidence: match.confidence,
      label: "ocr-match",
    })),
  };
}
"#;

    pub(crate) fn sample_artifact() -> ValidatedAutomation {
        validate_source(SAMPLE_SOURCE, &ValidationLimits::default())
            .expect("sample automation should validate")
    }

    #[test]
    fn revision_serde_round_trip() {
        let revision = AutomationRevision {
            store_schema_version: STORE_SCHEMA_VERSION,
            id: RevisionId("rev-1".into()),
            preset_id: PresetId("preset-1".into()),
            parent_id: None,
            created_at: "2026-06-24T00:00:00Z".into(),
            provenance: RevisionProvenance {
                origin: RevisionOrigin::AgentRun,
                note: Some("first".into()),
                source_run_ref: None,
            },
            artifact: sample_artifact(),
            capabilities: RevisionCapabilityMetadata::default(),
        };

        let json = serde_json::to_vec(&revision).unwrap();
        let decoded: AutomationRevision = serde_json::from_slice(&json).unwrap();
        assert_eq!(decoded, revision);
    }

    #[test]
    fn revision_capabilities_default_for_legacy_json() {
        let revision = AutomationRevision {
            store_schema_version: STORE_SCHEMA_VERSION,
            id: RevisionId("rev-1".into()),
            preset_id: PresetId("preset-1".into()),
            parent_id: None,
            created_at: "2026-06-28T00:00:00Z".into(),
            provenance: RevisionProvenance {
                origin: RevisionOrigin::AgentRun,
                note: None,
                source_run_ref: None,
            },
            artifact: sample_artifact(),
            capabilities: RevisionCapabilityMetadata::default(),
        };
        let mut value = serde_json::to_value(&revision).unwrap();
        value.as_object_mut().unwrap().remove("capabilities");

        let decoded: AutomationRevision = serde_json::from_value(value).unwrap();

        assert_eq!(decoded.capabilities, RevisionCapabilityMetadata::default());
    }
}
