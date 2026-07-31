use crate::models::Millis;

/// Result of pushing a frame timestamp into the CFR scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CfrEmission {
    /// Number of intermediate ticks that must repeat the previous frame
    /// (hold frames for ticks strictly between the last written tick and
    /// this arrival tick, exclusive of the arrival tick itself).
    pub repeat_previous: u64,
    /// Whether this arrival introduces a new image at its tick.
    /// `false` for duplicate or late timestamps (behind the written cursor),
    /// though the scheduler still updates its current visual state.
    pub write_new: bool,
}

/// Constant Frame Rate scheduler: maps variable-rate wall-clock timestamps
/// to a fixed 30 fps tick stream.
///
/// All arithmetic uses `u128` for `at_ms * 30` to avoid overflow.
pub struct CfrScheduler {
    /// Total ticks expected for the session.
    total_ticks: u64,
    /// The current visual frame's tick (None if nothing written yet).
    last_written_tick: Option<u64>,
    /// Count of new frames written.
    written_count: u64,
}

impl CfrScheduler {
    /// Create a new scheduler for a session of `duration_ms` milliseconds.
    /// Total output frames = ceil(duration_ms / 1000.0 * 30).
    pub fn new(duration_ms: Millis) -> Self {
        let total_ticks = if duration_ms == 0 {
            0
        } else {
            (duration_ms as u128 * 30).div_ceil(1000) as u64
        };
        Self {
            total_ticks,
            last_written_tick: None,
            written_count: 0,
        }
    }

    /// Push a frame with arrival timestamp `at_ms`.
    ///
    /// Returns a `CfrEmission` describing how many hold-frames to emit for
    /// prior ticks and whether a new frame should be written at the arrival tick.
    pub fn push(&mut self, at_ms: Millis) -> CfrEmission {
        // Convert arrival time to a tick index using u128 to avoid overflow.
        let arrival_tick = (at_ms as u128 * 30 / 1000) as u64;

        match self.last_written_tick {
            None => {
                // First frame ever. If it arrives at a nonzero tick, emit
                // hold-frames to fill the gap from tick 0 so the output
                // duration is not shortened by late first frames.
                let holds = arrival_tick;
                self.last_written_tick = Some(arrival_tick);
                self.written_count += 1;
                CfrEmission {
                    repeat_previous: holds,
                    write_new: true,
                }
            }
            Some(last_tick) => {
                if arrival_tick <= last_tick {
                    // Duplicate or late: update visual state but don't advance.
                    CfrEmission {
                        repeat_previous: 0,
                        write_new: false,
                    }
                } else {
                    // Normal case: emit holds for ticks between last and arrival.
                    let holds = arrival_tick - last_tick - 1;
                    self.last_written_tick = Some(arrival_tick);
                    self.written_count += 1;
                    CfrEmission {
                        repeat_previous: holds,
                        write_new: true,
                    }
                }
            }
        }
    }

    /// Finalize the session. Returns the number of hold-frames needed to fill
    /// out to `duration_ms`.
    pub fn finish(&mut self, duration_ms: Millis) -> u64 {
        let total_ticks = if duration_ms == 0 {
            0
        } else {
            (duration_ms as u128 * 30).div_ceil(1000) as u64
        };
        self.total_ticks = total_ticks;

        match self.last_written_tick {
            None => 0,
            Some(last_tick) => {
                if total_ticks <= last_tick + 1 {
                    0
                } else {
                    total_ticks - last_tick - 1
                }
            }
        }
    }

    /// Number of frames written so far (new image, not holds).
    pub fn frames_written(&self) -> u64 {
        self.written_count
    }

    /// Session duration in milliseconds, snapped to the nearest CFR boundary.
    pub fn duration_ms(&self) -> u64 {
        self.total_ticks * 1000 / 30
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_finish_returns_zero() {
        let mut scheduler = CfrScheduler::new(0);
        assert_eq!(scheduler.finish(0), 0);
    }

    #[test]
    fn first_frame_at_zero() {
        let mut scheduler = CfrScheduler::new(1000);
        assert_eq!(
            scheduler.push(0),
            CfrEmission {
                repeat_previous: 0,
                write_new: true,
            }
        );
    }

    #[test]
    fn irregular_timestamps_compute_holds() {
        let mut scheduler = CfrScheduler::new(1000);
        // Frame at 0ms → tick 0
        assert_eq!(
            scheduler.push(0),
            CfrEmission {
                repeat_previous: 0,
                write_new: true,
            }
        );
        // Frame at 100ms → tick 3 (100 * 30 / 1000 = 3)
        // Holds: ticks 1, 2 → repeat_previous = 2
        assert_eq!(
            scheduler.push(100),
            CfrEmission {
                repeat_previous: 2,
                write_new: true,
            }
        );
    }

    #[test]
    fn duplicate_timestamp_returns_false() {
        let mut scheduler = CfrScheduler::new(1000);
        // First frame at 100ms → tick 3, fills leading ticks 0-2
        assert_eq!(
            scheduler.push(100),
            CfrEmission {
                repeat_previous: 3,
                write_new: true,
            }
        );
        // Same timestamp again → write_new = false
        assert_eq!(
            scheduler.push(100),
            CfrEmission {
                repeat_previous: 0,
                write_new: false,
            }
        );
    }

    #[test]
    fn late_timestamp_behind_cursor_returns_false() {
        let mut scheduler = CfrScheduler::new(1000);
        // First frame at 200ms → tick 6, fills leading ticks 0-5
        assert_eq!(
            scheduler.push(200),
            CfrEmission {
                repeat_previous: 6,
                write_new: true,
            }
        );
        // Push a timestamp that maps to an earlier tick (100ms → tick 3, last was tick 6)
        assert_eq!(
            scheduler.push(100),
            CfrEmission {
                repeat_previous: 0,
                write_new: false,
            }
        );
    }

    #[test]
    fn over_rate_input_multiple_frames() {
        let mut scheduler = CfrScheduler::new(2000);
        // Frame at 0ms → tick 0
        assert_eq!(
            scheduler.push(0),
            CfrEmission {
                repeat_previous: 0,
                write_new: true,
            }
        );
        // Frame at 33ms → tick 0 (33 * 30 / 1000 = 0) → same tick, false
        assert_eq!(
            scheduler.push(33),
            CfrEmission {
                repeat_previous: 0,
                write_new: false,
            }
        );
        // Frame at 66ms → tick 1 (66 * 30 / 1000 = 1) → holds = 0
        assert_eq!(
            scheduler.push(66),
            CfrEmission {
                repeat_previous: 0,
                write_new: true,
            }
        );
    }

    #[test]
    fn finish_computes_final_holds() {
        let mut scheduler = CfrScheduler::new(1000);
        // Frame at 0ms → tick 0
        assert_eq!(
            scheduler.push(0),
            CfrEmission {
                repeat_previous: 0,
                write_new: true,
            }
        );
        // Frame at 100ms → tick 3, holds = 2
        assert_eq!(
            scheduler.push(100),
            CfrEmission {
                repeat_previous: 2,
                write_new: true,
            }
        );
        // finish(134ms) → total ticks = ceil(134 * 30 / 1000) = ceil(4.02) = 5
        // Last written tick = 3, so remaining holds = 5 - 3 - 1 = 1
        assert_eq!(scheduler.finish(134), 1);
    }

    #[test]
    fn duration_ms_snaps_to_nearest_cfr_boundary() {
        let mut scheduler = CfrScheduler::new(1000);
        scheduler.push(0);
        scheduler.push(100);
        scheduler.finish(134);
        // duration_ms = total_ticks * 1000 / 30 = 5 * 1000 / 30 = 166
        // 166 - 134 = 32 ≤ 34 ✓
        assert!(scheduler.duration_ms().abs_diff(134) <= 34);
    }

    #[test]
    fn finish_with_no_frames_returns_zero() {
        let mut scheduler = CfrScheduler::new(5000);
        assert_eq!(scheduler.finish(5000), 0);
    }
}
