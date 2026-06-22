//! `RealAutomationHost` — the runtime detection host.
//!
//! Template matching is prepared outside QuickJS; callbacks only perform
//! cached lookup and truncation.

use std::time::Instant;

use rollshot_automation::{
    AutomationHost, CapabilityError, LayoutQuery, LayoutRegion, OcrMatch, OcrQuery, RegionFeatures,
    RegionFeaturesQuery, TemplateMatch, TemplateMatchQuery,
};

use crate::index::VisualIndex;
use crate::template::{prepare_template_match as prepare_template_results, TemplateStore};

#[derive(Debug, Clone)]
struct PreparedTemplateMatch {
    template_handle: String,
    region: rollshot_automation::Region,
    max_limit: u32,
    results: Vec<TemplateMatch>,
}

#[derive(Debug, Default)]
pub struct RealAutomationHost {
    prepared_template_matches: Vec<PreparedTemplateMatch>,
}

impl RealAutomationHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Expensive preparation. Call before entering `QuickJsExecutor`.
    pub fn prepare_template_match(
        &mut self,
        index: &VisualIndex,
        templates: &TemplateStore,
        query: &TemplateMatchQuery,
    ) -> Result<(), CapabilityError> {
        let started = Instant::now();
        let results = prepare_template_results(index, templates, query)?;
        self.prepared_template_matches.retain(|prepared| {
            prepared.template_handle != query.template_handle || prepared.region != query.region
        });
        self.prepared_template_matches.push(PreparedTemplateMatch {
            template_handle: query.template_handle.clone(),
            region: query.region,
            max_limit: query.limit,
            results,
        });
        tracing::debug!(
            target: "rollshot::vision::template",
            duration_ms = started.elapsed().as_millis() as u64,
            result_count = self
                .prepared_template_matches
                .last()
                .map_or(0, |prepared| prepared.results.len()),
            "template query prepared"
        );
        Ok(())
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
        query: TemplateMatchQuery,
    ) -> Result<Vec<TemplateMatch>, CapabilityError> {
        if query.limit == 0 {
            return Err(CapabilityError::InvalidInput { code: "invalid_query" });
        }
        let prepared = self
            .prepared_template_matches
            .iter()
            .find(|prepared| {
                prepared.template_handle == query.template_handle && prepared.region == query.region
            })
            .ok_or(CapabilityError::Failed { code: "vision_index_unavailable" })?;
        if query.limit > prepared.max_limit {
            return Err(CapabilityError::LimitExceeded);
        }
        Ok(prepared
            .results
            .iter()
            .take(query.limit as usize)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::VisualIndex;
    use crate::template::{
        TemplateAsset, TemplateBytes, TemplateSensitivity, TemplateSource, TemplateStore,
    };
    use rollshot_automation::Region;

    #[test]
    fn unprepared_template_query_fails_explicitly() {
        let mut host = RealAutomationHost::new();
        let err = host
            .template_match(TemplateMatchQuery {
                template_handle: "missing-preparation".into(),
                region: Region::Full,
                limit: 1,
            })
            .unwrap_err();
        assert_eq!(err, CapabilityError::Failed { code: "vision_index_unavailable" });
    }

    #[test]
    fn prepared_callback_only_looks_up_and_truncates() {
        let mut scene = image::RgbaImage::from_pixel(16, 16, image::Rgba([120, 120, 120, 255]));
        for y in 0..4 {
            for x in 0..4 {
                let v = (x * 47 + y * 29) as u8;
                scene.put_pixel(6 + x, 7 + y, image::Rgba([v, v, v, 255]));
            }
        }
        let tpl = image::imageops::crop_imm(&scene, 6, 7, 4, 4).to_image();
        let index = VisualIndex::build(scene).unwrap();
        let mut store = TemplateStore::new();
        store
            .insert(TemplateAsset {
                handle: "mark".into(),
                sensitivity: TemplateSensitivity::Chrome,
                source: TemplateSource::UserRect,
                created_at_ms: 0,
                bounds_in_source_image: None,
                bytes: TemplateBytes::new(4, 4, tpl.into_raw()).unwrap(),
            })
            .unwrap();
        let prepared_query = TemplateMatchQuery {
            template_handle: "mark".into(),
            region: Region::Full,
            limit: 4,
        };
        let mut host = RealAutomationHost::new();
        host.prepare_template_match(&index, &store, &prepared_query).unwrap();

        let results = host
            .template_match(TemplateMatchQuery {
                limit: 1,
                ..prepared_query.clone()
            })
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(
            host.template_match(TemplateMatchQuery {
                limit: 5,
                ..prepared_query
            })
            .unwrap_err(),
            CapabilityError::LimitExceeded
        );
    }
}
