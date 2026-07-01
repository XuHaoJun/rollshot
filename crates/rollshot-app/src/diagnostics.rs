#[allow(dead_code)]
pub(crate) const TARGET_APP: &str = "rollshot::app";
#[allow(dead_code)]
pub(crate) const TARGET_FILTER: &str = "rollshot::app::filter";
#[allow(dead_code)]
pub(crate) const TARGET_OCR_TEXT: &str = "rollshot::app::ocr_text";
#[allow(dead_code)]
pub(crate) const TARGET_SAVE: &str = "rollshot::save";

use std::fs::{File, OpenOptions};
use std::path::Path;
use tracing_appender::non_blocking::{ErrorCounter, WorkerGuard};
use tracing_subscriber::layer::{Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

const DEFAULT_FILTER: &str = "warn";

#[allow(dead_code)]
pub(crate) struct DiagnosticsGuard {
    _file_guard: Option<WorkerGuard>,
    dropped_lines: Option<ErrorCounter>,
}

#[allow(dead_code)]
fn open_log_file(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .map_err(|err| format!("failed to open diagnostic log {}: {err}", path.display()))
}

#[allow(dead_code)]
pub(crate) fn init(
    log_file: Option<&Path>,
    selected: &SelectedFilter,
) -> Result<DiagnosticsGuard, String> {
    let console_filter = EnvFilter::try_new(&selected.accepted)
        .map_err(|err| format!("failed to build diagnostic filter: {err}"))?;
    let console = fmt::layer()
        .compact()
        .with_writer(std::io::stderr)
        .with_filter(console_filter);

    let (file_guard, dropped_lines) = match log_file {
        Some(path) => {
            let file = open_log_file(path)?;
            let (writer, guard) = tracing_appender::non_blocking(file);
            let dropped_lines = writer.error_counter();
            let file_filter = EnvFilter::try_new(&selected.accepted)
                .map_err(|err| format!("failed to build diagnostic filter: {err}"))?;
            let file_layer = fmt::layer()
                .json()
                .with_writer(writer)
                .with_filter(file_filter);
            tracing_subscriber::registry()
                .with(console)
                .with(file_layer)
                .try_init()
                .map_err(|err| format!("failed to initialize diagnostics: {err}"))?;
            (Some(guard), Some(dropped_lines))
        }
        None => {
            tracing_subscriber::registry()
                .with(console)
                .try_init()
                .map_err(|err| format!("failed to initialize diagnostics: {err}"))?;
            (None, None)
        }
    };

    Ok(DiagnosticsGuard {
        _file_guard: file_guard,
        dropped_lines,
    })
}

impl Drop for DiagnosticsGuard {
    fn drop(&mut self) {
        let dropped_lines = self
            .dropped_lines
            .as_ref()
            .map(ErrorCounter::dropped_lines)
            .unwrap_or(0);
        if dropped_lines > 0 {
            tracing::warn!(
                target: TARGET_APP,
                dropped_lines,
                "diagnostic file writer dropped events"
            );
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct SelectedFilter {
    pub(crate) accepted: String,
    pub(crate) ignored: Vec<String>,
}

#[allow(dead_code)]
pub(crate) fn select_filter(raw: Option<&str>) -> SelectedFilter {
    let Some(raw) = raw else {
        return SelectedFilter {
            accepted: DEFAULT_FILTER.to_string(),
            ignored: Vec::new(),
        };
    };
    let mut accepted = Vec::new();
    let mut ignored = Vec::new();

    for part in raw.split(',').filter(|part| !part.is_empty()) {
        if !part.contains(' ') && EnvFilter::try_new(part).is_ok() {
            accepted.push(part);
        } else {
            ignored.push(part.to_string());
        }
    }

    if accepted.is_empty() {
        accepted.push(DEFAULT_FILTER);
    }
    if !ignored.is_empty() {
        accepted.push("rollshot::app::filter=warn");
    }

    SelectedFilter {
        accepted: accepted.join(","),
        ignored,
    }
}

pub(crate) fn classify_app_error(error: &str) -> &'static str {
    let lower = error.to_lowercase();
    if lower.contains("launch") || lower.contains("argument") || lower.contains("payload") {
        "launch"
    } else if lower.contains("capture")
        || lower.contains("backend")
        || lower.contains("portal")
        || lower.contains("pipewire")
        || lower.contains("screencapturekit")
        || lower.contains("sck")
    {
        "capture"
    } else if lower.contains("overlay") || lower.contains("iced") || lower.contains("layer") {
        "overlay"
    } else if lower.contains("save")
        || lower.contains("write")
        || lower.contains("png")
        || lower.contains("image")
    {
        "save"
    } else if lower.contains("workspace") {
        "workspace"
    } else {
        "unknown"
    }
}

#[cfg(test)]
pub(crate) fn capture_test_logs(run: impl FnOnce()) -> String {
    use std::cell::RefCell;
    use std::io::Write;
    use std::sync::{Arc, Mutex, Once};
    use tracing_subscriber::fmt::MakeWriter;

    // tracing caches each callsite's interest process-globally, registered once
    // by whichever thread reaches it first. A scoped `with_default` subscriber
    // does not reliably override that: if a non-capturing test hits a
    // `rollshot::*` callsite first under the default `NoSubscriber`, the callsite
    // is cached as `Interest::never` and stays disabled, so a concurrent capture
    // silently drops that event (observed as a missing "save success" line while
    // "save start" survived).
    //
    // Fix the root cause instead of serializing captures: install one
    // process-global INFO subscriber for the whole test binary so a capturing
    // subscriber is always present when callsites register (never poisoned to
    // `never`), and route each event to a per-thread buffer so a capture sees
    // only its own thread's logs regardless of what runs concurrently.
    thread_local! {
        static ACTIVE: RefCell<Option<Arc<Mutex<Vec<u8>>>>> = const { RefCell::new(None) };
    }

    enum Sink {
        Buffer(Arc<Mutex<Vec<u8>>>),
        Discard,
    }

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if let Sink::Buffer(target) = self {
                target.lock().unwrap().extend_from_slice(buf);
            }
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct PerThreadWriter;

    impl<'a> MakeWriter<'a> for PerThreadWriter {
        type Writer = Sink;

        fn make_writer(&'a self) -> Self::Writer {
            ACTIVE.with(|active| match active.borrow().as_ref() {
                Some(buffer) => Sink::Buffer(Arc::clone(buffer)),
                None => Sink::Discard,
            })
        }
    }

    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_max_level(tracing::Level::INFO)
            .with_writer(PerThreadWriter)
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("install global test subscriber");
    });

    let buffer = Arc::new(Mutex::new(Vec::new()));
    ACTIVE.with(|active| *active.borrow_mut() = Some(Arc::clone(&buffer)));
    run();
    ACTIVE.with(|active| *active.borrow_mut() = None);

    let bytes = buffer.lock().unwrap().clone();
    String::from_utf8(bytes).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_filter_defaults_to_warn() {
        let selected = select_filter(None);
        assert_eq!(selected.accepted, "warn");
        assert!(selected.ignored.is_empty());
    }

    #[test]
    fn valid_directives_are_preserved() {
        let selected = select_filter(Some("warn,rollshot::capture=debug"));
        assert_eq!(selected.accepted, "warn,rollshot::capture=debug");
        assert!(selected.ignored.is_empty());
    }

    #[test]
    fn invalid_directives_are_reported_and_valid_ones_survive() {
        let selected = select_filter(Some("warn,not a directive,rollshot::stitch=trace"));
        assert_eq!(
            selected.accepted,
            "warn,rollshot::stitch=trace,rollshot::app::filter=warn"
        );
        assert_eq!(selected.ignored, vec!["not a directive"]);
    }

    #[test]
    fn all_invalid_directives_fall_back_to_warn() {
        let selected = select_filter(Some("not valid"));
        assert_eq!(selected.accepted, "warn,rollshot::app::filter=warn");
        assert_eq!(selected.ignored, vec!["not valid"]);
    }

    #[test]
    fn open_log_file_truncates_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollshot.jsonl");
        std::fs::write(&path, "old data").unwrap();
        drop(open_log_file(&path).unwrap());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "");
    }

    #[test]
    fn open_log_file_rejects_missing_parent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing").join("rollshot.jsonl");
        assert!(open_log_file(&path).is_err());
    }
}
