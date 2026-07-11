use crate::daemon::config::Shortcut;
use crate::daemon::core::DaemonEvent;
use std::future::Future;
use std::time::Duration;

const SHORTCUT_ID: &str = "capture-region";
#[cfg(feature = "ocr")]
const TEXT_SHORTCUT_ID: &str = "capture-text";
const PORTAL_CLOSE_TIMEOUT: Duration = Duration::from_secs(1);

pub fn event_for_shortcut(id: &str) -> Option<DaemonEvent> {
    match id {
        SHORTCUT_ID => Some(DaemonEvent::CaptureRegion),
        #[cfg(feature = "ocr")]
        TEXT_SHORTCUT_ID => Some(DaemonEvent::CaptureText),
        _ => None,
    }
}

pub struct ShortcutGuard {
    shutdown: tokio::sync::watch::Sender<bool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

pub fn is_capture_shortcut(id: &str) -> bool {
    id == SHORTCUT_ID
}

pub fn preferred_trigger(shortcut: &Shortcut) -> String {
    shortcut.portal_trigger()
}

pub(super) fn contains_capture_binding<'a>(ids: impl IntoIterator<Item = &'a str>) -> bool {
    ids.into_iter().any(is_capture_shortcut)
}

impl ShortcutGuard {
    pub fn start(
        events: std::sync::mpsc::Sender<DaemonEvent>,
        region_shortcut: &Shortcut,
        #[cfg(feature = "ocr")] text_shortcut: Option<&Shortcut>,
    ) -> Result<Self, String> {
        let preferred_region = preferred_trigger(region_shortcut);
        #[cfg(feature = "ocr")]
        let preferred_text = text_shortcut.map(preferred_trigger);
        let (shutdown, receiver) = tokio::sync::watch::channel(false);
        let thread = std::thread::Builder::new()
            .name("rollshot-global-shortcut".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        tracing::warn!(
                            target: "rollshot::daemon::shortcut",
                            %error,
                            "failed to create global shortcut runtime"
                        );
                        return;
                    }
                };
                if let Err(error) = runtime.block_on(run_portal(
                    events,
                    preferred_region,
                    #[cfg(feature = "ocr")]
                    preferred_text,
                    receiver,
                )) {
                    tracing::warn!(
                        target: "rollshot::daemon::shortcut",
                        %error,
                        "global shortcut unavailable; tray remains active"
                    );
                }
            })
            .map_err(|error| format!("failed to start shortcut thread: {error}"))?;
        Ok(Self {
            shutdown,
            thread: Some(thread),
        })
    }
}

impl Drop for ShortcutGuard {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                tracing::warn!(
                    target: "rollshot::daemon::shortcut",
                    "global shortcut thread panicked during shutdown"
                );
            }
        }
    }
}

async fn cancellable<T, E, F>(
    future: F,
    shutdown: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<Option<T>, String>
where
    E: std::fmt::Display,
    F: Future<Output = Result<T, E>>,
{
    tokio::select! {
        result = future => result
            .map(Some)
            .map_err(|error| error.to_string()),
        changed = shutdown.changed() => {
            let _ = changed;
            Ok(None)
        },
    }
}

async fn run_portal(
    events: std::sync::mpsc::Sender<DaemonEvent>,
    preferred_region: String,
    #[cfg(feature = "ocr")] preferred_text: Option<String>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> Result<(), String> {
    let Some(portal) = cancellable(
        ashpd::desktop::global_shortcuts::GlobalShortcuts::new(),
        &mut shutdown,
    )
    .await?
    else {
        return Ok(());
    };
    let Some(session) = cancellable(portal.create_session(), &mut shutdown).await? else {
        return Ok(());
    };

    let result = async {
        let mut shortcuts = vec![ashpd::desktop::global_shortcuts::NewShortcut::new(
            SHORTCUT_ID,
            "Capture a Rollshot region",
        )
        .preferred_trigger(Some(preferred_region.as_str()))];

        #[cfg(feature = "ocr")]
        if let Some(text_preferred) = &preferred_text {
            shortcuts.push(
                ashpd::desktop::global_shortcuts::NewShortcut::new(
                    TEXT_SHORTCUT_ID,
                    "Capture Rollshot text",
                )
                .preferred_trigger(Some(text_preferred.as_str())),
            );
        }

        let parent = ashpd::WindowIdentifier::default();
        let Some(request) = cancellable(
            portal.bind_shortcuts(&session, &shortcuts, &parent),
            &mut shutdown,
        )
        .await?
        else {
            return Ok(());
        };
        let response = request.response().map_err(|error| error.to_string())?;

        let bound_ids: Vec<&str> = response
            .shortcuts()
            .iter()
            .map(|shortcut| shortcut.id())
            .collect();

        if !contains_capture_binding(bound_ids.iter().copied()) {
            return Err("portal did not bind capture-region".into());
        }

        #[cfg(feature = "ocr")]
        if !bound_ids.contains(&TEXT_SHORTCUT_ID) && preferred_text.is_some() {
            tracing::warn!(
                target: "rollshot::daemon::shortcut",
                "text shortcut not bound by portal; text capture via hotkey degraded"
            );
        }

        let Some(mut activated) = cancellable(portal.receive_activated(), &mut shutdown).await?
        else {
            return Ok(());
        };
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    let _ = changed;
                    return Ok(());
                },
                event = futures_util::StreamExt::next(&mut activated) => {
                    let Some(event) = event else {
                        return Err(
                            "global shortcut activation stream closed".into()
                        );
                    };
                    if let Some(daemon_event) = event_for_shortcut(event.shortcut_id()) {
                        let _ = events.send(daemon_event);
                    }
                }
            }
        }
    }
    .await;

    match tokio::time::timeout(PORTAL_CLOSE_TIMEOUT, session.close()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!(
            target: "rollshot::daemon::shortcut",
            %error,
            "failed to close global shortcut session"
        ),
        Err(_) => tracing::warn!(
            target: "rollshot::daemon::shortcut",
            "timed out closing global shortcut session"
        ),
    };
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_capture_region_id_routes_to_capture() {
        assert!(is_capture_shortcut("capture-region"));
        assert!(!is_capture_shortcut("other"));
    }

    #[test]
    fn preferred_trigger_comes_from_configured_shortcut() {
        let shortcut: Shortcut = "Alt+Shift+6".parse().unwrap();
        assert_eq!(preferred_trigger(&shortcut), "ALT+SHIFT+6");
    }

    #[test]
    fn missing_capture_binding_is_detected() {
        assert!(contains_capture_binding(["capture-region"]));
        assert!(!contains_capture_binding(["other"]));
        assert!(!contains_capture_binding(std::iter::empty::<&str>()));
    }

    #[test]
    fn pending_portal_operation_is_cancelled_by_shutdown() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (shutdown, mut receiver) = tokio::sync::watch::channel(false);
        shutdown.send(true).unwrap();

        let result = runtime
            .block_on(cancellable(
                futures_util::future::pending::<Result<(), &'static str>>(),
                &mut receiver,
            ))
            .unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn event_for_shortcut_routes_capture_region() {
        assert_eq!(
            event_for_shortcut("capture-region"),
            Some(DaemonEvent::CaptureRegion)
        );
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn event_for_shortcut_routes_capture_text() {
        assert_eq!(
            event_for_shortcut("capture-text"),
            Some(DaemonEvent::CaptureText)
        );
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn event_for_shortcut_ignores_unknown_ids() {
        assert_eq!(event_for_shortcut("unknown"), None);
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn region_only_binding_keeps_guard_active() {
        let bound: Vec<&str> = vec!["capture-region"];
        assert!(contains_capture_binding(bound.iter().copied()));
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn empty_binding_detected_as_missing_region() {
        assert!(!contains_capture_binding(std::iter::empty::<&str>()));
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn text_only_binding_detected_as_missing_region() {
        let bound: Vec<&str> = vec!["capture-text"];
        assert!(!contains_capture_binding(bound.iter().copied()));
    }
}
