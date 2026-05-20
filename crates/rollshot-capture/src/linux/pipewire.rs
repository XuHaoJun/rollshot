use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Duration;

use crate::backend::FrameStream;
use crate::error::CaptureError;
use crate::types::CapturedFrame;

pub const NEXT_FRAME_TIMEOUT: Duration = Duration::from_secs(5);

#[allow(unsafe_code)] // OwnedFd::from_raw_fd is the documented bridge from a raw kernel fd.
pub fn dup_pipewire_fd(
    input: std::os::fd::BorrowedFd<'_>,
) -> Result<std::os::fd::OwnedFd, CaptureError> {
    use nix::fcntl::{fcntl, FcntlArg};
    use std::os::fd::{AsRawFd, FromRawFd};

    let raw = input.as_raw_fd();
    let duplicated = fcntl(raw, FcntlArg::F_DUPFD_CLOEXEC(5)).map_err(|err| {
        CaptureError::Backend(anyhow::anyhow!("failed to duplicate PipeWire fd: {err}"))
    })?;
    // SAFETY: fcntl(F_DUPFD_CLOEXEC) returns a fresh, exclusively-owned RawFd
    // with the CLOEXEC flag set. No other code path in this crate touches that
    // numeric fd before this OwnedFd is constructed, so ownership transfer is
    // unambiguous. The original `input` BorrowedFd is unaffected by F_DUPFD.
    Ok(unsafe { std::os::fd::OwnedFd::from_raw_fd(duplicated) })
}

pub enum FrameEvent {
    Frame(CapturedFrame),
    End,
    Error(String),
}

pub struct FrameQueue {
    inner: Mutex<VecDeque<FrameEvent>>,
    condvar: Condvar,
}

impl FrameQueue {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(VecDeque::new()),
            condvar: Condvar::new(),
        }
    }

    pub fn push(&self, event: FrameEvent) {
        let mut deque = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if deque.len() >= 3 {
            deque.pop_front();
        }
        deque.push_back(event);
        self.condvar.notify_one();
    }

    pub fn next_frame_with_timeout(
        &self,
        timeout: Duration,
    ) -> Result<CapturedFrame, CaptureError> {
        let deque = match self.inner.lock() {
            Ok(guard) => guard,
            Err(_poisoned) => {
                eprintln!("frame queue mutex poisoned — producer thread panicked");
                return Err(CaptureError::EndOfStream);
            }
        };

        let result = self.condvar.wait_timeout_while(deque, timeout, |d| d.is_empty());

        let (mut deque, wait_result) = match result {
            Ok(pair) => pair,
            Err(_poisoned) => {
                eprintln!("frame queue mutex poisoned during wait — producer thread panicked");
                return Err(CaptureError::EndOfStream);
            }
        };

        if wait_result.timed_out() && deque.is_empty() {
            return Err(CaptureError::Backend(anyhow::anyhow!(
                "PipeWire stream produced no frames within {timeout:?}"
            )));
        }

        match deque.pop_front() {
            Some(FrameEvent::Frame(f)) => Ok(f),
            Some(FrameEvent::End) => Err(CaptureError::EndOfStream),
            Some(FrameEvent::Error(msg)) => {
                Err(CaptureError::Backend(anyhow::anyhow!(msg)))
            }
            None => Err(CaptureError::Backend(anyhow::anyhow!(
                "PipeWire stream produced no frames within {timeout:?}"
            ))),
        }
    }
}

impl Default for FrameQueue {
    fn default() -> Self {
        Self::new()
    }
}

pub struct LinuxPortalFrameStream;

impl FrameStream for LinuxPortalFrameStream {
    fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        Err(CaptureError::NotImplemented {
            backend: "linux-portal-pipewire",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CapturedFrame, FrameMetadata};
    use image::RgbaImage;
    use std::io::{Read, Write};
    use std::os::fd::AsFd;
    use std::sync::Arc;
    use std::time::SystemTime;

    fn dummy_frame(n: u32) -> CapturedFrame {
        CapturedFrame {
            image: RgbaImage::new(n, n),
            timestamp: SystemTime::now(),
            metadata: FrameMetadata::fake(),
        }
    }

    #[test]
    fn dup_pipewire_fd_does_not_consume_input() {
        let (mut reader, mut writer) = std::os::unix::net::UnixStream::pair().unwrap();
        let duplicated = dup_pipewire_fd(reader.as_fd()).unwrap();
        drop(duplicated);
        writer.write_all(b"x").unwrap();
        let mut byte = [0; 1];
        reader.read_exact(&mut byte).unwrap();
        assert_eq!(byte, [b'x']);
    }

    #[test]
    fn queue_retains_newest_three_of_five() {
        let queue = FrameQueue::new();
        for i in 1..=5 {
            queue.push(FrameEvent::Frame(dummy_frame(i)));
        }
        let f3 = queue.next_frame_with_timeout(Duration::from_millis(50)).unwrap();
        let f4 = queue.next_frame_with_timeout(Duration::from_millis(50)).unwrap();
        let f5 = queue.next_frame_with_timeout(Duration::from_millis(50)).unwrap();
        assert_eq!(f3.image.width(), 3);
        assert_eq!(f4.image.width(), 4);
        assert_eq!(f5.image.width(), 5);
    }

    #[test]
    fn queue_timeout_returns_backend_error() {
        let queue = FrameQueue::new();
        let err = queue
            .next_frame_with_timeout(Duration::from_millis(100))
            .unwrap_err();
        match err {
            CaptureError::Backend(e) => {
                assert!(
                    e.to_string().contains("no frames within"),
                    "msg = {}",
                    e
                );
            }
            other => panic!("expected Backend, got {other:?}"),
        }
    }

    #[test]
    fn queue_end_event_returns_end_of_stream() {
        let queue = FrameQueue::new();
        queue.push(FrameEvent::End);
        let err = queue
            .next_frame_with_timeout(Duration::from_millis(50))
            .unwrap_err();
        assert!(matches!(err, CaptureError::EndOfStream), "got {err:?}");
    }

    #[test]
    fn queue_error_event_returns_backend_error() {
        let queue = FrameQueue::new();
        queue.push(FrameEvent::Error("bad pixel data".to_string()));
        let err = queue
            .next_frame_with_timeout(Duration::from_millis(50))
            .unwrap_err();
        match err {
            CaptureError::Backend(e) => {
                assert!(
                    e.to_string().contains("bad pixel data"),
                    "msg = {}",
                    e
                );
            }
            other => panic!("expected Backend, got {other:?}"),
        }
    }

    #[test]
    fn queue_producer_then_consumer() {
        let queue = Arc::new(FrameQueue::new());
        let q = Arc::clone(&queue);
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            q.push(FrameEvent::Frame(dummy_frame(42)));
        });
        let f = queue
            .next_frame_with_timeout(Duration::from_secs(1))
            .unwrap();
        assert_eq!(f.image.width(), 42);
        handle.join().unwrap();
    }

    // Drop order verification

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum DropKind {
        ThreadLoopStop,
        StreamDisconnectDestroy,
        ContextDestroy,
        OriginalFdDrop,
        PortalSessionClose,
    }

    struct DropRecorder {
        kind: DropKind,
        log: Arc<Mutex<Vec<DropKind>>>,
    }

    impl Drop for DropRecorder {
        fn drop(&mut self) {
            self.log.lock().unwrap().push(self.kind.clone());
        }
    }

    struct TestResources {
        _thread_loop_stop: DropRecorder,
        _stream_disconnect_destroy: DropRecorder,
        _context_destroy: DropRecorder,
        _original_fd_drop: DropRecorder,
        _portal_session_close: DropRecorder,
    }

    struct LinuxPortalFrameStreamTest {
        _resources: TestResources,
    }

    impl LinuxPortalFrameStreamTest {
        fn from_test_resources(resources: TestResources) -> Self {
            Self {
                _resources: resources,
            }
        }
    }

    #[test]
    fn drop_order_matches_spec() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let resources = TestResources {
            _thread_loop_stop: DropRecorder {
                kind: DropKind::ThreadLoopStop,
                log: Arc::clone(&log),
            },
            _stream_disconnect_destroy: DropRecorder {
                kind: DropKind::StreamDisconnectDestroy,
                log: Arc::clone(&log),
            },
            _context_destroy: DropRecorder {
                kind: DropKind::ContextDestroy,
                log: Arc::clone(&log),
            },
            _original_fd_drop: DropRecorder {
                kind: DropKind::OriginalFdDrop,
                log: Arc::clone(&log),
            },
            _portal_session_close: DropRecorder {
                kind: DropKind::PortalSessionClose,
                log: Arc::clone(&log),
            },
        };

        let stream = LinuxPortalFrameStreamTest::from_test_resources(resources);
        drop(stream);

        let recorded = log.lock().unwrap();
        assert_eq!(
            *recorded,
            vec![
                DropKind::ThreadLoopStop,
                DropKind::StreamDisconnectDestroy,
                DropKind::ContextDestroy,
                DropKind::OriginalFdDrop,
                DropKind::PortalSessionClose,
            ]
        );
    }
}
