//! The editable, reviewable guide model. Holds ordered steps and supports the
//! P0 workspace operations: rename, delete (with renumbering), and replacing a
//! step's keyframe with one of its retained nearby frames. UI lives in the app;
//! this is the headless model it drives.

use crate::models::{default_title, CandidateStep, FrameId, GuideStep};

pub const DEFAULT_GUIDE_TITLE: &str = "Action Guide";

#[derive(Clone, Debug, PartialEq)]
pub struct Guide {
    title: String,
    steps: Vec<GuideStep>,
}

impl Guide {
    /// Build a guide from detector candidates, assigning 1-based order and
    /// deterministic default titles.
    pub fn from_reviewed_steps(title: String, steps: Vec<GuideStep>) -> Result<Self, &'static str> {
        if steps.is_empty() {
            return Err("empty_guide");
        }
        if steps
            .iter()
            .enumerate()
            .any(|(offset, step)| step.index != offset + 1)
        {
            return Err("invalid_step_order");
        }
        let mut sources = std::collections::BTreeSet::new();
        if steps
            .iter()
            .any(|step| step.source == 0 || !sources.insert(step.source))
        {
            return Err("invalid_step_source");
        }
        Ok(Self { title, steps })
    }

    pub fn from_candidates(candidates: Vec<CandidateStep>) -> Self {
        let steps = candidates
            .into_iter()
            .enumerate()
            .map(|(i, c)| GuideStep {
                index: i + 1,
                title: default_title(c.kind).to_string(),
                caption: String::new(),
                kind: c.kind,
                reason: c.reason,
                at_ms: c.at_ms,
                keyframe: c.keyframe,
                nearby: c.nearby,
                source: c.id,
            })
            .collect();
        Self {
            title: DEFAULT_GUIDE_TITLE.to_string(),
            steps,
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn effective_title(&self) -> &str {
        let trimmed = self.title.trim();
        if trimmed.is_empty() {
            DEFAULT_GUIDE_TITLE
        } else {
            trimmed
        }
    }

    pub fn steps(&self) -> &[GuideStep] {
        &self.steps
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Set a step's title. Returns false if `index` is unknown.
    pub fn rename(&mut self, index: usize, title: String) -> bool {
        match self.steps.iter_mut().find(|s| s.index == index) {
            Some(step) => {
                step.title = title;
                true
            }
            None => false,
        }
    }

    /// Set a step's title and caption together when accepting a proposal.
    /// Returns false if no step with this index exists.
    pub fn set_title_and_caption(&mut self, index: usize, title: String, caption: String) -> bool {
        let Some(step) = self.steps.iter_mut().find(|s| s.index == index) else {
            return false;
        };
        step.title = title;
        step.caption = caption;
        true
    }

    /// Set a step's optional Storyboard/Issue Pack caption. Returns false if
    /// `index` is unknown.
    pub fn set_caption(&mut self, index: usize, caption: String) -> bool {
        match self.steps.iter_mut().find(|s| s.index == index) {
            Some(step) => {
                step.caption = caption;
                true
            }
            None => false,
        }
    }

    /// Delete a step and renumber the remainder. Returns false if `index` is
    /// unknown.
    pub fn delete(&mut self, index: usize) -> bool {
        let before = self.steps.len();
        self.steps.retain(|s| s.index != index);
        if self.steps.len() == before {
            return false;
        }
        for (i, step) in self.steps.iter_mut().enumerate() {
            step.index = i + 1;
        }
        true
    }

    /// Replace a step's keyframe with `frame`, which must be in that step's
    /// nearby strip. Returns false if the index is unknown or `frame` is not a
    /// retained nearby frame.
    pub fn replace_keyframe(&mut self, index: usize, frame: FrameId) -> bool {
        match self.steps.iter_mut().find(|s| s.index == index) {
            Some(step) if step.nearby.contains(&frame) => {
                step.keyframe = frame;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CandidateKind, CandidateStep, DetectReason};

    fn cand(id: u64, kind: CandidateKind, keyframe: u64, nearby: Vec<u64>) -> CandidateStep {
        CandidateStep {
            id,
            kind,
            reason: DetectReason::VisualChange,
            at_ms: id * 100,
            keyframe,
            nearby,
        }
    }

    fn reviewed_step(index: usize, source: u64, keyframe: u64) -> GuideStep {
        GuideStep {
            index,
            title: format!("Step {index}"),
            caption: String::new(),
            kind: CandidateKind::Click,
            reason: DetectReason::VisualChange,
            at_ms: 0,
            keyframe,
            nearby: vec![keyframe],
            source,
        }
    }

    #[test]
    fn from_candidates_numbers_steps_and_applies_default_titles() {
        let g = Guide::from_candidates(vec![
            cand(0, CandidateKind::Click, 5, vec![4, 5, 6]),
            cand(1, CandidateKind::Scroll, 12, vec![11, 12, 13]),
        ]);
        assert_eq!(g.steps()[0].index, 1);
        assert_eq!(g.steps()[0].title, "Click");
        assert_eq!(g.steps()[1].index, 2);
        assert_eq!(g.steps()[1].title, "Scroll");
        assert_eq!(g.steps()[0].source, 0);
    }

    #[test]
    fn delete_renumbers_remaining_steps() {
        let mut g = Guide::from_candidates(vec![
            cand(0, CandidateKind::Click, 5, vec![5]),
            cand(1, CandidateKind::Scroll, 12, vec![12]),
            cand(2, CandidateKind::UiChanged, 20, vec![20]),
        ]);
        assert!(g.delete(2));
        assert_eq!(
            g.steps().iter().map(|s| s.index).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(g.steps()[1].kind, CandidateKind::UiChanged);
        assert!(!g.delete(99));
    }

    #[test]
    fn rename_persists_and_unknown_index_is_rejected() {
        let mut g = Guide::from_candidates(vec![cand(0, CandidateKind::Click, 5, vec![5])]);
        assert!(g.rename(1, "Open Preferences".to_string()));
        assert_eq!(g.steps()[0].title, "Open Preferences");
        assert!(!g.rename(99, "x".to_string()));
    }

    #[test]
    fn replace_keyframe_only_accepts_a_nearby_frame() {
        let mut g = Guide::from_candidates(vec![cand(0, CandidateKind::Click, 5, vec![4, 5, 6])]);
        assert!(g.replace_keyframe(1, 6));
        assert_eq!(g.steps()[0].keyframe, 6);
        assert!(
            !g.replace_keyframe(1, 99),
            "frame not in nearby strip is rejected"
        );
        assert_eq!(g.steps()[0].keyframe, 6);
    }

    #[test]
    fn from_candidates_initializes_empty_captions() {
        let g = Guide::from_candidates(vec![cand(0, CandidateKind::Click, 5, vec![5])]);
        assert_eq!(g.steps()[0].caption, "");
    }

    #[test]
    fn set_caption_persists_and_unknown_index_is_rejected() {
        let mut g = Guide::from_candidates(vec![cand(0, CandidateKind::Click, 5, vec![5])]);
        assert!(g.set_caption(1, "Settings close but the value is not saved.".to_string()));
        assert_eq!(
            g.steps()[0].caption,
            "Settings close but the value is not saved."
        );
        assert!(!g.set_caption(99, "ignored".to_string()));
    }

    #[test]
    fn replace_keyframe_preserves_caption() {
        let mut g = Guide::from_candidates(vec![cand(0, CandidateKind::Click, 5, vec![4, 5, 6])]);
        assert!(g.set_caption(1, "The save action loses state.".to_string()));
        assert!(g.replace_keyframe(1, 6));
        assert_eq!(g.steps()[0].caption, "The save action loses state.");
        assert_eq!(g.steps()[0].keyframe, 6);
    }

    #[test]
    fn guide_title_is_editable_with_export_fallback() {
        let mut guide = Guide::from_candidates(vec![cand(0, CandidateKind::Click, 5, vec![5])]);
        assert_eq!(guide.title(), "Action Guide");
        guide.set_title("  Checkout failure  ".to_string());
        assert_eq!(guide.title(), "  Checkout failure  ");
        assert_eq!(guide.effective_title(), "Checkout failure");
        guide.set_title("   ".to_string());
        assert_eq!(guide.effective_title(), "Action Guide");
    }

    #[test]
    fn from_reviewed_steps_accepts_valid_two_step_guide() {
        let g = Guide::from_reviewed_steps(
            "Test".into(),
            vec![reviewed_step(1, 1, 5), reviewed_step(2, 2, 10)],
        )
        .expect("valid guide");
        assert_eq!(g.title(), "Test");
        assert_eq!(g.steps().len(), 2);
        assert_eq!(g.steps()[0].source, 1);
        assert_eq!(g.steps()[1].source, 2);
    }

    #[test]
    fn from_reviewed_steps_rejects_empty() {
        assert_eq!(
            Guide::from_reviewed_steps("T".into(), vec![]),
            Err("empty_guide")
        );
    }

    #[test]
    fn from_reviewed_steps_rejects_non_contiguous_order() {
        let steps = vec![reviewed_step(1, 1, 5), reviewed_step(3, 2, 10)];
        assert_eq!(
            Guide::from_reviewed_steps("T".into(), steps),
            Err("invalid_step_order")
        );
    }

    #[test]
    fn from_reviewed_steps_rejects_zero_source() {
        let steps = vec![reviewed_step(1, 0, 5)];
        assert_eq!(
            Guide::from_reviewed_steps("T".into(), steps),
            Err("invalid_step_source")
        );
    }

    #[test]
    fn from_reviewed_steps_rejects_duplicate_source() {
        let steps = vec![reviewed_step(1, 1, 5), reviewed_step(2, 1, 10)];
        assert_eq!(
            Guide::from_reviewed_steps("T".into(), steps),
            Err("invalid_step_source")
        );
    }
}
