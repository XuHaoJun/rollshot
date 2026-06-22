//! `RealAutomationHost` — the runtime detection host.
//!
//! Template matching is prepared outside QuickJS; callbacks only perform
//! cached lookup and truncation.

use std::time::Instant;

use rollshot_automation::{
    AutomationHost, CapabilityError, LayoutQuery, LayoutRegion, OcrMatch, OcrQuery, RegionFeatures,
    RegionFeaturesQuery, TemplateMatch, TemplateMatchQuery,
};
use rollshot_image_document::ImageRect;

use crate::index::VisualIndex;
use crate::rect::{region_to_pixel_rect, PixelRect};
use crate::region_features::{dominant_rgba, edge_density, MAX_REGION_FEATURES_AREA};
use crate::template::{prepare_template_match as prepare_template_results, TemplateStore};

#[derive(Debug, Clone)]
struct PreparedTemplateMatch {
    template_handle: String,
    region: rollshot_automation::Region,
    max_limit: u32,
    results: Vec<TemplateMatch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct RegionFeaturesKey {
    rect: PixelRect,
}

#[derive(Debug, Clone)]
struct PreparedRegionFeatures {
    key: RegionFeaturesKey,
    max_limit: u32,
    results: Vec<RegionFeatures>,
}

#[derive(Debug, Default)]
pub struct RealAutomationHost {
    prepared_template_matches: Vec<PreparedTemplateMatch>,
    prepared_region_features: Vec<PreparedRegionFeatures>,
    image_dimensions: Option<(u32, u32)>,
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

    /// Expensive preparation. Call before entering `QuickJsExecutor`.
    pub fn prepare_region_features(
        &mut self,
        index: &VisualIndex,
        query: &RegionFeaturesQuery,
    ) -> Result<(), CapabilityError> {
        let started = Instant::now();
        let rect = region_to_pixel_rect(
            &query.region,
            index.width(),
            index.height(),
            MAX_REGION_FEATURES_AREA,
        )?;
        let key = RegionFeaturesKey { rect };
        let features = RegionFeatures {
            bounds: ImageRect {
                x: rect.x as f32,
                y: rect.y as f32,
                width: rect.width as f32,
                height: rect.height as f32,
            },
            dominant_rgba: dominant_rgba(index.image(), rect),
            edge_density: edge_density(index.gray(), rect),
        };
        self.image_dimensions = Some((index.width(), index.height()));
        self.prepared_region_features
            .retain(|prepared| prepared.key != key);
        self.prepared_region_features.push(PreparedRegionFeatures {
            key,
            max_limit: query.limit,
            results: vec![features],
        });
        tracing::debug!(
            target: "rollshot::vision::region_features",
            duration_ms = started.elapsed().as_millis() as u64,
            result_count = 1u64,
            "region features prepared"
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
        query: RegionFeaturesQuery,
    ) -> Result<Vec<RegionFeatures>, CapabilityError> {
        if query.limit == 0 {
            return Err(CapabilityError::InvalidInput {
                code: "invalid_query",
            });
        }
        let (width, height) = self.image_dimensions.ok_or(CapabilityError::Failed {
            code: "vision_index_unavailable",
        })?;
        let rect = region_to_pixel_rect(&query.region, width, height, MAX_REGION_FEATURES_AREA)?;
        let key = RegionFeaturesKey { rect };
        let prepared = self
            .prepared_region_features
            .iter()
            .find(|prepared| prepared.key == key)
            .ok_or(CapabilityError::Failed {
                code: "vision_index_unavailable",
            })?;
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

    fn template_match(
        &mut self,
        query: TemplateMatchQuery,
    ) -> Result<Vec<TemplateMatch>, CapabilityError> {
        if query.limit == 0 {
            return Err(CapabilityError::InvalidInput {
                code: "invalid_query",
            });
        }
        let prepared = self
            .prepared_template_matches
            .iter()
            .find(|prepared| {
                prepared.template_handle == query.template_handle && prepared.region == query.region
            })
            .ok_or(CapabilityError::Failed {
                code: "vision_index_unavailable",
            })?;
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

    fn checkerboard(size: u32) -> image::RgbaImage {
        image::RgbaImage::from_fn(size, size, |x, y| {
            let v = if (x + y) % 2 == 0 { 0 } else { 255 };
            image::Rgba([v, v, v, 255])
        })
    }

    #[test]
    fn unprepared_region_features_query_fails_explicitly() {
        let mut host = RealAutomationHost::new();
        let err = host
            .region_features(RegionFeaturesQuery {
                region: Region::Full,
                limit: 1,
            })
            .unwrap_err();
        assert_eq!(
            err,
            CapabilityError::Failed {
                code: "vision_index_unavailable"
            }
        );
    }

    #[test]
    fn region_features_rejects_zero_limit() {
        let mut host = RealAutomationHost::new();
        let err = host
            .region_features(RegionFeaturesQuery {
                region: Region::Full,
                limit: 0,
            })
            .unwrap_err();
        assert_eq!(
            err,
            CapabilityError::InvalidInput {
                code: "invalid_query"
            }
        );
    }

    #[test]
    fn prepared_region_features_round_trips_and_canonical_key_matches() {
        let index = VisualIndex::build(checkerboard(8)).unwrap();
        let mut host = RealAutomationHost::new();
        host.prepare_region_features(
            &index,
            &RegionFeaturesQuery {
                region: Region::Full,
                limit: 1,
            },
        )
        .unwrap();

        // Full was prepared; an equivalent explicit full rect must hit the same key.
        let equivalent_full = Region::Rect {
            bounds: ImageRect {
                x: 0.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            },
        };
        let out = host
            .region_features(RegionFeaturesQuery {
                region: equivalent_full,
                limit: 1,
            })
            .unwrap();
        assert_eq!(out.len(), 1);
        // Clipped measured bounds, not raw requested bounds.
        assert_eq!(
            out[0].bounds,
            ImageRect {
                x: 0.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            }
        );
        // Checkerboard -> every counted pixel is an edge.
        assert_eq!(out[0].edge_density, 1.0);
    }

    #[test]
    fn region_features_limit_over_prepared_max_is_limit_exceeded() {
        let index = VisualIndex::build(checkerboard(8)).unwrap();
        let mut host = RealAutomationHost::new();
        host.prepare_region_features(
            &index,
            &RegionFeaturesQuery {
                region: Region::Full,
                limit: 1,
            },
        )
        .unwrap();
        let err = host
            .region_features(RegionFeaturesQuery {
                region: Region::Full,
                limit: 2,
            })
            .unwrap_err();
        assert_eq!(err, CapabilityError::LimitExceeded);
    }

    #[test]
    fn region_features_rejects_non_finite_region() {
        let index = VisualIndex::build(checkerboard(8)).unwrap();
        let mut host = RealAutomationHost::new();
        let bad = Region::Rect {
            bounds: ImageRect {
                x: f32::NAN,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            },
        };
        let err = host
            .prepare_region_features(
                &index,
                &RegionFeaturesQuery {
                    region: bad,
                    limit: 1,
                },
            )
            .unwrap_err();
        assert_eq!(
            err,
            CapabilityError::InvalidInput {
                code: "non_finite_region"
            }
        );
    }

    #[test]
    fn region_features_rejects_empty_region() {
        let index = VisualIndex::build(checkerboard(8)).unwrap();
        let mut host = RealAutomationHost::new();
        let bad = Region::Rect {
            bounds: ImageRect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 8.0,
            },
        };
        let err = host
            .prepare_region_features(
                &index,
                &RegionFeaturesQuery {
                    region: bad,
                    limit: 1,
                },
            )
            .unwrap_err();
        assert_eq!(
            err,
            CapabilityError::InvalidInput {
                code: "empty_region"
            }
        );
    }

    #[test]
    fn region_features_returns_clipped_measured_bounds() {
        let index = VisualIndex::build(checkerboard(8)).unwrap();
        let mut host = RealAutomationHost::new();
        host.prepare_region_features(
            &index,
            &RegionFeaturesQuery {
                region: Region::Rect {
                    bounds: ImageRect {
                        x: -5.0,
                        y: -5.0,
                        width: 20.0,
                        height: 20.0,
                    },
                },
                limit: 1,
            },
        )
        .unwrap();
        let out = host
            .region_features(RegionFeaturesQuery {
                region: Region::Rect {
                    bounds: ImageRect {
                        x: -5.0,
                        y: -5.0,
                        width: 20.0,
                        height: 20.0,
                    },
                },
                limit: 1,
            })
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].bounds,
            ImageRect {
                x: 0.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            }
        );
    }

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
        assert_eq!(
            err,
            CapabilityError::Failed {
                code: "vision_index_unavailable"
            }
        );
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
        host.prepare_template_match(&index, &store, &prepared_query)
            .unwrap();

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
