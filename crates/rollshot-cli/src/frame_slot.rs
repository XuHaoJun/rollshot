use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Duration;

use rollshot_capture::{CaptureError, CapturedFrame};

const DEFAULT_CAPACITY: usize = 8;

struct SlotState {
    frames: VecDeque<CapturedFrame>,
    capacity: usize,
    total_produced: u32,
    end: bool,
    error: Option<String>,
}

pub struct FrameSlot {
    inner: Mutex<SlotState>,
    condvar: Condvar,
}

impl FrameSlot {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(SlotState {
                frames: VecDeque::with_capacity(DEFAULT_CAPACITY),
                capacity: DEFAULT_CAPACITY,
                total_produced: 0,
                end: false,
                error: None,
            }),
            condvar: Condvar::new(),
        }
    }

    pub fn store(&self, frame: CapturedFrame) {
        let mut state = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if state.frames.len() >= state.capacity {
            state.frames.pop_front();
        }
        state.frames.push_back(frame);
        state.total_produced += 1;
        self.condvar.notify_one();
    }

    pub fn signal_end(&self) {
        let mut state = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        state.end = true;
        self.condvar.notify_one();
    }

    pub fn signal_error(&self, msg: String) {
        let mut state = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        state.error = Some(msg);
        self.condvar.notify_one();
    }

    pub fn take_blocking(&self, timeout: Duration) -> Result<CapturedFrame, CaptureError> {
        let state = self.inner.lock().unwrap_or_else(PoisonError::into_inner);

        let (mut state, wait_result) = self
            .condvar
            .wait_timeout_while(state, timeout, |s| {
                s.frames.is_empty() && !s.end && s.error.is_none()
            })
            .unwrap_or_else(PoisonError::into_inner);

        if !state.frames.is_empty() {
            let frame = state.frames.pop_back().unwrap();
            state.frames.clear();
            return Ok(frame);
        }
        if let Some(msg) = state.error.take() {
            return Err(CaptureError::Backend(anyhow::anyhow!(msg)));
        }
        if state.end {
            return Err(CaptureError::EndOfStream);
        }
        if wait_result.timed_out() {
            return Err(CaptureError::Backend(anyhow::anyhow!(
                "no frame within {timeout:?}"
            )));
        }
        Err(CaptureError::EndOfStream)
    }

    pub fn total_produced(&self) -> u32 {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .total_produced
    }
}
