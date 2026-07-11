use crate::product_ocr::{OcrTextItem, OrderedOcrItems, ProductOcrError};
use std::fmt;

#[allow(dead_code)]
#[derive(Debug)]
pub enum QuickOcrError {
    Ocr(ProductOcrError),
    Clipboard(String),
}

impl fmt::Display for QuickOcrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ocr(error) => write!(f, "{}", error.message()),
            Self::Clipboard(message) => write!(f, "{message}"),
        }
    }
}

#[allow(dead_code)]
pub(crate) trait TextClipboard {
    fn set_text(&mut self, text: &str) -> Result<(), String>;
}

#[allow(dead_code)]
pub fn finish_with(
    items: Vec<OcrTextItem>,
    clipboard: &mut dyn TextClipboard,
) -> Result<String, QuickOcrError> {
    let text = OrderedOcrItems::new(items)
        .into_text()
        .map_err(QuickOcrError::Ocr)?;
    clipboard
        .set_text(&text)
        .map_err(QuickOcrError::Clipboard)?;
    Ok(text)
}

pub fn run(
    _options: rollshot_capture::InteractiveLaunchOptions,
    _graphical_feedback: bool,
) -> Result<(), String> {
    #[cfg(not(feature = "ocr"))]
    {
        Err(ProductOcrError::Disabled.message().into())
    }
    #[cfg(feature = "ocr")]
    {
        todo!("platform capture integration is Task 3")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_ocr::{OcrItemId, OcrTextItem};
    use rollshot_image_document::{ImagePoint, ImageRect};

    struct FakeClipboard {
        written: Vec<String>,
    }

    impl FakeClipboard {
        fn new() -> Self {
            Self {
                written: Vec::new(),
            }
        }
    }

    impl TextClipboard for FakeClipboard {
        fn set_text(&mut self, text: &str) -> Result<(), String> {
            self.written.push(text.to_string());
            Ok(())
        }
    }

    struct FailClipboard;

    impl TextClipboard for FailClipboard {
        fn set_text(&mut self, _text: &str) -> Result<(), String> {
            Err("clipboard unavailable".to_string())
        }
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> ImageRect {
        ImageRect {
            x,
            y,
            width,
            height,
        }
    }

    fn item(id: u64, text: &str, bounds: ImageRect) -> OcrTextItem {
        OcrTextItem {
            id: OcrItemId(id),
            text: text.into(),
            confidence: 0.95,
            bounds,
            quad: [
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
            ],
        }
    }

    #[test]
    fn finish_with_returns_text_and_writes_clipboard_once() {
        let items = vec![
            item(0, "hello", rect(10.0, 10.0, 50.0, 12.0)),
            item(1, "world", rect(60.0, 10.0, 50.0, 12.0)),
        ];
        let mut clipboard = FakeClipboard::new();
        let result = finish_with(items, &mut clipboard).unwrap();
        assert_eq!(result, "hello world");
        assert_eq!(clipboard.written.len(), 1);
        assert_eq!(clipboard.written[0], "hello world");
    }

    #[test]
    fn finish_with_empty_items_returns_error_without_touching_clipboard() {
        let mut clipboard = FakeClipboard::new();
        let err = finish_with(vec![], &mut clipboard).unwrap_err();
        assert!(matches!(
            err,
            QuickOcrError::Ocr(ProductOcrError::EmptyResult)
        ));
        assert!(clipboard.written.is_empty());
    }

    #[test]
    fn finish_with_clipboard_error_contains_no_recognized_text() {
        let items = vec![item(0, "SECRET_TEXT", rect(0.0, 0.0, 50.0, 12.0))];
        let mut clipboard = FailClipboard;
        let err = finish_with(items, &mut clipboard).unwrap_err();
        let msg = err.to_string();
        assert!(
            !msg.contains("SECRET_TEXT"),
            "error must not leak recognized text: {msg}"
        );
        assert!(
            msg.contains("clipboard"),
            "error should mention clipboard: {msg}"
        );
    }
}
