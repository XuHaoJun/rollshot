use std::collections::BTreeMap;

use rollshot_image_document::{AnnotationId, ImageRect};
use serde::{Deserialize, Serialize};

use crate::Region;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationDescriptor {
    #[serde(with = "annotation_id_string")]
    pub id: AnnotationId,
    pub kind: String,
    pub bounds: Option<ImageRect>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationInput {
    pub image_width: u32,
    pub image_height: u32,
    pub region: Option<Region>,
    pub annotations: Vec<AnnotationDescriptor>,
    pub capability_handles: BTreeMap<String, String>,
}

mod annotation_id_string {
    use rollshot_image_document::AnnotationId;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(id: &AnnotationId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&id.0.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<AnnotationId, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value.is_empty()
            || (value.len() > 1 && value.starts_with('0'))
            || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(serde::de::Error::custom("invalid canonical annotation id"));
        }
        value
            .parse::<u64>()
            .map(AnnotationId)
            .map_err(serde::de::Error::custom)
    }
}
