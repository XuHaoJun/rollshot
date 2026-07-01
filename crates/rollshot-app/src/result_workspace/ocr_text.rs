use std::hash::{Hash, Hasher};

use rollshot_image_document::{Annotation, AnnotationId, ImagePoint, ImageRect};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OcrItemId(pub u64);

#[derive(Debug, Clone, PartialEq)]
pub struct OcrTextItem {
    pub id: OcrItemId,
    pub text: String,
    pub confidence: f32,
    pub bounds: ImageRect,
    pub quad: [ImagePoint; 4],
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

#[derive(Debug, Clone, PartialEq)]
pub struct OcrTextDocument {
    visible_items: Vec<OcrTextItem>,
    line_break_after: Vec<bool>,
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
            Some(item) => TextCursor::new(
                self.visible_items.len() - 1,
                item.text.chars().count(),
            ),
            None => TextCursor::new(0, 0),
        }
    }

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
        Annotation::OpaqueRedaction { bounds: redaction, .. } => bounds.intersects(redaction),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> ImageRect {
        ImageRect { x, y, width, height }
    }

    fn quad(bounds: ImageRect) -> [ImagePoint; 4] {
        [
            ImagePoint { x: bounds.x, y: bounds.y },
            ImagePoint { x: bounds.x + bounds.width, y: bounds.y },
            ImagePoint {
                x: bounds.x + bounds.width,
                y: bounds.y + bounds.height,
            },
            ImagePoint { x: bounds.x, y: bounds.y + bounds.height },
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
    fn reverse_selection_copies_same_text_as_forward_selection() {
        let items = vec![
            item(1, "alpha", rect(10.0, 10.0, 50.0, 12.0)),
            item(2, "beta", rect(70.0, 10.0, 40.0, 12.0)),
            item(3, "gamma", rect(10.0, 40.0, 60.0, 12.0)),
        ];
        let doc = OcrTextDocument::from_items(items, &[]);

        let forward = OcrSelection::range(TextCursor::new(0, 1), TextCursor::new(2, 3));
        let backward = OcrSelection::range(TextCursor::new(2, 3), TextCursor::new(0, 1));

        assert_eq!(
            doc.selected_text(&forward),
            doc.selected_text(&backward)
        );
        assert_eq!(doc.selected_text(&forward), "lpha beta\ngam");
    }
}
