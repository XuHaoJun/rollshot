//! Rollshot-specific, deterministic, UI-oriented vision adapter layer.
//! Implements the `rollshot_automation::AutomationHost` capability boundary.

#![forbid(unsafe_code)]

mod error;
mod host;

pub use error::VisionError;
pub use host::RealAutomationHost;

#[cfg(test)]
mod contract_tests {
    use rollshot_automation::{
        AutomationHost, LayoutQuery, OcrQuery, Region, RegionFeaturesQuery, TemplateMatchQuery,
    };

    use crate::RealAutomationHost;

    #[test]
    fn all_unimplemented_capabilities_report_unavailable() {
        let mut host = RealAutomationHost::new();
        let expected = rollshot_automation::CapabilityError::Failed {
            code: "capability_unavailable",
        };
        assert_eq!(
            host.ocr(OcrQuery { region: Region::Full, limit: 1 }).unwrap_err(),
            expected
        );
        assert_eq!(
            host.layout(LayoutQuery { region: Region::Full, limit: 1 }).unwrap_err(),
            expected
        );
        assert_eq!(
            host.region_features(RegionFeaturesQuery { region: Region::Full, limit: 1 })
                .unwrap_err(),
            expected
        );
        assert_eq!(
            host.template_match(TemplateMatchQuery {
                template_handle: "x".into(),
                region: Region::Full,
                limit: 1,
            })
            .unwrap_err(),
            expected
        );
    }
}
