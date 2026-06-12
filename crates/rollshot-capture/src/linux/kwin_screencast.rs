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

pub(crate) const TARGET_LINUX_KWIN: &str = "rollshot::capture::linux::kwin";

const MAX_SUPPORTED_VERSION: u32 = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
enum StreamEvent {
    Created(u32),
    Failed(String),
    Closed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum StreamOutcome {
    #[default]
    Pending,
    Created(u32),
    Failed(String),
    Closed,
}

impl StreamOutcome {
    fn apply(self, event: StreamEvent) -> StreamOutcome {
        match (&self, &event) {
            (StreamOutcome::Pending, StreamEvent::Created(id)) => StreamOutcome::Created(*id),
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
}
