use rollshot_capture::Workflow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspacePhase {
    Selecting,
    Selected,
    ScrollingCapture,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum WorkspaceEffect {
    None,
    ActivateWorkflow(Workflow),
    StartScrolling,
    FinishScrolling,
    FinishRegion,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CropRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToolbarPosition {
    Automatic,
    Manual(CropRect),
}

pub struct WorkspaceState {
    phase: WorkspacePhase,
    active_workflow: Workflow,
    toolbar_position: ToolbarPosition,
    auto_hide: ActivityAutoHide,
    crop_valid: bool,
}

impl WorkspaceState {
    pub fn new(workflow: Workflow) -> Self {
        Self {
            phase: WorkspacePhase::Selecting,
            active_workflow: workflow,
            toolbar_position: ToolbarPosition::Automatic,
            auto_hide: ActivityAutoHide::new(std::time::Duration::from_millis(500)),
            crop_valid: false,
        }
    }

    pub fn phase(&self) -> WorkspacePhase {
        self.phase
    }

    #[allow(dead_code)]
    pub fn active_workflow(&self) -> Workflow {
        self.active_workflow
    }

    #[allow(dead_code)]
    pub fn toolbar_position(&self) -> ToolbarPosition {
        self.toolbar_position
    }

    pub fn set_crop(&mut self, crop: Option<CropRect>) {
        self.crop_valid = crop.is_some_and(|c| c.width > 0.0 && c.height > 0.0);
        self.toolbar_position = ToolbarPosition::Automatic;
    }

    pub fn complete_selection(&mut self) -> WorkspaceEffect {
        if !self.crop_valid {
            return WorkspaceEffect::None;
        }
        self.phase = WorkspacePhase::Selected;
        WorkspaceEffect::None
    }

    pub fn activate_workflow(&mut self, workflow: Workflow) -> WorkspaceEffect {
        self.active_workflow = workflow;
        self.phase = if self.crop_valid {
            WorkspacePhase::Selected
        } else {
            WorkspacePhase::Selecting
        };
        WorkspaceEffect::ActivateWorkflow(workflow)
    }

    pub fn begin_scrolling(&mut self) {
        self.phase = WorkspacePhase::ScrollingCapture;
        self.auto_hide.accepted_frame(std::time::Instant::now());
    }

    pub fn finish_scrolling(&mut self) -> WorkspaceEffect {
        WorkspaceEffect::FinishScrolling
    }

    pub fn finish_region(&mut self) -> WorkspaceEffect {
        WorkspaceEffect::FinishRegion
    }

    pub fn cancel(&mut self) -> WorkspaceEffect {
        self.phase = WorkspacePhase::Selecting;
        self.crop_valid = false;
        WorkspaceEffect::Cancel
    }

    pub fn auto_hide(&self) -> &ActivityAutoHide {
        &self.auto_hide
    }

    pub fn auto_hide_mut(&mut self) -> &mut ActivityAutoHide {
        &mut self.auto_hide
    }
}

pub struct ActivityAutoHide {
    idle_duration: std::time::Duration,
    last_activity: Option<std::time::Instant>,
    interacting: bool,
}

impl ActivityAutoHide {
    pub fn new(idle_duration: std::time::Duration) -> Self {
        Self::new_at(idle_duration, std::time::Instant::now())
    }

    pub fn new_at(idle_duration: std::time::Duration, now: std::time::Instant) -> Self {
        Self {
            idle_duration,
            last_activity: Some(now),
            interacting: false,
        }
    }

    pub fn accepted_frame(&mut self, now: std::time::Instant) {
        self.last_activity = Some(now);
    }

    pub fn set_interacting(&mut self, interacting: bool) {
        self.interacting = interacting;
    }

    pub fn visible(&self, now: std::time::Instant) -> bool {
        if self.interacting {
            return true;
        }
        match self.last_activity {
            Some(last) => now.duration_since(last) >= self.idle_duration,
            None => false,
        }
    }
}

impl From<CropRect> for rollshot_overlay_core::chrome_placement::Rect {
    fn from(value: CropRect) -> Self {
        Self::new(value.x, value.y, value.width, value.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn valid_crop() -> Option<CropRect> {
        Some(CropRect {
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 80.0,
        })
    }

    #[test]
    fn region_release_enters_selected_instead_of_finishing() {
        let mut state = WorkspaceState::new(Workflow::Screenshot);
        state.set_crop(valid_crop());
        assert_eq!(state.complete_selection(), WorkspaceEffect::None);
        assert_eq!(state.phase(), WorkspacePhase::Selected);
    }

    #[test]
    fn accepted_activity_hides_no_space_chrome_until_idle_deadline() {
        let now = Instant::now();
        let mut visibility = ActivityAutoHide::new(Duration::from_millis(500));
        visibility.accepted_frame(now);
        assert!(!visibility.visible(now + Duration::from_millis(499)));
        assert!(visibility.visible(now + Duration::from_millis(500)));
    }

    #[test]
    fn toolbar_interaction_keeps_auto_hide_visible() {
        let now = Instant::now();
        let mut visibility = ActivityAutoHide::new(Duration::from_millis(500));
        visibility.accepted_frame(now);
        visibility.set_interacting(true);
        assert!(visibility.visible(now));
    }

    #[test]
    fn no_accepted_activity_reveals_auto_hide_after_idle_deadline() {
        let now = Instant::now();
        let visibility = ActivityAutoHide::new_at(Duration::from_millis(500), now);

        assert!(!visibility.visible(now + Duration::from_millis(499)));
        assert!(visibility.visible(now + Duration::from_millis(500)));
    }

    #[test]
    fn switching_modes_requests_new_workflow_resources() {
        let mut state = WorkspaceState::new(Workflow::Screenshot);
        state.set_crop(valid_crop());
        state.complete_selection();
        assert_eq!(
            state.activate_workflow(Workflow::Scrolling),
            WorkspaceEffect::ActivateWorkflow(Workflow::Scrolling)
        );
        assert_eq!(state.phase(), WorkspacePhase::Selected);
    }

    #[test]
    fn cancel_resets_phase_to_selecting() {
        let mut state = WorkspaceState::new(Workflow::Screenshot);
        state.set_crop(valid_crop());
        state.complete_selection();
        assert_eq!(state.phase(), WorkspacePhase::Selected);
        assert_eq!(state.cancel(), WorkspaceEffect::Cancel);
        assert_eq!(state.phase(), WorkspacePhase::Selecting);
    }

    #[test]
    fn capture_lifecycle_never_leaves_capture_phases() {
        // After Task 1 the overlay is capture-only: finishing a capture must
        // not transition the workspace into any result-review state. Drive a
        // full scrolling lifecycle and assert every phase stays among the
        // three capture phases.
        let is_capture_phase = |p: WorkspacePhase| {
            matches!(
                p,
                WorkspacePhase::Selecting
                    | WorkspacePhase::Selected
                    | WorkspacePhase::ScrollingCapture
            )
        };

        let mut state = WorkspaceState::new(Workflow::Scrolling);
        assert_eq!(state.phase(), WorkspacePhase::Selecting);
        assert!(is_capture_phase(state.phase()));

        state.set_crop(valid_crop());
        state.complete_selection();
        assert_eq!(state.phase(), WorkspacePhase::Selected);
        assert!(is_capture_phase(state.phase()));

        state.begin_scrolling();
        assert_eq!(state.phase(), WorkspacePhase::ScrollingCapture);
        assert!(is_capture_phase(state.phase()));

        // Finishing the capture returns an effect but keeps the phase in the
        // capture set rather than advancing to a removed review phase.
        state.finish_scrolling();
        assert_eq!(state.phase(), WorkspacePhase::ScrollingCapture);
        assert!(is_capture_phase(state.phase()));

        // A region finish behaves the same way from Selected.
        let mut shot = WorkspaceState::new(Workflow::Screenshot);
        shot.set_crop(valid_crop());
        shot.complete_selection();
        shot.finish_region();
        assert_eq!(shot.phase(), WorkspacePhase::Selected);
        assert!(is_capture_phase(shot.phase()));
    }

    #[test]
    fn finish_scrolling_requests_finalization() {
        let mut state = WorkspaceState::new(Workflow::Scrolling);
        state.set_crop(valid_crop());
        state.complete_selection();
        state.begin_scrolling();
        assert_eq!(state.finish_scrolling(), WorkspaceEffect::FinishScrolling);
        assert_eq!(state.phase(), WorkspacePhase::ScrollingCapture);
    }
}
