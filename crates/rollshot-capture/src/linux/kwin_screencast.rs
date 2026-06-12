pub mod protocol {
    pub mod __interfaces {
        use wayland_backend;
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/zkde-screencast-unstable-v1.xml");
    }

    use wayland_client;
    use self::__interfaces::*;
    use wayland_client::protocol::*;
    wayland_scanner::generate_client_code!("protocols/zkde-screencast-unstable-v1.xml");
}

#[allow(unused_imports)]
use crate::diagnostics::TARGET_LINUX_KWIN;

#[allow(dead_code)]
const MAX_SUPPORTED_VERSION: u32 = 6;

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
    Serial { hi: u32, lo: u32 },
    Failed(String),
    Closed,
}

impl StreamOutcome {
    #[allow(dead_code)]
    fn apply(self, event: StreamEvent) -> StreamOutcome {
        match (&self, &event) {
            (StreamOutcome::Pending, StreamEvent::Created(id)) => StreamOutcome::Created(*id),
            (StreamOutcome::Pending, StreamEvent::Serial { hi, lo }) => StreamOutcome::Serial { hi: *hi, lo: *lo },
            (StreamOutcome::Pending, StreamEvent::Failed(msg)) => StreamOutcome::Failed(msg.clone()),
            (StreamOutcome::Pending, StreamEvent::Closed) => StreamOutcome::Closed,
            _ => self,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_version_supports_output_streaming() {
        assert!(MAX_SUPPORTED_VERSION >= 1);
    }

    #[test]
    fn created_event_produces_node_id() {
        let state = StreamOutcome::default();
        assert_eq!(state.apply(StreamEvent::Created(42)), StreamOutcome::Created(42));
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
}
