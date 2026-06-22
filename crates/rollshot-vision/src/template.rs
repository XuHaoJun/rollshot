//! Template assets, the local template store, and the privacy serialization
//! gate. `TemplateAsset`/`TemplateStore` deliberately do NOT derive a generic
//! `Serialize` that writes bytes: serialization only goes through the explicit
//! `LocalTemplateAssetRecord` (keeps all bytes) and `ExportTemplateAssetRecord`
//! (drops `Sensitive` bytes). This makes it impossible to leak sensitive bytes
//! through an accidental `serde_json::to_writer(&store)`.

use std::collections::BTreeMap;
use std::path::Path;

use image::Luma;
use imageproc::template_matching::{match_template, MatchTemplateMethod};
use rollshot_automation::{CapabilityError, Region, TemplateMatch, TemplateMatchQuery};
use rollshot_image_document::{ImageRect, ImagePoint};
use serde::{Deserialize, Serialize};

use crate::VisionError;
use crate::index::VisualIndex;
use crate::rect::{iou, region_to_pixel_rect, MAX_SEARCH_AREA};

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

/// Variance floor below which a template carries too little information for NCC.
const MIN_TEMPLATE_VARIANCE: f32 = 25.0;
/// IoU above which two matches are treated as the same instance during NMS.
const NMS_IOU_THRESHOLD: f32 = 0.4;
/// Maximum score-map cells allocated by one prepared query. At this ceiling,
/// the f32 score map is ~16 MiB; two f64 integral-moment planes are ~64 MiB
/// for a similarly sized search image, before source/crop buffers.
pub const MAX_SCORE_POSITIONS: u64 = 4_000_000;
/// Maximum sliding-window pixel visits for one prepared query.
pub const MAX_TEMPLATE_MATCH_PIXEL_VISITS: u64 = 250_000_000;
/// Oversampling before NMS so one strong cluster does not hide later instances.
const PEAK_OVERSAMPLE: u32 = 64;

fn gray_variance(gray: &image::GrayImage) -> f32 {
    let n = f64::from(gray.width()) * f64::from(gray.height());
    if n == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;
    for p in gray.pixels() {
        let v = f64::from(p.0[0]);
        sum += v;
        sum_sq += v * v;
    }
    let mean = sum / n;
    ((sum_sq / n) - mean * mean) as f32
}

pub(crate) fn prepare_template_match(
    index: &VisualIndex,
    store: &TemplateStore,
    q: &TemplateMatchQuery,
) -> Result<Vec<TemplateMatch>, CapabilityError> {
    if q.limit == 0 {
        return Err(CapabilityError::InvalidInput { code: "invalid_query" });
    }
    let asset = store
        .get(&q.template_handle)
        .ok_or(CapabilityError::Failed { code: "template_not_found" })?;
    let tpl_gray = image::imageops::grayscale(&asset.bytes.to_rgba_image());
    match_template_image(index, &tpl_gray, &q.region, q.limit)
}

/// Core NCC + NMS matcher shared by the capability and self-validation. Takes a
/// grayscale template directly (no store handle).
pub(crate) fn match_template_image(
    index: &VisualIndex,
    tpl_gray: &image::GrayImage,
    region: &Region,
    limit: u32,
) -> Result<Vec<TemplateMatch>, CapabilityError> {
    if limit == 0 {
        return Err(CapabilityError::InvalidInput { code: "invalid_query" });
    }
    if gray_variance(tpl_gray) < MIN_TEMPLATE_VARIANCE {
        return Err(CapabilityError::InvalidInput { code: "template_low_information" });
    }
    let (tw, th) = tpl_gray.dimensions();
    if tw == 0 || th == 0 {
        return Err(CapabilityError::InvalidInput { code: "template_low_information" });
    }

    let search = region_to_pixel_rect(region, index.width(), index.height(), MAX_SEARCH_AREA)?;
    if tw > search.width || th > search.height {
        return Err(CapabilityError::InvalidInput { code: "template_larger_than_region" });
    }
    let positions = u64::from(search.width - tw + 1)
        .checked_mul(u64::from(search.height - th + 1))
        .ok_or(CapabilityError::InvalidInput { code: "region_too_large" })?;
    let template_area = u64::from(tw)
        .checked_mul(u64::from(th))
        .ok_or(CapabilityError::InvalidInput { code: "region_too_large" })?;
    let pixel_visits = positions
        .checked_mul(template_area)
        .ok_or(CapabilityError::InvalidInput { code: "region_too_large" })?;
    if positions > MAX_SCORE_POSITIONS || pixel_visits > MAX_TEMPLATE_MATCH_PIXEL_VISITS {
        return Err(CapabilityError::InvalidInput { code: "region_too_large" });
    }

    let scene = image::imageops::crop_imm(index.gray(), search.x, search.y, search.width, search.height)
        .to_image();

    let raw_map: image::ImageBuffer<Luma<f32>, Vec<f32>> =
        if scene.width() == tw || scene.height() == th {
            match_equal_dimension(&scene, tpl_gray)
        } else {
            match_template(&scene, tpl_gray, MatchTemplateMethod::CrossCorrelation)
        };
    let score_map = zero_mean_normalize(&scene, tpl_gray, raw_map);

    let candidate_cap = limit
        .saturating_mul(PEAK_OVERSAMPLE)
        .clamp(64, 8_192) as usize;
    let mut candidates = std::collections::BinaryHeap::<
        std::cmp::Reverse<Peak>,
    >::with_capacity(candidate_cap);
    for (mx, my, px) in score_map.enumerate_pixels() {
        let score = px.0[0];
        if !score.is_finite() {
            continue;
        }
        let peak = Peak { score, x: search.x + mx, y: search.y + my };
        if candidates.len() < candidate_cap {
            candidates.push(std::cmp::Reverse(peak));
        } else if candidates.peek().is_some_and(|worst| peak > worst.0) {
            candidates.pop();
            candidates.push(std::cmp::Reverse(peak));
        }
    }

    let mut candidates: Vec<_> = candidates.into_iter().map(|p| p.0).collect();
    candidates.sort_by(|a, b| b.cmp(a));

    let mut kept: Vec<(f32, ImageRect)> = Vec::new();
    for peak in candidates {
        let score = peak.score;
        let rect = ImageRect {
            x: peak.x as f32,
            y: peak.y as f32,
            width: tw as f32,
            height: th as f32,
        };
        if kept.iter().any(|(_, k)| iou(*k, rect) > NMS_IOU_THRESHOLD) {
            continue;
        }
        kept.push((score, rect));
        if kept.len() as u32 >= limit {
            break;
        }
    }

    Ok(kept
        .into_iter()
        .map(|(score, bounds)| TemplateMatch {
            bounds,
            score,
            anchor: ImagePoint::new(bounds.x + bounds.width / 2.0, bounds.y + bounds.height / 2.0),
        })
        .collect())
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Peak {
    score: f32,
    x: u32,
    y: u32,
}

impl Eq for Peak {}

impl Ord for Peak {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.score
            .total_cmp(&other.score)
            .then_with(|| other.x.cmp(&self.x))
            .then_with(|| other.y.cmp(&self.y))
    }
}

impl PartialOrd for Peak {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn match_equal_dimension(
    scene: &image::GrayImage,
    template: &image::GrayImage,
) -> image::ImageBuffer<Luma<f32>, Vec<f32>> {
    let out_w = scene.width() - template.width() + 1;
    let out_h = scene.height() - template.height() + 1;
    image::ImageBuffer::from_fn(out_w, out_h, |x, y| {
        Luma([dot_at(scene, template, x, y) as f32])
    })
}

fn dot_at(
    scene: &image::GrayImage,
    template: &image::GrayImage,
    offset_x: u32,
    offset_y: u32,
) -> f64 {
    let mut dot = 0.0f64;
    for y in 0..template.height() {
        for x in 0..template.width() {
            let s = f64::from(scene.get_pixel(offset_x + x, offset_y + y).0[0]);
            let t = f64::from(template.get_pixel(x, y).0[0]);
            dot += s * t;
        }
    }
    dot
}

fn zero_mean_normalize(
    scene: &image::GrayImage,
    template: &image::GrayImage,
    raw_map: image::ImageBuffer<Luma<f32>, Vec<f32>>,
) -> image::ImageBuffer<Luma<f32>, Vec<f32>> {
    let moments = IntegralMoments::build(scene);
    let n = f64::from(template.width()) * f64::from(template.height());
    let template_sum: f64 = template.pixels().map(|p| f64::from(p.0[0])).sum();
    let template_sq: f64 = template
        .pixels()
        .map(|p| {
            let v = f64::from(p.0[0]);
            v * v
        })
        .sum();
    let template_var = template_sq - template_sum * template_sum / n;

    image::ImageBuffer::from_fn(raw_map.width(), raw_map.height(), |x, y| {
        let (scene_sum, scene_sq) =
            moments.rect(x, y, template.width(), template.height());
        let scene_var = scene_sq - scene_sum * scene_sum / n;
        let numerator =
            f64::from(raw_map.get_pixel(x, y).0[0]) - scene_sum * template_sum / n;
        let score = if scene_var > 1.0 && template_var > 1.0 {
            (numerator / (scene_var * template_var).sqrt()) as f32
        } else {
            f32::NAN
        };
        Luma([score])
    })
}

struct IntegralMoments {
    width: usize,
    sum: Vec<f64>,
    square_sum: Vec<f64>,
}

impl IntegralMoments {
    fn build(image: &image::GrayImage) -> Self {
        let width = image.width() as usize + 1;
        let height = image.height() as usize + 1;
        let mut sum = vec![0.0; width * height];
        let mut square_sum = vec![0.0; width * height];
        for y in 0..image.height() as usize {
            let mut row_sum = 0.0;
            let mut row_square_sum = 0.0;
            for x in 0..image.width() as usize {
                let v = f64::from(image.get_pixel(x as u32, y as u32).0[0]);
                row_sum += v;
                row_square_sum += v * v;
                let index = (y + 1) * width + x + 1;
                sum[index] = sum[y * width + x + 1] + row_sum;
                square_sum[index] = square_sum[y * width + x + 1] + row_square_sum;
            }
        }
        Self { width, sum, square_sum }
    }

    fn rect(&self, x: u32, y: u32, width: u32, height: u32) -> (f64, f64) {
        let x0 = x as usize;
        let y0 = y as usize;
        let x1 = x0 + width as usize;
        let y1 = y0 + height as usize;
        let read = |values: &[f64]| {
            values[y1 * self.width + x1]
                - values[y0 * self.width + x1]
                - values[y1 * self.width + x0]
                + values[y0 * self.width + x0]
        };
        (read(&self.sum), read(&self.square_sum))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VisionError;
    use crate::index::VisualIndex;
    use rollshot_automation::{CapabilityError, Region, TemplateMatchQuery};

    fn bytes(w: u32, h: u32) -> TemplateBytes {
        TemplateBytes::new(w, h, vec![0u8; (w * h * 4) as usize]).unwrap()
    }

    /// 40x40 deterministic textured scene with a non-periodic 8x8 glyph pasted
    /// at (10,12) and (28,6). Returns (scene, template_bytes).
    fn scene_with_two_marks() -> (image::RgbaImage, TemplateBytes) {
        let mut scene = image::RgbaImage::from_fn(40, 40, |x, y| {
            let v = 120 + ((x * 3 + y * 5) % 23) as u8;
            image::Rgba([v, v, v, 255])
        });
        for &(ox, oy) in &[(10u32, 12u32), (28, 6)] {
            for dy in 0..8 {
                for dx in 0..8 {
                    let v = ((dx * 31 + dy * 17 + dx * dy * 7) % 220) as u8;
                    scene.put_pixel(ox + dx, oy + dy, image::Rgba([v, v, v, 255]));
                }
            }
        }
        let tpl_img = image::imageops::crop_imm(&scene, 10, 12, 8, 8).to_image();
        let bytes = TemplateBytes::new(8, 8, tpl_img.into_raw()).unwrap();
        (scene, bytes)
    }

    fn store_with(handle: &str, bytes: TemplateBytes, s: TemplateSensitivity) -> TemplateStore {
        let mut store = TemplateStore::new();
        store.insert(TemplateAsset {
            handle: handle.into(),
            sensitivity: s,
            source: TemplateSource::UserRect,
            created_at_ms: 0,
            bounds_in_source_image: None,
            bytes,
        })
        .unwrap();
        store
    }

    #[test]
    fn finds_both_instances_with_nms() {
        let (scene, tpl) = scene_with_two_marks();
        let index = VisualIndex::build(scene).unwrap();
        let store = store_with("mark", tpl, TemplateSensitivity::Chrome);
        let matches = prepare_template_match(
            &index,
            &store,
            &TemplateMatchQuery {
                template_handle: "mark".into(),
                region: Region::Full,
                limit: 2,
            },
        )
        .unwrap();
        assert_eq!(matches.len(), 2);
        assert!(matches.iter().all(|m| m.score > 0.99));
        let positions: std::collections::BTreeSet<_> = matches
            .iter()
            .map(|m| (m.bounds.x as i32, m.bounds.y as i32))
            .collect();
        assert_eq!(positions, [(10, 12), (28, 6)].into_iter().collect());
        assert_eq!((matches[0].bounds.width, matches[0].bounds.height), (8.0, 8.0));
        let c = matches[0].bounds;
        assert!((matches[0].anchor.x - (c.x + c.width / 2.0)).abs() < 1e-3);
    }

    #[test]
    fn limit_is_respected() {
        let (scene, tpl) = scene_with_two_marks();
        let index = VisualIndex::build(scene).unwrap();
        let store = store_with("mark", tpl, TemplateSensitivity::Chrome);
        let matches = prepare_template_match(
            &index,
            &store,
            &TemplateMatchQuery {
                template_handle: "mark".into(),
                region: Region::Full,
                limit: 1,
            },
        )
        .unwrap();
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn missing_handle_is_typed_error() {
        let (scene, _tpl) = scene_with_two_marks();
        let index = VisualIndex::build(scene).unwrap();
        let store = TemplateStore::new();
        let e = prepare_template_match(
            &index,
            &store,
            &TemplateMatchQuery {
                template_handle: "nope".into(),
                region: Region::Full,
                limit: 10,
            },
        )
        .unwrap_err();
        assert_eq!(e, CapabilityError::Failed { code: "template_not_found" });
    }

    #[test]
    fn low_information_template_is_rejected() {
        let scene = image::RgbaImage::from_pixel(40, 40, image::Rgba([180, 180, 180, 255]));
        let index = VisualIndex::build(scene).unwrap();
        let flat = TemplateBytes::new(8, 8, vec![180u8; 8 * 8 * 4]).unwrap();
        let store = store_with("flat", flat, TemplateSensitivity::Chrome);
        let e = prepare_template_match(
            &index,
            &store,
            &TemplateMatchQuery {
                template_handle: "flat".into(),
                region: Region::Full,
                limit: 10,
            },
        )
        .unwrap_err();
        assert_eq!(e, CapabilityError::InvalidInput { code: "template_low_information" });
    }

    #[test]
    fn template_larger_than_region_is_error() {
        let scene = image::RgbaImage::from_pixel(6, 6, image::Rgba([180, 180, 180, 255]));
        let index = VisualIndex::build(scene).unwrap();
        let mut big_rgba = vec![0u8; 8 * 8 * 4];
        for i in 0..(8 * 8) {
            let v = ((i * 37) % 251) as u8;
            big_rgba[i * 4..i * 4 + 4].copy_from_slice(&[v, v, v, 255]);
        }
        let big = TemplateBytes::new(8, 8, big_rgba).unwrap();
        let store = store_with("big", big, TemplateSensitivity::Chrome);
        let e = prepare_template_match(
            &index,
            &store,
            &TemplateMatchQuery {
                template_handle: "big".into(),
                region: Region::Full,
                limit: 10,
            },
        )
        .unwrap_err();
        assert_eq!(e, CapabilityError::InvalidInput { code: "template_larger_than_region" });
    }

    #[test]
    fn zero_limit_is_rejected_by_core_api() {
        let (scene, tpl) = scene_with_two_marks();
        let index = VisualIndex::build(scene).unwrap();
        let store = store_with("mark", tpl, TemplateSensitivity::Chrome);
        let e = prepare_template_match(
            &index,
            &store,
            &TemplateMatchQuery {
                template_handle: "mark".into(),
                region: Region::Full,
                limit: 0,
            },
        )
        .unwrap_err();
        assert_eq!(e, CapabilityError::InvalidInput { code: "invalid_query" });
    }

    #[test]
    fn template_equal_to_region_scores_one_position_without_panicking() {
        let (scene, tpl) = scene_with_two_marks();
        let exact = image::imageops::crop_imm(&scene, 10, 12, 8, 8).to_image();
        let index = VisualIndex::build(exact).unwrap();
        let store = store_with("exact", tpl, TemplateSensitivity::Chrome);
        let matches = prepare_template_match(
            &index,
            &store,
            &TemplateMatchQuery {
                template_handle: "exact".into(),
                region: Region::Full,
                limit: 1,
            },
        )
        .unwrap();
        assert_eq!(matches.len(), 1);
        assert!(matches[0].score > 0.99);
    }

    #[test]
    fn excessive_match_work_is_rejected_before_matching() {
        let scene = image::RgbaImage::from_pixel(1000, 1000, image::Rgba([80, 80, 80, 255]));
        let index = VisualIndex::build(scene).unwrap();
        let tpl_image = image::RgbaImage::from_fn(64, 64, |x, y| {
            let v = ((x * 31 + y * 17 + x * y * 7) % 251) as u8;
            image::Rgba([v, v, v, 255])
        });
        let tpl = TemplateBytes::new(64, 64, tpl_image.into_raw()).unwrap();
        let store = store_with("mark", tpl, TemplateSensitivity::Chrome);
        let e = prepare_template_match(
            &index,
            &store,
            &TemplateMatchQuery {
                template_handle: "mark".into(),
                region: Region::Full,
                limit: 1,
            },
        )
        .unwrap_err();
        assert_eq!(e, CapabilityError::InvalidInput { code: "region_too_large" });
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
