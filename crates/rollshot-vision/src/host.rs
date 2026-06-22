//! `RealAutomationHost` — the runtime detection host.
//!
//! SP1 implements `template_match` (wired in PR4). Capabilities not yet
//! implemented return an explicit `capability_unavailable` error rather than
//! empty results: in a redaction tool, silently returning no results would let
//! a detector conclude "nothing to hide" and miss sensitive regions.

use rollshot_automation::{
    AutomationHost, CapabilityError, LayoutQuery, LayoutRegion, OcrMatch, OcrQuery, RegionFeatures,
    RegionFeaturesQuery, TemplateMatch, TemplateMatchQuery,
};

#[derive(Debug, Default)]
pub struct RealAutomationHost {}

impl RealAutomationHost {
    pub fn new() -> Self {
        Self {}
    }
}

impl AutomationHost for RealAutomationHost {
    fn ocr(&mut self, _query: OcrQuery) -> Result<Vec<OcrMatch>, CapabilityError> {
        Err(CapabilityError::Failed {
            code: "capability_unavailable",
        })
    }

    fn layout(&mut self, _query: LayoutQuery) -> Result<Vec<LayoutRegion>, CapabilityError> {
        Err(CapabilityError::Failed {
            code: "capability_unavailable",
        })
    }

    fn region_features(
        &mut self,
        _query: RegionFeaturesQuery,
    ) -> Result<Vec<RegionFeatures>, CapabilityError> {
        Err(CapabilityError::Failed {
            code: "capability_unavailable",
        })
    }

    fn template_match(
        &mut self,
        _query: TemplateMatchQuery,
    ) -> Result<Vec<TemplateMatch>, CapabilityError> {
        Err(CapabilityError::Failed {
            code: "capability_unavailable",
        })
    }
}
