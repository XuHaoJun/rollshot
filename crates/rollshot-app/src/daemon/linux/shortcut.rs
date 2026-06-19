use crate::daemon::config::Shortcut;
use crate::daemon::core::DaemonEvent;
use std::future::Future;
use std::time::Duration;

const SHORTCUT_ID: &str = "capture-region";
const PORTAL_CLOSE_TIMEOUT: Duration = Duration::from_secs(1);

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
        shortcut: &Shortcut,
    ) -> Result<Self, String> {
        let preferred = preferred_trigger(shortcut);
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
                if let Err(error) = runtime.block_on(run_portal(events, preferred, receiver)) {
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
    preferred: String,
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
        let shortcut = ashpd::desktop::global_shortcuts::NewShortcut::new(
            SHORTCUT_ID,
            "Capture a Rollshot region",
        )
        .preferred_trigger(Some(preferred.as_str()));
        let shortcuts = [shortcut];
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

        if !contains_capture_binding(response.shortcuts().iter().map(|shortcut| shortcut.id())) {
            return Err("portal did not bind capture-region".into());
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
                    if is_capture_shortcut(event.shortcut_id()) {
                        let _ = events.send(DaemonEvent::CaptureRegion);
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
}
