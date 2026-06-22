//! Template assets, the local template store, and the privacy serialization
//! gate. `TemplateAsset`/`TemplateStore` deliberately do NOT derive a generic
//! `Serialize` that writes bytes: serialization only goes through the explicit
//! `LocalTemplateAssetRecord` (keeps all bytes) and `ExportTemplateAssetRecord`
//! (drops `Sensitive` bytes). This makes it impossible to leak sensitive bytes
//! through an accidental `serde_json::to_writer(&store)`.

use std::collections::BTreeMap;
use std::path::Path;

use rollshot_image_document::ImageRect;
use serde::{Deserialize, Serialize};

use crate::VisionError;

/// Cap on a single template's pixel area.
pub const MAX_TEMPLATE_AREA: u64 = 1_048_576; // 1024x1024
/// Cap on templates in one preset-local store.
pub const MAX_TEMPLATE_COUNT: usize = 256;
/// Cap on raw RGBA bytes retained by one store.
pub const MAX_TEMPLATE_STORE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateSensitivity {
    Chrome,
    Sensitive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateSource {
    UserRect,
    AgentSuggested,
}

/// Raw RGBA template pixels. Invariant: `rgba.len() == width * height * 4`,
/// `width > 0`, `height > 0`, `width * height <= MAX_TEMPLATE_AREA`. Only
/// constructible through `new`, which checks the invariant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateBytes {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl TemplateBytes {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, VisionError> {
        if width == 0 || height == 0 {
            return Err(VisionError::InvalidTemplateBytes {
                code: "zero_dimension",
            });
        }
        if (width as u64) * (height as u64) > MAX_TEMPLATE_AREA {
            return Err(VisionError::InvalidTemplateBytes { code: "too_large" });
        }
        if rgba.len() != (width as usize) * (height as usize) * 4 {
            return Err(VisionError::InvalidTemplateBytes {
                code: "length_mismatch",
            });
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn byte_len(&self) -> usize {
        self.rgba.len()
    }

    /// Infallible: the checked invariant guarantees a valid buffer.
    pub fn to_rgba_image(&self) -> image::RgbaImage {
        image::RgbaImage::from_raw(self.width, self.height, self.rgba.clone())
            .expect("TemplateBytes invariant guarantees a valid RGBA buffer")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TemplateAsset {
    pub handle: String,
    pub sensitivity: TemplateSensitivity,
    pub source: TemplateSource,
    pub created_at_ms: u64,
    pub bounds_in_source_image: Option<ImageRect>,
    pub bytes: TemplateBytes,
}

#[derive(Debug)]
pub struct TemplateStore {
    assets: BTreeMap<String, TemplateAsset>,
    pub(crate) total_bytes: usize,
    max_count: usize,
    max_bytes: usize,
}

impl Default for TemplateStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateStore {
    pub fn new() -> Self {
        Self {
            assets: BTreeMap::new(),
            total_bytes: 0,
            max_count: MAX_TEMPLATE_COUNT,
            max_bytes: MAX_TEMPLATE_STORE_BYTES,
        }
    }

    #[cfg(test)]
    fn with_limits(max_count: usize, max_bytes: usize) -> Self {
        Self {
            assets: BTreeMap::new(),
            total_bytes: 0,
            max_count,
            max_bytes,
        }
    }

    pub fn insert(&mut self, asset: TemplateAsset) -> Result<(), VisionError> {
        let replaced_len = self
            .assets
            .get(&asset.handle)
            .map(|old| old.bytes.byte_len())
            .unwrap_or(0);
        let is_new = !self.assets.contains_key(&asset.handle);
        if is_new && self.assets.len() >= self.max_count {
            return Err(VisionError::StoreLimit {
                code: "too_many_templates",
            });
        }
        let next_total = self
            .total_bytes
            .checked_sub(replaced_len)
            .and_then(|n| n.checked_add(asset.bytes.byte_len()))
            .ok_or(VisionError::StoreLimit {
                code: "template_bytes_overflow",
            })?;
        if next_total > self.max_bytes {
            return Err(VisionError::StoreLimit {
                code: "store_too_large",
            });
        }
        self.assets.insert(asset.handle.clone(), asset);
        self.total_bytes = next_total;
        Ok(())
    }

    pub fn get(&self, handle: &str) -> Option<&TemplateAsset> {
        self.assets.get(handle)
    }

    /// Local persistence: keeps all bytes (chrome + sensitive).
    pub fn save_local(&self, dst: &Path) -> Result<(), VisionError> {
        let records: Vec<_> = self
            .assets
            .values()
            .map(LocalTemplateAssetRecord::from_asset)
            .collect();
        let bytes =
            serde_json::to_vec(&records).map_err(|_| VisionError::Io { code: "serialize" })?;
        std::fs::write(dst, bytes).map_err(|_| VisionError::Io { code: "write" })
    }

    pub fn load_local(src: &Path) -> Result<Self, VisionError> {
        let bytes = std::fs::read(src).map_err(|_| VisionError::Io { code: "read" })?;
        let records: Vec<LocalTemplateAssetRecord> =
            serde_json::from_slice(&bytes).map_err(|_| VisionError::Io {
                code: "deserialize",
            })?;
        let mut store = Self::new();
        for record in records {
            store.insert(record.into_asset()?)?;
        }
        Ok(store)
    }

    /// Export: strips `Sensitive` bytes before any serialization occurs.
    pub fn export(&self, dst: &Path) -> Result<(), VisionError> {
        let records: Vec<_> = self
            .assets
            .values()
            .map(ExportTemplateAssetRecord::from_asset)
            .collect();
        let bytes =
            serde_json::to_vec(&records).map_err(|_| VisionError::Io { code: "serialize" })?;
        std::fs::write(dst, bytes).map_err(|_| VisionError::Io { code: "write" })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalTemplateAssetRecord {
    pub handle: String,
    pub sensitivity_sensitive: bool,
    pub source_agent_suggested: bool,
    pub created_at_ms: u64,
    pub bounds_in_source_image: Option<ImageRect>,
    pub width: u32,
    pub height: u32,
    pub bytes: TemplateBytesRecord,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportTemplateAssetRecord {
    pub handle: String,
    pub sensitivity_sensitive: bool,
    pub source_agent_suggested: bool,
    pub created_at_ms: u64,
    pub bounds_in_source_image: Option<ImageRect>,
    pub width: u32,
    pub height: u32,
    /// `None` for `Sensitive` assets — bytes are stripped on export.
    pub bytes: Option<TemplateBytesRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateBytesRecord {
    pub rgba: Vec<u8>,
}

impl LocalTemplateAssetRecord {
    fn from_asset(a: &TemplateAsset) -> Self {
        Self {
            handle: a.handle.clone(),
            sensitivity_sensitive: matches!(a.sensitivity, TemplateSensitivity::Sensitive),
            source_agent_suggested: matches!(a.source, TemplateSource::AgentSuggested),
            created_at_ms: a.created_at_ms,
            bounds_in_source_image: a.bounds_in_source_image,
            width: a.bytes.width(),
            height: a.bytes.height(),
            bytes: TemplateBytesRecord {
                rgba: a.bytes.rgba.clone(),
            },
        }
    }

    fn into_asset(self) -> Result<TemplateAsset, VisionError> {
        Ok(TemplateAsset {
            handle: self.handle,
            sensitivity: if self.sensitivity_sensitive {
                TemplateSensitivity::Sensitive
            } else {
                TemplateSensitivity::Chrome
            },
            source: if self.source_agent_suggested {
                TemplateSource::AgentSuggested
            } else {
                TemplateSource::UserRect
            },
            created_at_ms: self.created_at_ms,
            bounds_in_source_image: self.bounds_in_source_image,
            bytes: TemplateBytes::new(self.width, self.height, self.bytes.rgba)?,
        })
    }
}

impl ExportTemplateAssetRecord {
    fn from_asset(a: &TemplateAsset) -> Self {
        let bytes = match a.sensitivity {
            TemplateSensitivity::Sensitive => None,
            TemplateSensitivity::Chrome => Some(TemplateBytesRecord {
                rgba: a.bytes.rgba.clone(),
            }),
        };
        Self {
            handle: a.handle.clone(),
            sensitivity_sensitive: matches!(a.sensitivity, TemplateSensitivity::Sensitive),
            source_agent_suggested: matches!(a.source, TemplateSource::AgentSuggested),
            created_at_ms: a.created_at_ms,
            bounds_in_source_image: a.bounds_in_source_image,
            width: a.bytes.width(),
            height: a.bytes.height(),
            bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VisionError;

    fn bytes(w: u32, h: u32) -> TemplateBytes {
        TemplateBytes::new(w, h, vec![0u8; (w * h * 4) as usize]).unwrap()
    }

    fn asset(handle: &str, s: TemplateSensitivity) -> TemplateAsset {
        TemplateAsset {
            handle: handle.into(),
            sensitivity: s,
            source: TemplateSource::UserRect,
            created_at_ms: 0,
            bounds_in_source_image: None,
            bytes: bytes(4, 4),
        }
    }

    fn temp_file(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "rollshot-vision-{}-{name}.json",
            std::process::id()
        ))
    }

    #[test]
    fn template_bytes_rejects_wrong_length() {
        let e = TemplateBytes::new(2, 2, vec![0u8; 8]).unwrap_err();
        assert_eq!(
            e,
            VisionError::InvalidTemplateBytes {
                code: "length_mismatch"
            }
        );
    }

    #[test]
    fn template_bytes_rejects_zero_dim() {
        let e = TemplateBytes::new(0, 2, vec![]).unwrap_err();
        assert_eq!(
            e,
            VisionError::InvalidTemplateBytes {
                code: "zero_dimension"
            }
        );
    }

    #[test]
    fn template_bytes_rejects_oversized() {
        // 1 px over the cap.
        let side = (MAX_TEMPLATE_AREA as f64).sqrt() as u32 + 2;
        let e = TemplateBytes::new(side, side, vec![0u8; (side as usize) * (side as usize) * 4]);
        assert_eq!(
            e.unwrap_err(),
            VisionError::InvalidTemplateBytes { code: "too_large" }
        );
    }

    #[test]
    fn get_returns_inserted_asset() {
        let mut store = TemplateStore::new();
        store
            .insert(asset("a", TemplateSensitivity::Chrome))
            .unwrap();
        assert!(store.get("a").is_some());
        assert!(store.get("missing").is_none());
    }

    #[test]
    fn local_round_trip_keeps_all_bytes_and_export_strips_sensitive() {
        let local_path = temp_file("local-round-trip");
        let export_path = temp_file("export-strip");
        let mut store = TemplateStore::new();
        store
            .insert(asset("chrome", TemplateSensitivity::Chrome))
            .unwrap();
        store
            .insert(asset("secret", TemplateSensitivity::Sensitive))
            .unwrap();

        store.save_local(&local_path).unwrap();
        let loaded = TemplateStore::load_local(&local_path).unwrap();
        assert_eq!(loaded.get("secret").unwrap().bytes.byte_len(), 4 * 4 * 4);

        store.export(&export_path).unwrap();
        let json = std::fs::read(&export_path).unwrap();
        let exported: Vec<ExportTemplateAssetRecord> = serde_json::from_slice(&json).unwrap();
        let secret = exported.iter().find(|r| r.handle == "secret").unwrap();
        let chrome = exported.iter().find(|r| r.handle == "chrome").unwrap();
        assert!(
            secret.bytes.is_none(),
            "sensitive bytes must be stripped on export"
        );
        assert!(chrome.bytes.is_some(), "chrome bytes are kept on export");

        let _ = std::fs::remove_file(local_path);
        let _ = std::fs::remove_file(export_path);
    }

    #[test]
    fn load_rejects_corrupt_records() {
        let path = temp_file("corrupt");
        std::fs::write(&path, br#"[{"handle":"x"}]"#).unwrap();
        assert_eq!(
            TemplateStore::load_local(&path).unwrap_err(),
            VisionError::Io {
                code: "deserialize"
            }
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn store_rejects_too_many_templates() {
        let mut store = TemplateStore::with_limits(2, 1024);
        for i in 0..2 {
            store
                .insert(asset(&format!("template-{i}"), TemplateSensitivity::Chrome))
                .unwrap();
        }
        assert_eq!(
            store
                .insert(asset("one-too-many", TemplateSensitivity::Chrome))
                .unwrap_err(),
            VisionError::StoreLimit {
                code: "too_many_templates"
            }
        );
    }

    #[test]
    fn store_byte_limit_accounts_for_replacement() {
        let mut store = TemplateStore::with_limits(4, 64);
        store
            .insert(asset("same", TemplateSensitivity::Chrome))
            .unwrap();
        store
            .insert(asset("same", TemplateSensitivity::Sensitive))
            .unwrap();
        assert_eq!(store.total_bytes, 64);
        assert_eq!(
            store
                .insert(asset("overflow", TemplateSensitivity::Chrome))
                .unwrap_err(),
            VisionError::StoreLimit {
                code: "store_too_large"
            }
        );
    }

    static_assertions::assert_not_impl_any!(TemplateAsset: serde::Serialize);
    static_assertions::assert_not_impl_any!(TemplateStore: serde::Serialize);
    static_assertions::assert_not_impl_any!(TemplateBytes: serde::Serialize);
    static_assertions::assert_impl_all!(LocalTemplateAssetRecord: serde::Serialize);
    static_assertions::assert_impl_all!(ExportTemplateAssetRecord: serde::Serialize);
}
