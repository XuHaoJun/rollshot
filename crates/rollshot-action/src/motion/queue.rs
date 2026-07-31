use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::frame_store::SharedActionFrame;
use crate::models::Millis;

/// A single frame offered to the motion mailbox.
#[derive(Debug, Clone)]
pub struct MotionFrame {
    pub at_ms: Millis,
    pub image: SharedActionFrame,
}

/// Result of offering a frame to the motion mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionOfferResult {
    /// The frame was queued without evicting anything.
    Queued,
    /// The frame was queued and the oldest frame was evicted to make room.
    ReplacedOldest,
    /// The receiver has been dropped; no more frames can be accepted.
    Disconnected,
}

/// Bounded latest-frame mailbox sender.
///
/// Uses only `try_send`/`try_recv` — no mutex, condition variable,
/// blocking send, or unbounded channel. The sender retains a receiver clone
/// for non-blocking eviction when the mailbox is full.
pub struct MotionFrameSender {
    tx: crossbeam_channel::Sender<MotionFrame>,
    /// Retained receiver clone used for try_recv eviction.
    rx_clone: crossbeam_channel::Receiver<MotionFrame>,
    /// Set to false when the primary receiver is dropped.
    /// Required because the rx_clone keeps the channel technically alive.
    disconnected: Arc<AtomicBool>,
}

impl MotionFrameSender {
    /// Offer a frame. If the mailbox is full, evicts the oldest via
    /// non-blocking try_recv before re-offering.
    pub fn offer(&self, frame: MotionFrame) -> MotionOfferResult {
        // Quick check: primary receiver already gone?
        if !self.disconnected.load(Ordering::Acquire) {
            return MotionOfferResult::Disconnected;
        }
        match self.tx.try_send(frame) {
            Ok(()) => {
                // Check liveness after send to close the race window.
                if self.disconnected.load(Ordering::Acquire) {
                    MotionOfferResult::Queued
                } else {
                    MotionOfferResult::Disconnected
                }
            }
            Err(crossbeam_channel::TrySendError::Full(frame)) => {
                // Evict oldest via the receiver clone (non-blocking).
                let _ = self.rx_clone.try_recv();
                // Re-offer the same frame; should succeed now.
                match self.tx.try_send(frame) {
                    Ok(()) => MotionOfferResult::ReplacedOldest,
                    Err(crossbeam_channel::TrySendError::Disconnected(_)) => {
                        MotionOfferResult::Disconnected
                    }
                    Err(crossbeam_channel::TrySendError::Full(_)) => {
                        // Shouldn't happen with capacity ≥ 1 after eviction.
                        MotionOfferResult::ReplacedOldest
                    }
                }
            }
            Err(crossbeam_channel::TrySendError::Disconnected(_)) => {
                MotionOfferResult::Disconnected
            }
        }
    }
}

/// Bounded latest-frame mailbox receiver.
pub struct MotionFrameReceiver {
    rx: crossbeam_channel::Receiver<MotionFrame>,
    /// Shared liveness flag; set to false on drop.
    disconnected: Arc<AtomicBool>,
}

impl MotionFrameReceiver {
    /// Block until a frame is available. Returns None if disconnected.
    pub fn recv(&self) -> Option<MotionFrame> {
        self.rx.recv().ok()
    }
}

impl Drop for MotionFrameReceiver {
    fn drop(&mut self) {
        // Signal the sender that the primary consumer is gone.
        self.disconnected.store(false, Ordering::Release);
    }
}

/// Create a bounded latest-frame mailbox with the given capacity.
///
/// The sender retains a receiver clone for non-blocking eviction. A shared
/// `AtomicBool` tracks primary-receiver liveness so the sender can report
/// `Disconnected` even though the clone keeps the channel technically open.
pub fn motion_frame_mailbox(
    capacity: usize,
) -> (MotionFrameSender, MotionFrameReceiver) {
    let (tx, rx) = crossbeam_channel::bounded::<MotionFrame>(capacity);
    let rx_clone = rx.clone();
    let disconnected = Arc::new(AtomicBool::new(true));
    (
        MotionFrameSender {
            tx,
            rx_clone,
            disconnected: Arc::clone(&disconnected),
        },
        MotionFrameReceiver {
            rx,
            disconnected,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    fn frame(at_ms: Millis) -> MotionFrame {
        MotionFrame {
            at_ms,
            image: Arc::new(RgbaImage::new(1, 1)),
        }
    }

    #[test]
    fn capacity_two_queues_two_frames() {
        let (sender, _receiver) = motion_frame_mailbox(2);
        assert_eq!(sender.offer(frame(1)), MotionOfferResult::Queued);
        assert_eq!(sender.offer(frame(2)), MotionOfferResult::Queued);
    }

    #[test]
    fn third_frame_replaces_oldest_and_receiver_order_is_correct() {
        let (sender, receiver) = motion_frame_mailbox(2);
        assert_eq!(sender.offer(frame(1)), MotionOfferResult::Queued);
        assert_eq!(sender.offer(frame(2)), MotionOfferResult::Queued);
        assert_eq!(sender.offer(frame(3)), MotionOfferResult::ReplacedOldest);
        // Oldest (ts=1) was evicted; remaining are ts=2, ts=3.
        assert_eq!(receiver.recv().unwrap().at_ms, 2);
        assert_eq!(receiver.recv().unwrap().at_ms, 3);
    }

    #[test]
    fn disconnected_returns_disconnected() {
        let (sender, receiver) = motion_frame_mailbox(2);
        drop(receiver);
        assert_eq!(sender.offer(frame(1)), MotionOfferResult::Disconnected);
    }

    #[test]
    fn producer_does_not_block_when_receiver_stalled() {
        let (sender, _receiver) = motion_frame_mailbox(2);
        // The receiver is deliberately not draining. We offer 10,000 frames.
        // If the producer ever blocks, this test will exceed the 1-second deadline.
        let deadline = Duration::from_secs(1);
        let start = std::time::Instant::now();
        for i in 0..10_000 {
            sender.offer(frame(i));
        }
        assert!(
            start.elapsed() < deadline,
            "Producer blocked: took {:?} (deadline {:?})",
            start.elapsed(),
            deadline,
        );
    }
}
