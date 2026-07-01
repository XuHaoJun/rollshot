use std::hash::{Hash, Hasher};

use rollshot_image_document::{Annotation, ImagePoint, ImageRect};

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
pub struct TextCursor {
    pub item_index: usize,
    pub char_index: usize,
}

impl TextCursor {
    pub fn new(item_index: usize, char_index: usize) -> Self {
        Self {
            item_index,
            char_index,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OcrSelection {
    pub anchor: TextCursor,
    pub focus: TextCursor,
}

impl OcrSelection {
    pub fn range(anchor: TextCursor, focus: TextCursor) -> Self {
        Self { anchor, focus }
    }

    pub fn normalized(self) -> (TextCursor, TextCursor) {
        if (self.anchor.item_index, self.anchor.char_index)
            <= (self.focus.item_index, self.focus.char_index)
        {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct OcrTextDocument {
    visible_items: Vec<OcrTextItem>,
    line_break_after: Vec<bool>,
}

impl std::fmt::Debug for OcrTextDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcrTextDocument")
            .field("visible_item_count", &self.visible_items.len())
            .finish()
    }
}

impl OcrTextDocument {
    pub fn from_items(items: Vec<OcrTextItem>, redactions: &[Annotation]) -> Self {
        let mut visible_items: Vec<OcrTextItem> = items
            .into_iter()
            .filter(|item| !is_redacted(item.bounds, redactions))
            .collect();
        visible_items.sort_by(reading_order);

        let mut line_break_after = vec![false; visible_items.len()];
        for index in 0..visible_items.len().saturating_sub(1) {
            let current = visible_items[index].bounds;
            let next = visible_items[index + 1].bounds;
            line_break_after[index] = !same_line(current, next);
        }

        Self {
            visible_items,
            line_break_after,
        }
    }

    #[allow(dead_code)]
    pub fn visible_items(&self) -> &[OcrTextItem] {
        &self.visible_items
    }

    pub fn copy_all_text(&self) -> String {
        self.text_for_range(TextCursor::new(0, 0), self.end_cursor())
    }

    pub fn selected_text(&self, selection: &OcrSelection) -> String {
        let (start, end) = selection.normalized();
        self.text_for_range(start, end)
    }

    pub fn end_cursor(&self) -> TextCursor {
        match self.visible_items.last() {
            Some(item) => TextCursor::new(self.visible_items.len() - 1, item.text.chars().count()),
            None => TextCursor::new(0, 0),
        }
    }

    #[allow(dead_code)]
    pub fn selection_is_valid(&self, selection: &OcrSelection) -> bool {
        let (start, end) = selection.normalized();
        let Some(last) = self.visible_items.len().checked_sub(1) else {
            return false;
        };
        start.item_index <= last && end.item_index <= last
    }

    fn text_for_range(&self, start: TextCursor, end: TextCursor) -> String {
        if self.visible_items.is_empty() {
            return String::new();
        }

        let mut out = String::new();
        for index in start.item_index..=end.item_index.min(self.visible_items.len() - 1) {
            let item = &self.visible_items[index];
            let start_char = if index == start.item_index {
                start.char_index
            } else {
                0
            };
            let end_char = if index == end.item_index {
                end.char_index
            } else {
                item.text.chars().count()
            };
            if start_char < end_char {
                out.push_str(&slice_chars(&item.text, start_char, end_char));
            }
            if index < end.item_index && index < self.visible_items.len() - 1 {
                if self.line_break_after[index] {
                    out.push('\n');
                } else {
                    out.push(' ');
                }
            }
        }
        out.trim().to_string()
    }
}

fn is_redacted(bounds: ImageRect, redactions: &[Annotation]) -> bool {
    redactions.iter().any(|annotation| match annotation {
        Annotation::OpaqueRedaction {
            bounds: redaction, ..
        } => bounds.intersects(redaction),
        _ => false,
    })
}

pub fn redaction_signature(redactions: &[Annotation]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for annotation in redactions {
        if let Annotation::OpaqueRedaction { id, bounds } = annotation {
            id.hash(&mut hasher);
            bounds.x.to_bits().hash(&mut hasher);
            bounds.y.to_bits().hash(&mut hasher);
            bounds.width.to_bits().hash(&mut hasher);
            bounds.height.to_bits().hash(&mut hasher);
        }
    }
    hasher.finish()
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

fn slice_chars(text: &str, start: usize, end: usize) -> String {
    text.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProductOcrError {
    #[allow(dead_code)]
    Disabled,
    SessionInit,
    Detect,
    InvalidRegion,
}

impl ProductOcrError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Disabled => "OCR is not available in this build",
            Self::SessionInit => "OCR session initialization failed",
            Self::Detect => "OCR detection failed",
            Self::InvalidRegion => "OCR region is invalid",
        }
    }
}

#[derive(Clone, PartialEq)]
pub enum OcrTextStatus {
    Idle,
    Preparing,
    Ready(OcrTextDocument),
    Failed(ProductOcrError),
}

impl std::fmt::Debug for OcrTextStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            OcrTextStatus::Idle => "Idle",
            OcrTextStatus::Preparing => "Preparing",
            OcrTextStatus::Ready(_) => "Ready",
            OcrTextStatus::Failed(_) => "Failed",
        })
    }
}

#[derive(Clone, PartialEq)]
pub struct OcrTextState {
    status: OcrTextStatus,
    selection: Option<OcrSelection>,
    raw_items: Vec<OcrTextItem>,
    redaction_signature: u64,
}

impl std::fmt::Debug for OcrTextState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcrTextState")
            .field("status", &self.status_name())
            .field("has_selection", &self.selection.is_some())
            .field("raw_item_count", &self.raw_items.len())
            .finish()
    }
}

impl OcrTextState {
    pub fn idle() -> Self {
        Self {
            status: OcrTextStatus::Idle,
            selection: None,
            raw_items: Vec::new(),
            redaction_signature: 0,
        }
    }

    fn status_name(&self) -> &'static str {
        match &self.status {
            OcrTextStatus::Idle => "idle",
            OcrTextStatus::Preparing => "preparing",
            OcrTextStatus::Ready(_) => "ready",
            OcrTextStatus::Failed(_) => "failed",
        }
    }

    pub fn begin_prepare(&mut self) {
        self.status = OcrTextStatus::Preparing;
        self.selection = None;
        self.raw_items.clear();
        self.redaction_signature = 0;
    }

    pub fn finish_prepare(&mut self, items: Vec<OcrTextItem>, redactions: &[Annotation]) {
        self.redaction_signature = redaction_signature(redactions);
        self.raw_items = items.clone();
        self.status = OcrTextStatus::Ready(OcrTextDocument::from_items(items, redactions));
        self.selection = None;
    }

    pub fn fail_prepare(&mut self, error: ProductOcrError) {
        self.status = OcrTextStatus::Failed(error);
        self.selection = None;
        self.raw_items.clear();
        self.redaction_signature = 0;
    }

    #[allow(dead_code)]
    pub fn is_preparing_or_ready(&self) -> bool {
        matches!(
            &self.status,
            OcrTextStatus::Preparing | OcrTextStatus::Ready(_)
        )
    }

    pub fn document(&self) -> Option<&OcrTextDocument> {
        match &self.status {
            OcrTextStatus::Ready(document) => Some(document),
            _ => None,
        }
    }

    pub fn selection(&self) -> Option<&OcrSelection> {
        self.selection.as_ref()
    }

    pub fn set_selection(&mut self, selection: Option<OcrSelection>) {
        self.selection = selection;
    }

    #[allow(dead_code)]
    pub fn refresh_redactions(&mut self, redactions: &[Annotation]) {
        if !matches!(&self.status, OcrTextStatus::Ready(_)) {
            return;
        }
        let signature = redaction_signature(redactions);
        if signature == self.redaction_signature {
            return;
        }

        self.redaction_signature = signature;
        let document = OcrTextDocument::from_items(self.raw_items.clone(), redactions);
        self.selection = self
            .selection
            .filter(|selection| document.selection_is_valid(selection));
        self.status = OcrTextStatus::Ready(document);
    }

    #[cfg(test)]
    pub fn set_ready_for_tests(&mut self, items: Vec<OcrTextItem>) {
        self.raw_items = items.clone();
        self.status = OcrTextStatus::Ready(OcrTextDocument::from_items(items, &[]));
        self.redaction_signature = 0;
        self.selection = None;
    }
}

#[cfg(feature = "ocr")]
pub fn prepare_product_ocr(image: &image::RgbaImage) -> Result<Vec<OcrTextItem>, ProductOcrError> {
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
            .map_err(|error| match error {
                rollshot_automation::CapabilityError::Failed {
                    code: "ocr_session_init",
                } => ProductOcrError::SessionInit,
                rollshot_automation::CapabilityError::Failed { code: "ocr_detect" } => {
                    ProductOcrError::Detect
                }
                _ => ProductOcrError::InvalidRegion,
            })?;

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
pub fn prepare_product_ocr(_image: &image::RgbaImage) -> Result<Vec<OcrTextItem>, ProductOcrError> {
    Err(ProductOcrError::Disabled)
}

pub fn character_index_for_axis_aligned_item(item: &OcrTextItem, point: ImagePoint) -> usize {
    let chars = item.text.chars().count();
    if chars == 0 || item.bounds.width <= 0.0 {
        return 0;
    }
    let t = ((point.x - item.bounds.x) / item.bounds.width).clamp(0.0, 1.0);
    ((chars as f32) * t).round() as usize
}

pub fn char_index_for_byte_offset(text: &str, byte_offset: usize) -> usize {
    let end = byte_offset.min(text.len());
    text.char_indices()
        .take_while(|(index, _)| *index < end)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_image_document::AnnotationId;

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
    fn normalized_order_groups_lines_top_to_bottom_left_to_right() {
        let items = vec![
            item(3, "second", rect(10.0, 50.0, 60.0, 12.0)),
            item(2, "world", rect(80.0, 10.0, 50.0, 12.0)),
            item(1, "hello", rect(10.0, 11.0, 50.0, 12.0)),
        ];

        let doc = OcrTextDocument::from_items(items, &[]);
        assert_eq!(doc.copy_all_text(), "hello world\nsecond");
    }

    #[test]
    fn redaction_intersections_are_not_copyable() {
        let items = vec![
            item(1, "visible", rect(10.0, 10.0, 50.0, 12.0)),
            item(2, "secret", rect(10.0, 40.0, 50.0, 12.0)),
        ];
        let redactions = vec![Annotation::OpaqueRedaction {
            id: AnnotationId(1),
            bounds: rect(8.0, 38.0, 60.0, 18.0),
        }];

        let doc = OcrTextDocument::from_items(items, &redactions);
        assert_eq!(doc.copy_all_text(), "visible");
        assert!(doc.visible_items().iter().all(|item| item.text != "secret"));
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
        let bounds = ImageRect {
            x: 10.0,
            y: 100.0,
            width: 120.0,
            height: 24.0,
        };
        let duplicate = OcrTextItem {
            id: OcrItemId(99),
            text: "duplicate".into(),
            confidence: 0.90,
            bounds,
            quad: [
                ImagePoint { x: 10.0, y: 100.0 },
                ImagePoint { x: 130.0, y: 100.0 },
                ImagePoint { x: 130.0, y: 124.0 },
                ImagePoint { x: 10.0, y: 124.0 },
            ],
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
    fn reverse_selection_copies_same_text_as_forward_selection() {
        let items = vec![
            item(1, "alpha", rect(10.0, 10.0, 50.0, 12.0)),
            item(2, "beta", rect(70.0, 10.0, 40.0, 12.0)),
            item(3, "gamma", rect(10.0, 40.0, 60.0, 12.0)),
        ];
        let doc = OcrTextDocument::from_items(items, &[]);

        let forward = OcrSelection::range(TextCursor::new(0, 1), TextCursor::new(2, 3));
        let backward = OcrSelection::range(TextCursor::new(2, 3), TextCursor::new(0, 1));

        assert_eq!(doc.selected_text(&forward), doc.selected_text(&backward));
        assert_eq!(doc.selected_text(&forward), "lpha beta\ngam");
    }

    #[test]
    fn whole_document_selection_copies_all_text() {
        let items = vec![
            item(1, "alpha", rect(10.0, 10.0, 50.0, 12.0)),
            item(2, "beta", rect(70.0, 10.0, 40.0, 12.0)),
            item(3, "gamma", rect(10.0, 40.0, 60.0, 12.0)),
        ];
        let doc = OcrTextDocument::from_items(items, &[]);
        let selection = OcrSelection::range(TextCursor::new(0, 0), doc.end_cursor());

        assert_eq!(doc.selected_text(&selection), doc.copy_all_text());
        assert_eq!(doc.selected_text(&selection), "alpha beta\ngamma");
    }

    #[test]
    fn redacted_item_cannot_be_selected_after_initial_filtering() {
        let items = vec![
            item(1, "public", rect(0.0, 0.0, 60.0, 12.0)),
            item(2, "private", rect(0.0, 30.0, 70.0, 12.0)),
        ];
        let redactions = vec![Annotation::OpaqueRedaction {
            id: AnnotationId(9),
            bounds: rect(0.0, 25.0, 90.0, 25.0),
        }];
        let doc = OcrTextDocument::from_items(items, &redactions);

        assert_eq!(doc.copy_all_text(), "public");
        assert!(doc
            .visible_items()
            .iter()
            .all(|item| item.text != "private"));
    }

    #[test]
    fn redaction_refresh_removes_stale_selection() {
        let items = vec![
            item(1, "public", rect(0.0, 0.0, 60.0, 12.0)),
            item(2, "private", rect(0.0, 30.0, 70.0, 12.0)),
        ];
        let mut state = OcrTextState::idle();
        state.finish_prepare(items, &[]);
        state.set_selection(Some(OcrSelection::range(
            TextCursor::new(1, 0),
            TextCursor::new(1, 7),
        )));

        let redactions = vec![Annotation::OpaqueRedaction {
            id: AnnotationId(9),
            bounds: rect(0.0, 25.0, 90.0, 25.0),
        }];
        state.refresh_redactions(&redactions);

        assert!(state.selection().is_none());
        assert_eq!(state.document().unwrap().copy_all_text(), "public");
    }

    #[test]
    fn axis_aligned_hit_test_maps_x_to_character_index() {
        let item = item(1, "secret", rect(10.0, 10.0, 60.0, 12.0));
        assert_eq!(
            character_index_for_axis_aligned_item(&item, ImagePoint { x: 10.0, y: 12.0 }),
            0
        );
        assert_eq!(
            character_index_for_axis_aligned_item(&item, ImagePoint { x: 40.0, y: 12.0 }),
            3
        );
        assert_eq!(
            character_index_for_axis_aligned_item(&item, ImagePoint { x: 70.0, y: 12.0 }),
            6
        );
    }

    #[test]
    fn byte_offsets_from_text_layout_are_converted_to_character_indices() {
        assert_eq!(char_index_for_byte_offset("你好ab", "你好".len()), 2);
        assert_eq!(char_index_for_byte_offset("éclair", "é".len()), 1);
        assert_eq!(char_index_for_byte_offset("secret", 3), 3);
    }
}
