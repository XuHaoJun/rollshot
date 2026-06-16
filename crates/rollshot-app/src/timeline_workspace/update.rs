use iced::Task;

use super::TimelineWorkspace;

#[derive(Debug, Clone)]
pub enum Message {
    SelectStep(usize),
    TitleChanged(String),
    DeleteStep,
    ReplaceKeyframe(rollshot_action::FrameId),
    DismissMessage,
}

pub fn update(state: &mut TimelineWorkspace, message: Message) -> Task<Message> {
    match message {
        Message::SelectStep(index) => {
            if state.guide.steps().iter().any(|s| s.index == index) {
                state.selected = Some(index);
                state.rebuild_selection_handles();
            }
            Task::none()
        }
        Message::TitleChanged(title) => {
            if let Some(index) = state.selected {
                state.guide.rename(index, title);
            }
            Task::none()
        }
        Message::DeleteStep => {
            if let Some(index) = state.selected {
                if state.guide.delete(index) {
                    let len = state.guide.steps().len();
                    state.selected = if len == 0 { None } else { Some(index.min(len)) };
                    state.rebuild_selection_handles();
                }
            }
            Task::none()
        }
        Message::ReplaceKeyframe(frame) => {
            if let Some(index) = state.selected {
                if state.guide.replace_keyframe(index, frame) {
                    state.rebuild_selection_handles();
                }
            }
            Task::none()
        }
        Message::DismissMessage => {
            state.message = None;
            Task::none()
        }
    }
}

pub fn subscription(_state: &TimelineWorkspace) -> iced::Subscription<Message> {
    iced::Subscription::none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline_workspace::tests::{recording_from_frames, synthetic_recording};
    use crate::timeline_workspace::TimelineWorkspace;
    use rollshot_action::{CaptureRegion, InputCapability, InputSourceKind};

    fn ws(recording: rollshot_action::Recording) -> TimelineWorkspace {
        TimelineWorkspace::new(
            recording,
            CaptureRegion { x: 0, y: 0, width: 32, height: 32 },
            InputCapability::SemanticEvents,
            InputSourceKind::LinuxEvdev,
        )
    }

    #[test]
    fn select_step_changes_selection() {
        let mut state = ws(synthetic_recording(3));
        let _ = update(&mut state, Message::SelectStep(2));
        assert_eq!(state.selected, Some(2));
    }

    #[test]
    fn select_out_of_range_is_ignored() {
        let mut state = ws(synthetic_recording(2));
        let _ = update(&mut state, Message::SelectStep(99));
        assert_eq!(state.selected, Some(1));
    }

    #[test]
    fn title_changed_renames_selected_step() {
        let mut state = ws(synthetic_recording(2));
        let _ = update(&mut state, Message::TitleChanged("Open Preferences".to_string()));
        assert_eq!(state.selected_step().unwrap().title, "Open Preferences");
    }

    #[test]
    fn delete_step_renumbers_and_clamps_selection() {
        let mut state = ws(synthetic_recording(3));
        let _ = update(&mut state, Message::SelectStep(3));
        let _ = update(&mut state, Message::DeleteStep);
        assert_eq!(state.guide.steps().len(), 2);
        // Steps are renumbered 1..=2; selection clamps to the new last step.
        assert_eq!(state.selected, Some(2));
        assert!(state.guide.steps().iter().all(|s| s.index <= 2));
    }

    #[test]
    fn delete_middle_step_keeps_selection_on_a_real_step() {
        let mut state = ws(synthetic_recording(3));
        let _ = update(&mut state, Message::SelectStep(2));
        let _ = update(&mut state, Message::DeleteStep);
        assert_eq!(state.guide.steps().len(), 2);
        // Deleting the middle step renumbers remaining steps to 1..=2; selection
        // clamps to Some(2) (the former step 3) and must resolve to a real step.
        assert_eq!(state.selected, Some(2));
        assert!(state.selected_step().is_some());
    }

    #[test]
    fn delete_last_remaining_step_clears_selection() {
        let mut state = ws(synthetic_recording(1));
        let _ = update(&mut state, Message::DeleteStep);
        assert!(state.guide.steps().is_empty());
        assert_eq!(state.selected, None);
    }

    #[test]
    fn replace_keyframe_swaps_to_a_nearby_frame() {
        let mut state = ws(synthetic_recording(1));
        let step = state.selected_step().unwrap();
        // synthetic step 1: keyframe = 1, nearby = [0, 1, 2].
        assert_eq!(step.keyframe, 1);
        let target = *step.nearby.iter().find(|&&f| f != step.keyframe).unwrap();
        let _ = update(&mut state, Message::ReplaceKeyframe(target));
        assert_eq!(state.selected_step().unwrap().keyframe, target);
    }

    #[test]
    fn replace_keyframe_rejects_frame_outside_nearby() {
        let mut state = ws(synthetic_recording(1));
        let _ = update(&mut state, Message::ReplaceKeyframe(9999));
        assert_eq!(state.selected_step().unwrap().keyframe, 1);
    }

    #[test]
    fn delete_on_real_recording_keeps_handles_consistent() {
        // Real store so rebuild_selection_handles resolves frames; ensures the
        // delete path's handle rebuild does not panic.
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::DeleteStep);
        // No assertion on handle contents (opaque); reaching here = no panic.
    }
}
