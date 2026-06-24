//! Rollshot-specific, deterministic, UI-oriented vision adapter layer.
//! Implements the `rollshot_automation::AutomationHost` capability boundary.

#![forbid(unsafe_code)]

mod error;
mod host;
mod index;
pub mod rect;
mod region_features;
mod self_validation;
mod template;

pub use error::VisionError;
pub use host::RealAutomationHost;
pub use index::VisualIndex;
pub use self_validation::{
    self_validate, ExpectedCount, SelfValidationConfig, TemplateDecision, TemplateSelfValidation,
};
pub use template::{
    ExportTemplateAssetRecord, LocalTemplateAssetRecord, TemplateAsset, TemplateBytes,
    TemplateBytesRecord, TemplateSensitivity, TemplateSource, TemplateStore, MAX_SCORE_POSITIONS,
    MAX_TEMPLATE_AREA, MAX_TEMPLATE_COUNT, MAX_TEMPLATE_MATCH_PIXEL_VISITS,
    MAX_TEMPLATE_STORE_BYTES,
};

#[cfg(test)]
mod contract_tests {
    use rollshot_automation::{AutomationHost, LayoutQuery, OcrQuery, Region, TemplateMatchQuery};

    use crate::RealAutomationHost;

    #[test]
    fn all_unimplemented_capabilities_report_unavailable() {
        let mut host = RealAutomationHost::new();
        let expected = rollshot_automation::CapabilityError::Failed {
            code: "capability_unavailable",
        };
        #[cfg(not(feature = "ocr"))]
        assert_eq!(
            host.ocr(OcrQuery {
                region: Region::Full,
                limit: 1
            })
            .unwrap_err(),
            expected
        );
        #[cfg(feature = "ocr")]
        assert_eq!(
            host.ocr(OcrQuery {
                region: Region::Full,
                limit: 1
            })
            .unwrap_err(),
            rollshot_automation::CapabilityError::Failed { code: "vision_index_unavailable" }
        );
        assert_eq!(
            host.layout(LayoutQuery {
                region: Region::Full,
                limit: 1
            })
            .unwrap_err(),
            expected
        );
        // template_match returns vision_index_unavailable when no
        // preparation has been done (the prepared callback contract).
        assert_eq!(
            host.template_match(TemplateMatchQuery {
                template_handle: "x".into(),
                region: Region::Full,
                limit: 1,
            })
            .unwrap_err(),
            rollshot_automation::CapabilityError::Failed {
                code: "vision_index_unavailable",
            }
        );
    }
}
