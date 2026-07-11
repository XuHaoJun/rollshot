use crate::product_ocr::{OcrTextItem, OrderedOcrItems, ProductOcrError};
use std::fmt;

#[allow(dead_code)]
#[derive(Debug)]
pub enum QuickOcrError {
    Ocr(ProductOcrError),
    Clipboard(String),
    Worker,
}

impl fmt::Display for QuickOcrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ocr(error) => write!(f, "{}", error.message()),
            Self::Clipboard(message) => write!(f, "{message}"),
            Self::Worker => write!(f, "OCR worker task panicked"),
        }
    }
}

#[allow(dead_code)]
pub(crate) trait TextClipboard {
    fn set_text(&mut self, text: &str) -> Result<(), String>;
}

/// Stdout side-effect port: writes text to the CLI output.
#[allow(dead_code)]
pub(crate) trait CliOutput {
    fn write_text(&mut self, text: &str) -> Result<(), String>;
}

/// Graphical feedback side-effect port: shows success/failure notifications.
#[allow(dead_code)]
pub(crate) trait QuickOcrFeedback {
    fn copied(&mut self) -> Result<(), String>;
    fn failed(&mut self, message: &str) -> Result<(), String>;
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

/// Complete the OCR-to-clipboard flow with injected side effects.
///
/// Clipboard is committed first; stdout and feedback follow only on success.
/// On failure, no stdout is written and feedback is called with the error.
#[allow(dead_code)]
pub(crate) fn complete_cli_with(
    items: Vec<OcrTextItem>,
    clipboard: &mut dyn TextClipboard,
    output: &mut dyn CliOutput,
    feedback: &mut dyn QuickOcrFeedback,
    graphical_feedback: bool,
) -> Result<String, QuickOcrError> {
    let text = finish_with(items, clipboard)?;
    output
        .write_text(&format!("{text}\n"))
        .map_err(QuickOcrError::Clipboard)?;
    if graphical_feedback {
        feedback.copied().map_err(QuickOcrError::Clipboard)?;
    }
    Ok(text)
}

/// Production completion: OCR the image, copy to clipboard, write to stdout,
/// and optionally show a desktop notification.
#[cfg(feature = "ocr")]
#[allow(dead_code)]
pub fn complete_cli(image: image::RgbaImage, graphical_feedback: bool) -> Result<(), String> {
    let items = crate::product_ocr::prepare(&image).map_err(|e| e.message().to_string())?;
    let mut clipboard = ArboardClipboard;
    let mut output = StdoutOutput;
    let mut feedback = NotifyFeedback;
    let _text = complete_cli_with(
        items,
        &mut clipboard,
        &mut output,
        &mut feedback,
        graphical_feedback,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(not(feature = "ocr"))]
#[allow(dead_code)]
pub fn complete_cli(_image: image::RgbaImage, _graphical_feedback: bool) -> Result<(), String> {
    Err(ProductOcrError::Disabled.message().into())
}

/// Production clipboard adapter using arboard.
pub(crate) struct ArboardClipboard;

impl TextClipboard for ArboardClipboard {
    fn set_text(&mut self, text: &str) -> Result<(), String> {
        let mut ctx = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        ctx.set_text(text).map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Production stdout adapter: writes to locked stdout.
#[allow(dead_code)]
pub(crate) struct StdoutOutput;

impl CliOutput for StdoutOutput {
    fn write_text(&mut self, text: &str) -> Result<(), String> {
        use std::io::Write;
        let mut stdout = std::io::stdout().lock();
        stdout
            .write_all(text.as_bytes())
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Production notification adapter: shows desktop notifications via notify-rust.
#[cfg(feature = "ocr")]
struct NotifyFeedback;

#[cfg(feature = "ocr")]
impl QuickOcrFeedback for NotifyFeedback {
    fn copied(&mut self) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        {
            notify_rust::Notification::new()
                .summary("Text copied")
                .body("Recognized text is in the clipboard.")
                .show()
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn failed(&mut self, message: &str) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        {
            if notify_rust::Notification::new()
                .summary("OCR failed")
                .body(message)
                .show()
                .is_err()
            {
                rfd::MessageDialog::new()
                    .set_title("OCR failed")
                    .set_description(message)
                    .show();
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = message;
        }
        Ok(())
    }
}

/// No-op feedback adapter for platforms where notify-rust is unavailable.
#[allow(dead_code)]
pub(crate) struct NoopFeedback;

#[allow(dead_code)]
impl QuickOcrFeedback for NoopFeedback {
    fn copied(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn failed(&mut self, _message: &str) -> Result<(), String> {
        Ok(())
    }
}

#[allow(dead_code)]
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

    // --- completion boundary tests ---

    struct FakeOutput {
        written: Vec<String>,
    }

    impl FakeOutput {
        fn new() -> Self {
            Self {
                written: Vec::new(),
            }
        }
    }

    impl CliOutput for FakeOutput {
        fn write_text(&mut self, text: &str) -> Result<(), String> {
            self.written.push(text.to_string());
            Ok(())
        }
    }

    struct FailOutput;

    impl CliOutput for FailOutput {
        fn write_text(&mut self, _text: &str) -> Result<(), String> {
            Err("output unavailable".to_string())
        }
    }

    struct FakeFeedback {
        copied_count: usize,
        last_failure: Option<String>,
    }

    impl FakeFeedback {
        fn new() -> Self {
            Self {
                copied_count: 0,
                last_failure: None,
            }
        }
    }

    impl QuickOcrFeedback for FakeFeedback {
        fn copied(&mut self) -> Result<(), String> {
            self.copied_count += 1;
            Ok(())
        }
        fn failed(&mut self, message: &str) -> Result<(), String> {
            self.last_failure = Some(message.to_string());
            Ok(())
        }
    }

    #[test]
    fn complete_cli_success_writes_text_with_newline() {
        let items = vec![item(0, "hello", rect(10.0, 10.0, 50.0, 12.0))];
        let mut clipboard = FakeClipboard::new();
        let mut output = FakeOutput::new();
        let mut feedback = FakeFeedback::new();
        let result =
            complete_cli_with(items, &mut clipboard, &mut output, &mut feedback, false).unwrap();
        assert_eq!(result, "hello");
        assert_eq!(clipboard.written, vec!["hello"]);
        assert_eq!(output.written, vec!["hello\n"]);
    }

    #[test]
    fn complete_cli_direct_mode_makes_no_feedback_calls() {
        let items = vec![item(0, "hello", rect(10.0, 10.0, 50.0, 12.0))];
        let mut clipboard = FakeClipboard::new();
        let mut output = FakeOutput::new();
        let mut feedback = FakeFeedback::new();
        let _ = complete_cli_with(items, &mut clipboard, &mut output, &mut feedback, false);
        assert_eq!(feedback.copied_count, 0);
    }

    #[test]
    fn complete_cli_daemon_mode_calls_copied_once() {
        let items = vec![item(0, "hello", rect(10.0, 10.0, 50.0, 12.0))];
        let mut clipboard = FakeClipboard::new();
        let mut output = FakeOutput::new();
        let mut feedback = FakeFeedback::new();
        let _ = complete_cli_with(items, &mut clipboard, &mut output, &mut feedback, true);
        assert_eq!(feedback.copied_count, 1);
    }

    #[test]
    fn complete_cli_ocr_failure_writes_no_stdout() {
        let mut clipboard = FakeClipboard::new();
        let mut output = FakeOutput::new();
        let mut feedback = FakeFeedback::new();
        let err = complete_cli_with(vec![], &mut clipboard, &mut output, &mut feedback, true)
            .unwrap_err();
        assert!(matches!(
            err,
            QuickOcrError::Ocr(ProductOcrError::EmptyResult)
        ));
        assert!(output.written.is_empty());
        assert!(clipboard.written.is_empty());
    }

    #[test]
    fn complete_cli_output_failure_returns_error_after_clipboard() {
        let items = vec![item(0, "hello", rect(10.0, 10.0, 50.0, 12.0))];
        let mut clipboard = FakeClipboard::new();
        let mut output = FailOutput;
        let mut feedback = FakeFeedback::new();
        let err = complete_cli_with(items, &mut clipboard, &mut output, &mut feedback, false)
            .unwrap_err();
        assert!(
            matches!(err, QuickOcrError::Clipboard(_)),
            "output failure should be Clipboard error: {err:?}"
        );
        assert_eq!(clipboard.written, vec!["hello"]);
    }

    #[test]
    fn complete_cli_ocr_failure_does_not_call_success_feedback() {
        let mut clipboard = FakeClipboard::new();
        let mut output = FakeOutput::new();
        let mut feedback = FakeFeedback::new();
        let _ = complete_cli_with(vec![], &mut clipboard, &mut output, &mut feedback, true);
        assert_eq!(feedback.copied_count, 0);
    }
}
