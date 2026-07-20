use crate::detector::CandidateMarker;

use super::MAX_GENERATED_STEPS;
use super::REDUCTION_BUCKETS;

#[derive(Debug)]
pub struct SelectionResult {
    pub candidates: Vec<CandidateMarker>,
    pub reduced: bool,
}

struct ReducedState {
    first: Option<CandidateMarker>,
    latest: Option<CandidateMarker>,
    buckets: [Option<CandidateMarker>; REDUCTION_BUCKETS],
}

pub struct CandidateSelector {
    duration_ms: u64,
    buffer: Vec<CandidateMarker>,
    reduced: Option<ReducedState>,
}

impl CandidateSelector {
    pub fn new(duration_ms: u64) -> Self {
        Self {
            duration_ms,
            buffer: Vec::new(),
            reduced: None,
        }
    }

    pub fn count(&self) -> usize {
        match &self.reduced {
            Some(state) => {
                let mut candidates = Vec::with_capacity(MAX_GENERATED_STEPS);
                candidates.extend(state.first);
                candidates.extend(state.buckets.iter().flatten().copied());
                candidates.extend(state.latest);
                candidates.sort_by_key(|candidate| (candidate.at_ms, candidate.center_id));
                candidates.dedup_by(|a, b| a.at_ms == b.at_ms);
                candidates.len()
            }
            None => self.buffer.len(),
        }
    }

    pub fn push(&mut self, marker: CandidateMarker) {
        if let Some(ref mut state) = self.reduced {
            Self::push_reduced(state, marker, self.duration_ms);
            return;
        }

        if self.buffer.len() < MAX_GENERATED_STEPS {
            self.buffer.push(marker);
            return;
        }

        // Candidate 201: initialize reduced mode by replaying existing buffer.
        let mut state = ReducedState {
            first: None,
            latest: None,
            buckets: [const { None }; REDUCTION_BUCKETS],
        };
        for existing in self.buffer.drain(..) {
            Self::push_reduced(&mut state, existing, self.duration_ms);
        }
        Self::push_reduced(&mut state, marker, self.duration_ms);
        self.reduced = Some(state);
    }

    fn push_reduced(state: &mut ReducedState, marker: CandidateMarker, duration_ms: u64) {
        if state.first.is_none() {
            state.first = Some(marker);
        }
        state.latest = Some(marker);

        let bucket = if duration_ms == 0 {
            0
        } else {
            let raw =
                (u128::from(marker.at_ms) * REDUCTION_BUCKETS as u128) / u128::from(duration_ms);
            // Bounded to [0, REDUCTION_BUCKETS - 1].
            (raw.min(REDUCTION_BUCKETS as u128 - 1)) as usize
        };
        // Replace only with a later candidate.
        if state.buckets[bucket].is_none_or(|existing| marker.at_ms >= existing.at_ms) {
            state.buckets[bucket] = Some(marker);
        }
    }

    pub fn finish(self) -> SelectionResult {
        if let Some(state) = self.reduced {
            return Self::finish_reduced(state);
        }

        let mut candidates = self.buffer;
        candidates.sort_by_key(|a| (a.at_ms, a.center_id));
        candidates.dedup_by(|a, b| a.at_ms == b.at_ms);
        SelectionResult {
            candidates,
            reduced: false,
        }
    }

    fn finish_reduced(state: ReducedState) -> SelectionResult {
        let mut candidates = Vec::new();

        if let Some(first) = state.first {
            candidates.push(first);
        }

        candidates.extend(state.buckets.iter().flatten().copied());

        if let Some(latest) = state.latest {
            candidates.push(latest);
        }

        candidates.sort_by_key(|a| (a.at_ms, a.center_id));
        candidates.dedup_by(|a, b| a.at_ms == b.at_ms);

        if candidates.len() > MAX_GENERATED_STEPS + 2 {
            // first + 198 buckets + latest = 200 max; anything beyond is an invariant violation.
            panic!(
                "selection invariant violated: reduced result has {} candidates, expected <= {}",
                candidates.len(),
                MAX_GENERATED_STEPS + 2
            );
        }

        SelectionResult {
            candidates,
            reduced: true,
        }
    }
}

pub fn evidence_sample_indices(candidate_indices: &[usize], n: usize) -> Vec<usize> {
    let mut indices = Vec::new();
    for &center in candidate_indices {
        for offset in &[0usize, 1] {
            let idx = center.saturating_sub(*offset).min(n.saturating_sub(1));
            indices.push(idx);
        }
        if center + 1 < n {
            indices.push(center + 1);
        }
    }
    indices.sort_unstable();
    indices.dedup();
    indices
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CandidateKind, DetectReason};

    fn marker(center_id: u64, at_ms: u64) -> CandidateMarker {
        CandidateMarker {
            kind: CandidateKind::UiChanged,
            reason: DetectReason::VisualChange,
            at_ms,
            center_id,
        }
    }

    #[test]
    fn two_hundred_candidates_are_not_reduced() {
        let mut selector = CandidateSelector::new(100_000);
        for index in 0..200 {
            selector.push(marker(index, index * 500));
        }
        let result = selector.finish();
        assert_eq!(result.candidates.len(), 200);
        assert!(!result.reduced);
    }

    #[test]
    fn candidate_201_switches_to_full_duration_reduction() {
        let mut selector = CandidateSelector::new(200_000);
        for index in 0..401 {
            selector.push(marker(index, index * 500));
        }
        let result = selector.finish();
        assert!(result.candidates.len() <= MAX_GENERATED_STEPS);
        assert_eq!(result.candidates.first().unwrap().at_ms, 0);
        assert_eq!(result.candidates.last().unwrap().at_ms, 200_000);
        assert!(result
            .candidates
            .windows(2)
            .all(|w| w[0].at_ms < w[1].at_ms));
        assert!(result.reduced);
        assert!(result
            .candidates
            .iter()
            .any(|candidate| candidate.at_ms < 50_000));
        assert!(result
            .candidates
            .iter()
            .any(|candidate| (50_000..150_000).contains(&candidate.at_ms)));
    }

    #[test]
    fn evidence_indices_are_sorted_unique_and_bounded() {
        let indices = evidence_sample_indices(&[0, 4, 9], 10);
        assert_eq!(indices, vec![0, 1, 3, 4, 5, 8, 9]);
        assert!(indices.len() <= 3 * MAX_GENERATED_STEPS);
    }

    #[test]
    fn empty_selector_returns_empty() {
        let selector = CandidateSelector::new(10_000);
        let result = selector.finish();
        assert!(result.candidates.is_empty());
        assert!(!result.reduced);
    }

    #[test]
    fn single_candidate_preserved() {
        let mut selector = CandidateSelector::new(10_000);
        selector.push(marker(42, 5000));
        let result = selector.finish();
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].center_id, 42);
        assert_eq!(result.candidates[0].at_ms, 5000);
        assert!(!result.reduced);
    }

    #[test]
    fn finish_sorts_by_at_ms_then_center_id() {
        let mut selector = CandidateSelector::new(10_000);
        selector.push(marker(2, 500));
        selector.push(marker(1, 200));
        selector.push(marker(3, 700));
        selector.push(marker(0, 1000));
        let result = selector.finish();
        assert_eq!(result.candidates[0].at_ms, 200);
        assert_eq!(result.candidates[0].center_id, 1);
        assert_eq!(result.candidates[1].at_ms, 500);
        assert_eq!(result.candidates[1].center_id, 2);
        assert_eq!(result.candidates[2].at_ms, 700);
        assert_eq!(result.candidates[3].at_ms, 1000);
    }

    #[test]
    fn finish_deduplicates_same_at_ms() {
        let mut selector = CandidateSelector::new(10_000);
        selector.push(marker(1, 500));
        selector.push(marker(2, 500));
        let result = selector.finish();
        assert_eq!(result.candidates.len(), 1);
    }

    #[test]
    fn reduced_mode_preserves_first_and_latest() {
        let mut selector = CandidateSelector::new(100_000);
        for i in 0..250 {
            selector.push(marker(i, i * 400));
        }
        let result = selector.finish();
        assert!(result.reduced);
        assert_eq!(result.candidates.first().unwrap().at_ms, 0);
        assert_eq!(result.candidates.last().unwrap().at_ms, 99_600);
    }

    #[test]
    fn reduced_result_is_sorted_and_deduplicated() {
        let mut selector = CandidateSelector::new(200_000);
        for i in 0..500 {
            selector.push(marker(i, i * 400));
        }
        let result = selector.finish();
        for window in result.candidates.windows(2) {
            assert!(window[0].at_ms <= window[1].at_ms);
            if window[0].at_ms == window[1].at_ms {
                assert!(window[0].center_id < window[1].center_id);
            }
        }
    }

    #[test]
    fn evidence_indices_at_boundaries() {
        let indices = evidence_sample_indices(&[0], 1);
        assert_eq!(indices, vec![0]);
    }

    #[test]
    fn evidence_indices_overflow_clamped() {
        let indices = evidence_sample_indices(&[0, 9], 10);
        assert_eq!(indices, vec![0, 1, 8, 9]);
        assert!(indices.len() <= 3 * MAX_GENERATED_STEPS);
    }

    #[test]
    fn reduced_mode_with_zero_duration() {
        let mut selector = CandidateSelector::new(0);
        for i in 0..250 {
            selector.push(marker(i, 0));
        }
        let result = selector.finish();
        assert!(result.reduced);
        // All go to bucket 0, first and latest preserved.
        assert!(!result.candidates.is_empty());
    }

    #[test]
    fn repeated_push_identical_sequence_produces_identical_result() {
        let run = || {
            let mut selector = CandidateSelector::new(200_000);
            for i in 0..300 {
                selector.push(marker(i, i * 600));
            }
            selector.finish()
        };
        let a = run();
        let b = run();
        assert_eq!(a.candidates.len(), b.candidates.len());
        assert_eq!(a.reduced, b.reduced);
        for (ca, cb) in a.candidates.iter().zip(b.candidates.iter()) {
            assert_eq!(ca.at_ms, cb.at_ms);
            assert_eq!(ca.center_id, cb.center_id);
        }
    }

    #[test]
    fn reduced_count_reports_retained_candidates() {
        let mut selector = CandidateSelector::new(200_000);
        for i in 0..250 {
            selector.push(marker(i, i * 500));
        }
        assert!((1..=MAX_GENERATED_STEPS).contains(&selector.count()));
    }
}
