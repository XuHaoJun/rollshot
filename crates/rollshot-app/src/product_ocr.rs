#![allow(dead_code)]

use rollshot_image_document::{ImagePoint, ImageRect};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OcrItemId(pub u64);

#[derive(Clone, PartialEq)]
pub struct OcrTextItem {
    pub id: OcrItemId,
    pub text: String,
    pub confidence: f32,
    pub bounds: ImageRect,
    pub quad: [ImagePoint; 4],
}

impl std::fmt::Debug for OcrTextItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcrTextItem")
            .field("id", &self.id)
            .field("confidence", &self.confidence)
            .field("bounds", &self.bounds)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductOcrError {
    #[allow(dead_code)]
    Disabled,
    SessionInit,
    Detect,
    InvalidRegion,
    EmptyResult,
}

impl ProductOcrError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Disabled => "OCR is not available in this build",
            Self::SessionInit => "OCR session initialization failed",
            Self::Detect => "OCR detection failed",
            Self::InvalidRegion => "OCR region is invalid",
            Self::EmptyResult => "No text was recognized in the selected region",
        }
    }
}

fn reading_order(a: &OcrTextItem, b: &OcrTextItem) -> std::cmp::Ordering {
    if same_line(a.bounds, b.bounds) {
        a.bounds
            .x
            .partial_cmp(&b.bounds.x)
            .unwrap_or(std::cmp::Ordering::Equal)
    } else {
        a.bounds
            .y
            .partial_cmp(&b.bounds.y)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

fn same_line(a: ImageRect, b: ImageRect) -> bool {
    let a_mid = a.y + a.height / 2.0;
    let b_mid = b.y + b.height / 2.0;
    (a_mid - b_mid).abs() <= a.height.max(b.height) * 0.6
}

pub struct OrderedOcrItems {
    items: Vec<OcrTextItem>,
    line_break_after: Vec<bool>,
}

impl OrderedOcrItems {
    pub fn new(mut items: Vec<OcrTextItem>) -> Self {
        items.sort_by(reading_order);
        let line_break_after = items
            .windows(2)
            .map(|pair| !same_line(pair[0].bounds, pair[1].bounds))
            .collect();
        Self {
            items,
            line_break_after,
        }
    }

    pub fn as_parts(&self) -> (&[OcrTextItem], &[bool]) {
        (&self.items, &self.line_break_after)
    }

    pub fn into_parts(self) -> (Vec<OcrTextItem>, Vec<bool>) {
        (self.items, self.line_break_after)
    }

    pub fn into_text(self) -> Result<String, ProductOcrError> {
        let mut out = String::new();
        for (index, item) in self.items.iter().enumerate() {
            out.push_str(&item.text);
            if index + 1 < self.items.len() {
                out.push(if self.line_break_after[index] {
                    '\n'
                } else {
                    ' '
                });
            }
        }
        let text = out.trim().to_owned();
        (!text.is_empty())
            .then_some(text)
            .ok_or(ProductOcrError::EmptyResult)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OcrTile {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

pub fn vertical_tiles(width: u32, height: u32, max_area: u64, overlap: u32) -> Vec<OcrTile> {
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let tile_height = ((max_area / width.max(1) as u64) as u32).max(1).min(height);
    let step = tile_height.saturating_sub(overlap).max(1);
    let mut tiles = Vec::new();
    let mut y = 0;
    loop {
        let remaining = height - y;
        let h = tile_height.min(remaining);
        tiles.push(OcrTile {
            x: 0,
            y,
            width,
            height: h,
        });
        if y + h >= height {
            break;
        }
        y = (y + step).min(height - 1);
    }
    tiles
}

pub fn merge_tile_items(mut items: Vec<OcrTextItem>) -> Vec<OcrTextItem> {
    items.sort_by(reading_order);
    let mut merged: Vec<OcrTextItem> = Vec::new();
    'items: for item in items {
        for existing in &merged {
            if existing.text == item.text && iou(existing.bounds, item.bounds) >= 0.80 {
                continue 'items;
            }
        }
        merged.push(item);
    }
    for (index, item) in merged.iter_mut().enumerate() {
        item.id = OcrItemId(index as u64);
    }
    merged
}

fn iou(a: ImageRect, b: ImageRect) -> f32 {
    let ax2 = a.x + a.width;
    let ay2 = a.y + a.height;
    let bx2 = b.x + b.width;
    let by2 = b.y + b.height;
    let ix = (ax2.min(bx2) - a.x.max(b.x)).max(0.0);
    let iy = (ay2.min(by2) - a.y.max(b.y)).max(0.0);
    let intersection = ix * iy;
    let union = a.width * a.height + b.width * b.height - intersection;
    if union <= 0.0 {
        0.0
    } else {
        intersection / union
    }
}

#[cfg(feature = "ocr")]
pub fn prepare(image: &image::RgbaImage) -> Result<Vec<OcrTextItem>, ProductOcrError> {
    use rollshot_automation::{AutomationHost, OcrQuery, Region};
    use rollshot_vision::{rect::MAX_OCR_AREA, RealAutomationHost, VisualIndex};

    let index = VisualIndex::build(image.clone()).map_err(|_| ProductOcrError::InvalidRegion)?;
    let mut host = RealAutomationHost::new();
    let tiles = vertical_tiles(index.width(), index.height(), MAX_OCR_AREA, 64);
    let mut items = Vec::new();

    for tile in tiles {
        let query = OcrQuery {
            region: Region::Rect {
                bounds: ImageRect {
                    x: tile.x as f32,
                    y: tile.y as f32,
                    width: tile.width as f32,
                    height: tile.height as f32,
                },
            },
            limit: 5_000,
        };
        host.prepare_ocr(&index, &query)
            .map_err(map_capability_error)?;

        for m in host.ocr(query).map_err(|_| ProductOcrError::Detect)? {
            let id = OcrItemId(items.len() as u64);
            items.push(OcrTextItem {
                id,
                text: m.text,
                confidence: m.confidence,
                bounds: m.bounds,
                quad: m.quad,
            });
        }
    }

    Ok(merge_tile_items(items))
}

#[cfg(not(feature = "ocr"))]
pub fn prepare(_image: &image::RgbaImage) -> Result<Vec<OcrTextItem>, ProductOcrError> {
    Err(ProductOcrError::Disabled)
}

#[cfg(feature = "ocr")]
fn map_capability_error(error: rollshot_automation::CapabilityError) -> ProductOcrError {
    match error {
        rollshot_automation::CapabilityError::Failed {
            code: "ocr_session_init",
        } => ProductOcrError::SessionInit,
        rollshot_automation::CapabilityError::Failed { code: "ocr_detect" } => {
            ProductOcrError::Detect
        }
        _ => ProductOcrError::InvalidRegion,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> ImageRect {
        ImageRect {
            x,
            y,
            width,
            height,
        }
    }

    fn quad(bounds: ImageRect) -> [ImagePoint; 4] {
        [
            ImagePoint {
                x: bounds.x,
                y: bounds.y,
            },
            ImagePoint {
                x: bounds.x + bounds.width,
                y: bounds.y,
            },
            ImagePoint {
                x: bounds.x + bounds.width,
                y: bounds.y + bounds.height,
            },
            ImagePoint {
                x: bounds.x,
                y: bounds.y + bounds.height,
            },
        ]
    }

    fn item(id: u64, text: &str, bounds: ImageRect) -> OcrTextItem {
        OcrTextItem {
            id: OcrItemId(id),
            text: text.into(),
            confidence: 0.95,
            bounds,
            quad: quad(bounds),
        }
    }

    #[test]
    fn assemble_text_orders_lines_and_words() {
        let items = vec![
            item(2, "world", rect(80.0, 10.0, 50.0, 12.0)),
            item(0, "second", rect(10.0, 50.0, 60.0, 12.0)),
            item(1, "hello", rect(10.0, 11.0, 50.0, 12.0)),
        ];
        assert_eq!(
            OrderedOcrItems::new(items).into_text().unwrap(),
            "hello world\nsecond"
        );
    }

    #[test]
    fn assemble_text_rejects_empty_or_whitespace_only_results() {
        assert_eq!(
            OrderedOcrItems::new(vec![]).into_text(),
            Err(ProductOcrError::EmptyResult)
        );
        let whitespace_item = OcrTextItem {
            id: OcrItemId(0),
            text: "   ".into(),
            confidence: 0.0,
            bounds: rect(0.0, 0.0, 10.0, 10.0),
            quad: quad(rect(0.0, 0.0, 10.0, 10.0)),
        };
        assert_eq!(
            OrderedOcrItems::new(vec![whitespace_item]).into_text(),
            Err(ProductOcrError::EmptyResult)
        );
    }

    #[test]
    fn debug_omits_recognized_text() {
        let bounds = rect(0.0, 0.0, 10.0, 10.0);
        let item = OcrTextItem {
            id: OcrItemId(0),
            text: "PRIVATE_OCR_SENTINEL".into(),
            confidence: 0.0,
            bounds,
            quad: quad(bounds),
        };
        assert!(!format!("{item:?}").contains("PRIVATE_OCR_SENTINEL"));
    }

    #[test]
    fn ordered_items_as_parts_and_into_parts() {
        let items = vec![
            item(1, "beta", rect(70.0, 10.0, 40.0, 12.0)),
            item(0, "alpha", rect(10.0, 10.0, 50.0, 12.0)),
        ];
        let ordered = OrderedOcrItems::new(items);
        let (part_items, breaks) = ordered.as_parts();
        assert_eq!(part_items.len(), 2);
        assert_eq!(part_items[0].text, "alpha");
        assert_eq!(part_items[1].text, "beta");
        assert!(!breaks[0]);
    }

    #[test]
    fn vertical_tiles_overlap_and_cover_full_height() {
        let tiles = vertical_tiles(1200, 40_000, 16_000_000, 64);

        assert_eq!(tiles.first().unwrap().y, 0);
        assert_eq!(
            tiles.last().unwrap().y + tiles.last().unwrap().height,
            40_000
        );
        for pair in tiles.windows(2) {
            let first_bottom = pair[0].y + pair[0].height;
            assert!(pair[1].y < first_bottom);
        }
    }

    #[test]
    fn seam_merge_removes_duplicate_text_with_high_iou() {
        let bounds = rect(10.0, 100.0, 120.0, 24.0);
        let duplicate = OcrTextItem {
            id: OcrItemId(99),
            text: "duplicate".into(),
            confidence: 0.90,
            bounds,
            quad: quad(bounds),
        };
        let merged = merge_tile_items(vec![
            OcrTextItem {
                id: OcrItemId(1),
                ..duplicate.clone()
            },
            OcrTextItem {
                id: OcrItemId(2),
                ..duplicate
            },
        ]);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].text, "duplicate");
    }

    #[test]
    fn error_messages_are_human_readable() {
        assert!(!ProductOcrError::Disabled.message().is_empty());
        assert!(!ProductOcrError::SessionInit.message().is_empty());
        assert!(!ProductOcrError::Detect.message().is_empty());
        assert!(!ProductOcrError::InvalidRegion.message().is_empty());
        assert!(!ProductOcrError::EmptyResult.message().is_empty());
    }
}
