use crate::{
    LayoutQuery, LayoutRegion, OcrMatch, OcrQuery, RegionFeatures, RegionFeaturesQuery,
    TemplateMatch, TemplateMatchQuery,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CapabilityError {
    #[error("capability input is invalid: {code}")]
    InvalidInput { code: &'static str },
    #[error("capability limit exceeded")]
    LimitExceeded,
    #[error("capability failed: {code}")]
    Failed { code: &'static str },
}

pub trait AutomationHost: Send {
    fn ocr(&mut self, query: OcrQuery) -> Result<Vec<OcrMatch>, CapabilityError>;
    fn layout(&mut self, query: LayoutQuery) -> Result<Vec<LayoutRegion>, CapabilityError>;
    fn region_features(
        &mut self,
        query: RegionFeaturesQuery,
    ) -> Result<Vec<RegionFeatures>, CapabilityError>;
    fn template_match(
        &mut self,
        query: TemplateMatchQuery,
    ) -> Result<Vec<TemplateMatch>, CapabilityError>;
}

#[derive(Debug, Default)]
pub struct FakeAutomationHost {
    pub ocr_results: Vec<OcrMatch>,
    pub layout_results: Vec<LayoutRegion>,
    pub region_feature_results: Vec<RegionFeatures>,
    pub template_results: Vec<TemplateMatch>,
    pub failure: Option<CapabilityError>,
}

impl FakeAutomationHost {
    fn take_bounded<T: Clone>(&self, values: &[T], limit: u32) -> Result<Vec<T>, CapabilityError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        Ok(values.iter().take(limit as usize).cloned().collect())
    }
}

impl AutomationHost for FakeAutomationHost {
    fn ocr(&mut self, query: OcrQuery) -> Result<Vec<OcrMatch>, CapabilityError> {
        self.take_bounded(&self.ocr_results, query.limit)
    }

    fn layout(&mut self, query: LayoutQuery) -> Result<Vec<LayoutRegion>, CapabilityError> {
        self.take_bounded(&self.layout_results, query.limit)
    }

    fn region_features(
        &mut self,
        query: RegionFeaturesQuery,
    ) -> Result<Vec<RegionFeatures>, CapabilityError> {
        self.take_bounded(&self.region_feature_results, query.limit)
    }

    fn template_match(
        &mut self,
        query: TemplateMatchQuery,
    ) -> Result<Vec<TemplateMatch>, CapabilityError> {
        self.take_bounded(&self.template_results, query.limit)
    }
}
