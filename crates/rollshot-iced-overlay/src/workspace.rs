use rollshot_capture::CaptureMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspacePhase {
    Selecting,
    Selected,
    ScrollingCapture,
    ResultReview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputAction {
    Save,
    Copy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum WorkspaceEffect {
    None,
    ActivateMode(CaptureMode),
    StartScrolling,
    StopScrolling { discard: bool },
    FinalizeScrolling { output: Option<OutputAction> },
    PrepareScreenshot { output: Option<OutputAction> },
    PerformOutput(OutputAction),
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
    active_mode: CaptureMode,
    toolbar_position: ToolbarPosition,
    auto_hide: ActivityAutoHide,
    crop_valid: bool,
}

impl WorkspaceState {
    pub fn new(mode: CaptureMode) -> Self {
        Self {
            phase: WorkspacePhase::Selecting,
            active_mode: mode,
            toolbar_position: ToolbarPosition::Automatic,
            auto_hide: ActivityAutoHide::new(std::time::Duration::from_millis(500)),
            crop_valid: false,
        }
    }

    pub fn phase(&self) -> WorkspacePhase {
        self.phase
    }

    #[allow(dead_code)]
    pub fn active_mode(&self) -> CaptureMode {
        self.active_mode
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

    pub fn activate_mode(&mut self, mode: CaptureMode) -> WorkspaceEffect {
        self.active_mode = mode;
        self.phase = WorkspacePhase::Selecting;
        self.crop_valid = false;
        WorkspaceEffect::ActivateMode(mode)
    }

    pub fn begin_scrolling(&mut self) {
        self.phase = WorkspacePhase::ScrollingCapture;
    }

    pub fn finish_scrolling(&mut self, output: Option<OutputAction>) -> WorkspaceEffect {
        self.phase = WorkspacePhase::ResultReview;
        WorkspaceEffect::FinalizeScrolling { output }
    }

    pub fn prepare_screenshot(&mut self, output: Option<OutputAction>) -> WorkspaceEffect {
        self.phase = WorkspacePhase::ResultReview;
        WorkspaceEffect::PrepareScreenshot { output }
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
        Self {
            idle_duration,
            last_activity: None,
            interacting: false,
        }
    }

    pub fn accepted_frame(&mut self, now: std::time::Instant) {
        self.last_activity = Some(now);
    }

    #[allow(dead_code)]
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
    fn screenshot_release_enters_selected_instead_of_finishing() {
        let mut state = WorkspaceState::new(CaptureMode::Screenshot);
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
    fn switching_modes_requests_new_workflow_resources() {
        let mut state = WorkspaceState::new(CaptureMode::Screenshot);
        state.set_crop(valid_crop());
        state.complete_selection();
        assert_eq!(
            state.activate_mode(CaptureMode::Scrolling),
            WorkspaceEffect::ActivateMode(CaptureMode::Scrolling)
        );
    }

    #[test]
    fn cancel_resets_phase_to_selecting() {
        let mut state = WorkspaceState::new(CaptureMode::Screenshot);
        state.set_crop(valid_crop());
        state.complete_selection();
        assert_eq!(state.phase(), WorkspacePhase::Selected);
        assert_eq!(state.cancel(), WorkspaceEffect::Cancel);
        assert_eq!(state.phase(), WorkspacePhase::Selecting);
    }

    #[test]
    fn finish_scrolling_enters_result_review() {
        let mut state = WorkspaceState::new(CaptureMode::Scrolling);
        state.set_crop(valid_crop());
        state.complete_selection();
        state.begin_scrolling();
        assert_eq!(
            state.finish_scrolling(None),
            WorkspaceEffect::FinalizeScrolling { output: None }
        );
        assert_eq!(state.phase(), WorkspacePhase::ResultReview);
    }
}
