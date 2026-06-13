pub mod protocol {
    pub mod __interfaces {
        use wayland_backend;
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/zkde-screencast-unstable-v1.xml");
    }

    use self::__interfaces::*;
    use wayland_client;
    use wayland_client::protocol::*;
    wayland_scanner::generate_client_code!("protocols/zkde-screencast-unstable-v1.xml");
}

use crate::diagnostics::TARGET_LINUX_KWIN;
use crate::error::CaptureError;

#[allow(dead_code)]
// Cap at v5 to match KDE Spectacle's behavior.  Version 6 deprecates the
// `created` event in favour of `serial`, but the current event-loop
// structure expects `created` to arrive as the success signal.
const MAX_SUPPORTED_VERSION: u32 = 5;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum StreamEvent {
    Created(u32),
    Serial { hi: u32, lo: u32 },
    Failed(String),
    Closed,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum StreamOutcome {
    #[default]
    Pending,
    Created(u32),
    Serial {
        hi: u32,
        lo: u32,
    },
    Failed(String),
    Closed,
}

impl StreamOutcome {
    #[allow(dead_code)]
    fn apply(self, event: StreamEvent) -> StreamOutcome {
        match (&self, &event) {
            (StreamOutcome::Pending, StreamEvent::Created(id)) => StreamOutcome::Created(*id),
            (StreamOutcome::Pending, StreamEvent::Serial { hi, lo }) => {
                StreamOutcome::Serial { hi: *hi, lo: *lo }
            }
            (StreamOutcome::Pending, StreamEvent::Failed(msg)) => {
                StreamOutcome::Failed(msg.clone())
            }
            (StreamOutcome::Pending, StreamEvent::Closed) => StreamOutcome::Closed,
            _ => self,
        }
    }
}

#[derive(Debug)]
pub(crate) struct OutputInfo {
    pub registry_name: u32,
    pub name: Option<String>,
    pub(crate) wl_output: Option<wl_output::WlOutput>,
}

pub(crate) fn select_output<'a>(
    outputs: &'a [OutputInfo],
    target: &str,
) -> Result<&'a OutputInfo, CaptureError> {
    outputs
        .iter()
        .find(|o| o.name.as_deref() == Some(target))
        .ok_or_else(|| CaptureError::Mapping {
            message: format!("Wayland output '{target}' not found"),
        })
}

pub(crate) fn map_stream_failure(reason: &str) -> CaptureError {
    if reason.contains("authorized") || reason.contains("denied") {
        CaptureError::PermissionDenied {
            message: format!("KWin screencast authorization rejected: {reason}"),
        }
    } else {
        CaptureError::Backend(anyhow::anyhow!("KWin screencast failed: {reason}"))
    }
}

pub(crate) fn stream_timeout(stage: &str) -> CaptureError {
    CaptureError::Timeout {
        message: format!("KWin screencast {stage} timed out"),
        duration: STREAM_DEADLINE,
    }
}

use std::os::fd::AsFd;
use wayland_client::protocol::{wl_output, wl_registry};
use wayland_client::{Connection, Dispatch, QueueHandle};

use self::protocol::{zkde_screencast_stream_unstable_v1, zkde_screencast_unstable_v1};

const STREAM_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

const POINTER_HIDDEN: u32 = 1;
const POINTER_EMBEDDED: u32 = 2;

fn poll_timeout_until(now: std::time::Instant, deadline: std::time::Instant) -> u16 {
    let remaining = deadline.saturating_duration_since(now);
    if remaining.is_zero() {
        0
    } else {
        remaining.as_millis().clamp(1, 100) as u16
    }
}

fn dispatch_until_event(
    conn: &Connection,
    event_queue: &mut wayland_client::EventQueue<KwinState>,
    state: &mut KwinState,
    deadline: std::time::Instant,
    stage: &str,
) -> Result<(), CaptureError> {
    loop {
        if event_queue.dispatch_pending(state).map_err(|e| {
            CaptureError::Backend(anyhow::anyhow!("{stage} pending dispatch failed: {e}"))
        })? > 0
        {
            return Ok(());
        }

        conn.flush().map_err(|e| {
            CaptureError::Backend(anyhow::anyhow!("{stage} Wayland flush failed: {e}"))
        })?;

        let timeout = poll_timeout_until(std::time::Instant::now(), deadline);
        if timeout == 0 {
            return Err(stream_timeout(stage));
        }

        let Some(read_guard) = event_queue.prepare_read() else {
            continue;
        };
        let mut poll_fd = nix::poll::PollFd::new(conn.as_fd(), nix::poll::PollFlags::POLLIN);
        match nix::poll::poll(std::slice::from_mut(&mut poll_fd), timeout) {
            Ok(0) => continue,
            Ok(_) => {
                read_guard.read().map_err(|e| {
                    CaptureError::Backend(anyhow::anyhow!("{stage} Wayland read failed: {e}"))
                })?;
            }
            Err(e) => {
                return Err(CaptureError::Backend(anyhow::anyhow!(
                    "{stage} Wayland poll failed: {e}"
                )))
            }
        }
    }
}

pub struct KwinScreencastSession {
    node_id: u32,
    #[allow(dead_code)]
    connection: Connection,
    #[allow(dead_code)]
    stream: zkde_screencast_stream_unstable_v1::ZkdeScreencastStreamUnstableV1,
}

impl KwinScreencastSession {
    pub fn node_id(&self) -> u32 {
        self.node_id
    }
}

pub trait KwinScreencastClient: Send {
    fn start_output(
        &self,
        output_name: &str,
        show_cursor: bool,
    ) -> Result<KwinScreencastSession, CaptureError>;
}

#[derive(Debug)]
struct RegistryName(u32);

struct KwinState {
    outputs: Vec<OutputInfo>,
    screencast: Option<zkde_screencast_unstable_v1::ZkdeScreencastUnstableV1>,
    stream_outcome: StreamOutcome,
}

pub struct RealKwinScreencastClient;

impl Default for RealKwinScreencastClient {
    fn default() -> Self {
        Self
    }
}

impl RealKwinScreencastClient {
    pub fn new() -> Self {
        Self
    }
}

impl KwinScreencastClient for RealKwinScreencastClient {
    fn start_output(
        &self,
        output_name: &str,
        show_cursor: bool,
    ) -> Result<KwinScreencastSession, CaptureError> {
        tracing::debug!(target: TARGET_LINUX_KWIN, "connecting to Wayland display");

        let conn = Connection::connect_to_env().map_err(|e| {
            CaptureError::Backend(anyhow::anyhow!("Wayland connection failed: {e}"))
        })?;

        let mut event_queue = conn.new_event_queue();
        let qh = event_queue.handle();

        let display = conn.display();
        display.get_registry(&qh, ());

        let mut state = KwinState {
            outputs: Vec::new(),
            screencast: None,
            stream_outcome: StreamOutcome::Pending,
        };

        let registry_deadline = std::time::Instant::now() + STREAM_DEADLINE;
        dispatch_until_event(
            &conn,
            &mut event_queue,
            &mut state,
            registry_deadline,
            "registry",
        )?;

        // Dispatch pending output name events
        while event_queue.dispatch_pending(&mut state).unwrap_or(0) > 0 {}

        // The compositor sends wl_output::Name events after we bind the
        // proxies.  Do one more blocking read so those events are received
        // before we attempt to select an output by name.
        if state.outputs.iter().any(|o| o.name.is_none()) {
            dispatch_until_event(
                &conn,
                &mut event_queue,
                &mut state,
                registry_deadline,
                "output name",
            )?;
            while event_queue.dispatch_pending(&mut state).unwrap_or(0) > 0 {}
        }

        let screencast = state
            .screencast
            .as_ref()
            .ok_or_else(|| CaptureError::Unsupported {
                message: "zkde_screencast_unstable_v1 global not available".to_string(),
            })?;

        tracing::debug!(target: TARGET_LINUX_KWIN, version = MAX_SUPPORTED_VERSION, "bound zkde_screencast_unstable_v1");

        let selected = select_output(&state.outputs, output_name)?;
        let registry_name = selected.registry_name;
        tracing::debug!(target: TARGET_LINUX_KWIN, output_name, registry_name, "selected output");

        let wl_output_ref = selected.wl_output.as_ref().ok_or_else(|| {
            CaptureError::Backend(anyhow::anyhow!("wl_output proxy not available"))
        })?;

        let pointer_mode = if show_cursor {
            POINTER_EMBEDDED
        } else {
            POINTER_HIDDEN
        };

        tracing::debug!(target: TARGET_LINUX_KWIN, "requesting stream_output");
        let stream = screencast.stream_output(wl_output_ref, pointer_mode, &qh, ());

        let deadline = std::time::Instant::now() + STREAM_DEADLINE;

        loop {
            if std::time::Instant::now() >= deadline {
                return Err(stream_timeout("stream_output"));
            }

            match &state.stream_outcome {
                StreamOutcome::Created(node_id) => {
                    tracing::info!(target: TARGET_LINUX_KWIN, node_id, "KWin screencast session created");
                    return Ok(KwinScreencastSession {
                        node_id: *node_id,
                        connection: conn,
                        stream,
                    });
                }
                StreamOutcome::Serial { .. } => {
                    return Err(CaptureError::Backend(anyhow::anyhow!(
                        "KWin screencast returned serial without node_id"
                    )));
                }
                StreamOutcome::Failed(reason) => {
                    tracing::warn!(target: TARGET_LINUX_KWIN, error = %reason, "KWin screencast failed");
                    return Err(map_stream_failure(reason));
                }
                StreamOutcome::Closed => {
                    return Err(CaptureError::Backend(anyhow::anyhow!(
                        "KWin screencast stream closed before created"
                    )));
                }
                StreamOutcome::Pending => {}
            }

            dispatch_until_event(
                &conn,
                &mut event_queue,
                &mut state,
                deadline,
                "stream_output",
            )?;

            tracing::debug!(target: TARGET_LINUX_KWIN, outcome = ?state.stream_outcome, "stream event received");
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for KwinState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
            ..
        } = event
        {
            match &interface[..] {
                "wl_output" => {
                    if version < 4 {
                        tracing::debug!(target: TARGET_LINUX_KWIN, version, "skipping wl_output below version 4 (name event unavailable)");
                        return;
                    }
                    let output: wl_output::WlOutput = registry.bind(
                        name,
                        version.min(MAX_SUPPORTED_VERSION),
                        qh,
                        RegistryName(name),
                    );
                    state.outputs.push(OutputInfo {
                        registry_name: name,
                        wl_output: Some(output),
                        name: None,
                    });
                }
                "zkde_screencast_unstable_v1" => {
                    let clamped = version.min(MAX_SUPPORTED_VERSION);
                    let sc: zkde_screencast_unstable_v1::ZkdeScreencastUnstableV1 =
                        registry.bind(name, clamped, qh, ());
                    state.screencast = Some(sc);
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, RegistryName> for KwinState {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        udata: &RegistryName,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event {
            if let Some(info) = state
                .outputs
                .iter_mut()
                .find(|o| o.registry_name == udata.0)
            {
                info.name = Some(name);
            }
        }
    }
}

impl Dispatch<zkde_screencast_unstable_v1::ZkdeScreencastUnstableV1, ()> for KwinState {
    fn event(
        _: &mut Self,
        _: &zkde_screencast_unstable_v1::ZkdeScreencastUnstableV1,
        _: <zkde_screencast_unstable_v1::ZkdeScreencastUnstableV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zkde_screencast_stream_unstable_v1::ZkdeScreencastStreamUnstableV1, ()>
    for KwinState
{
    fn event(
        state: &mut Self,
        _: &zkde_screencast_stream_unstable_v1::ZkdeScreencastStreamUnstableV1,
        event: <zkde_screencast_stream_unstable_v1::ZkdeScreencastStreamUnstableV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let stream_event = match event {
            zkde_screencast_stream_unstable_v1::Event::Created { node } => {
                StreamEvent::Created(node)
            }
            zkde_screencast_stream_unstable_v1::Event::Failed { error } => {
                StreamEvent::Failed(error)
            }
            zkde_screencast_stream_unstable_v1::Event::Closed => StreamEvent::Closed,
            zkde_screencast_stream_unstable_v1::Event::Serial {
                object_serial_hi,
                object_serial_low,
            } => StreamEvent::Serial {
                hi: object_serial_hi,
                lo: object_serial_low,
            },
        };
        state.stream_outcome = state.stream_outcome.clone().apply(stream_event);
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_version_supports_output_streaming() {
        const { assert!(MAX_SUPPORTED_VERSION >= 1) }
    }

    #[test]
    fn created_event_produces_node_id() {
        let state = StreamOutcome::default();
        assert_eq!(
            state.apply(StreamEvent::Created(42)),
            StreamOutcome::Created(42)
        );
    }

    #[test]
    fn failed_event_preserves_message() {
        let state = StreamOutcome::default();
        assert_eq!(
            state.apply(StreamEvent::Failed("denied".to_string())),
            StreamOutcome::Failed("denied".to_string())
        );
    }

    #[test]
    fn serial_event_produces_hi_lo_pair() {
        let state = StreamOutcome::default();
        assert_eq!(
            state.apply(StreamEvent::Serial { hi: 1, lo: 42 }),
            StreamOutcome::Serial { hi: 1, lo: 42 }
        );
    }

    #[test]
    fn matching_output_name_selects_exact_output() {
        let outputs = vec![
            OutputInfo {
                registry_name: 7,
                name: Some("DP-1".into()),
                wl_output: None,
            },
            OutputInfo {
                registry_name: 9,
                name: Some("eDP-1".into()),
                wl_output: None,
            },
        ];
        assert_eq!(select_output(&outputs, "eDP-1").unwrap().registry_name, 9);
    }

    #[test]
    fn missing_output_name_is_mapping_error() {
        let err = select_output(&[], "eDP-1").unwrap_err();
        assert!(matches!(err, CaptureError::Mapping { .. }));
    }

    #[test]
    fn failed_event_maps_to_permission_denied_when_authorization_is_rejected() {
        let err = map_stream_failure("Client is not authorized");
        assert!(matches!(err, CaptureError::PermissionDenied { .. }));
    }

    #[test]
    fn timeout_is_fallback_eligible_capture_timeout() {
        let err = stream_timeout("stream_output");
        assert!(matches!(err, CaptureError::Timeout { .. }));
    }

    #[test]
    fn expired_deadline_has_zero_poll_timeout() {
        let now = std::time::Instant::now();
        assert_eq!(poll_timeout_until(now, now), 0);
    }

    #[test]
    fn future_deadline_caps_poll_timeout() {
        let now = std::time::Instant::now();
        assert_eq!(
            poll_timeout_until(now, now + std::time::Duration::from_secs(10)),
            100
        );
    }
}
