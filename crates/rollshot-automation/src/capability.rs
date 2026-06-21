use std::collections::BTreeSet;

use rollshot_image_document::{ImagePoint, ImageRect};
use serde::{Deserialize, Serialize};

use crate::{CapabilityApiVersion, SourceSpan, CAPABILITY_API_V1};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CapabilityName {
    Ocr,
    Layout,
    RegionFeatures,
    TemplateMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Region {
    Full,
    Rect { bounds: ImageRect },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrQuery {
    pub region: Region,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrMatch {
    pub bounds: ImageRect,
    pub text: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutQuery {
    pub region: Region,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutRegion {
    pub bounds: ImageRect,
    pub role: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionFeaturesQuery {
    pub region: Region,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionFeatures {
    pub bounds: ImageRect,
    pub dominant_rgba: [u8; 4],
    pub edge_density: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateMatchQuery {
    pub template_handle: String,
    pub region: Region,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateMatch {
    pub bounds: ImageRect,
    pub score: f32,
    pub anchor: ImagePoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityCallManifest {
    pub capability: CapabilityName,
    pub source_span: SourceSpan,
    pub max_calls: u32,
    pub max_results_per_call: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityManifest {
    pub capability_api_version: CapabilityApiVersion,
    pub calls: Vec<CapabilityCallManifest>,
    pub required_input_fields: BTreeSet<String>,
    pub max_aggregate_results: u32,
}

impl Default for CapabilityManifest {
    fn default() -> Self {
        Self {
            capability_api_version: CAPABILITY_API_V1,
            calls: Vec::new(),
            required_input_fields: BTreeSet::new(),
            max_aggregate_results: 0,
        }
    }
}
