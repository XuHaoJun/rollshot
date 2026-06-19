use std::path::{Path, PathBuf};

use iced::Task;
use rollshot_action::{export_gif, export_guide, GifOptions};

use super::TimelineWorkspace;

#[derive(Debug, Clone)]
pub enum Message {
    SelectStep(usize),
    TitleChanged(String),
    DeleteStep,
    ReplaceKeyframe(rollshot_action::FrameId),
    DiscardRequested,
    CloseRequested,
    CancelDiscard,
    ConfirmDiscard,
    ExportRequested,
    ExportDirChosen(Option<PathBuf>),
    ExportGifRequested,
    ExportGifPathChosen(Option<PathBuf>),
    #[cfg(target_os = "macos")]
    OpenInputMonitoringSettings,
    DismissBanner,
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
        Message::DiscardRequested | Message::CloseRequested => {
            state.pending_discard = true;
            Task::none()
        }
        Message::CancelDiscard => {
            state.pending_discard = false;
            Task::none()
        }
        Message::ConfirmDiscard => {
            state.pending_discard = false;
            iced::exit()
        }
        Message::ExportRequested => {
            state.message = None;
            Task::perform(
                pick_export_dir(picker_default_dir()),
                Message::ExportDirChosen,
            )
        }
        Message::ExportDirChosen(None) => Task::none(),
        Message::ExportDirChosen(Some(dir)) => match export_to(state, &dir) {
            Ok(out) => {
                tracing::info!(
                    target: "rollshot::action::export",
                    path = %out.display(),
                    "guide exported"
                );
                state.message = None;
                iced::exit()
            }
            Err(error) => {
                tracing::error!(
                    target: "rollshot::action::export",
                    %error,
                    "guide export failed"
                );
                state.message = Some(error);
                Task::none()
            }
        },
        Message::ExportGifRequested => {
            state.message = None;
            Task::perform(
                pick_gif_save_path(picker_default_dir()),
                Message::ExportGifPathChosen,
            )
        }
        Message::ExportGifPathChosen(None) => Task::none(),
        Message::ExportGifPathChosen(Some(path)) => {
            match export_gif(&state.guide, &state.store, GifOptions::default(), &path) {
                Ok(()) => {
                    tracing::info!(
                        target: "rollshot::action::export",
                        path = %path.display(),
                        "gif exported"
                    );
                    state.message = Some(format!("GIF saved to {}", path.display()));
                }
                Err(error) => {
                    tracing::error!(
                        target: "rollshot::action::export",
                        %error,
                        "gif export failed"
                    );
                    state.message = Some(format!("GIF export failed: {error}"));
                }
            }
            // Unlike guide export, GIF export does NOT exit — the user can still
            // Export Guide afterwards.
            Task::none()
        }
        #[cfg(target_os = "macos")]
        Message::OpenInputMonitoringSettings => {
            rollshot_macos_input::open_input_monitoring_settings();
            state.message = Some("Grant Input Monitoring, then restart Rollshot.".to_string());
            Task::none()
        }
        Message::DismissBanner => {
            state.message = None;
            Task::none()
        }
    }
}

pub fn subscription(_state: &TimelineWorkspace) -> iced::Subscription<Message> {
    iced::window::close_requests().map(|_id| Message::CloseRequested)
}

/// Export the (possibly edited) guide into `out_dir/action-guide/`.
fn export_to(state: &TimelineWorkspace, out_dir: &Path) -> Result<PathBuf, String> {
    export_guide(
        &state.guide,
        &state.store,
        state.region,
        state.capability,
        state.source_kind,
        out_dir,
    )
    .map_err(|e| format!("export failed: {e}"))
}

/// Initial directory for the folder picker: the user's Pictures dir, or temp.
fn picker_default_dir() -> PathBuf {
    dirs::picture_dir().unwrap_or_else(std::env::temp_dir)
}

async fn pick_export_dir(default_dir: PathBuf) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_directory(default_dir)
        .pick_folder()
        .await
        .map(|handle| handle.path().to_path_buf())
}

async fn pick_gif_save_path(default_dir: PathBuf) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_directory(default_dir)
        .set_file_name("summary.gif")
        .add_filter("GIF image", &["gif"])
        .save_file()
        .await
        .map(|handle| handle.path().to_path_buf())
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
            CaptureRegion {
                x: 0,
                y: 0,
                width: 32,
                height: 32,
            },
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
        let _ = update(
            &mut state,
            Message::TitleChanged("Open Preferences".to_string()),
        );
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

    #[test]
    fn discard_requested_shows_modal_then_cancel_clears_it() {
        let mut state = ws(synthetic_recording(2));
        let _ = update(&mut state, Message::DiscardRequested);
        assert!(state.pending_discard);
        let _ = update(&mut state, Message::CancelDiscard);
        assert!(!state.pending_discard);
    }

    #[test]
    fn confirm_discard_clears_pending_flag() {
        let mut state = ws(synthetic_recording(1));
        let _ = update(&mut state, Message::DiscardRequested);
        assert!(state.pending_discard);
        let _ = update(&mut state, Message::ConfirmDiscard);
        assert!(!state.pending_discard);
    }

    #[test]
    fn close_requested_also_prompts_discard() {
        let mut state = ws(synthetic_recording(1));
        let _ = update(&mut state, Message::CloseRequested);
        assert!(state.pending_discard);
    }

    #[test]
    fn export_dir_chosen_writes_guide_folder_and_clears_message() {
        let mut state = ws(recording_from_frames());
        state.message = Some("stale".to_string());
        let tmp = tempfile::tempdir().unwrap();
        let _ = update(
            &mut state,
            Message::ExportDirChosen(Some(tmp.path().to_path_buf())),
        );
        assert!(tmp.path().join("action-guide/steps.md").exists());
        assert!(tmp.path().join("action-guide/session.json").exists());
        assert!(
            state.message.is_none(),
            "successful export clears the banner"
        );
    }

    #[test]
    fn export_empty_guide_sets_error_and_writes_nothing() {
        let mut state = ws(synthetic_recording(0));
        let tmp = tempfile::tempdir().unwrap();
        let _ = update(
            &mut state,
            Message::ExportDirChosen(Some(tmp.path().to_path_buf())),
        );
        assert!(
            !tmp.path().join("action-guide").exists(),
            "empty guide must not write a folder"
        );
        assert!(
            state.message.is_some(),
            "export failure surfaces an inline message"
        );
    }

    #[test]
    fn export_cancelled_picker_is_a_no_op() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::ExportDirChosen(None));
        assert!(state.message.is_none());
    }

    #[test]
    fn export_gif_path_chosen_writes_file_and_keeps_window_open() {
        let mut state = ws(recording_from_frames());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("summary.gif");
        let _ = update(&mut state, Message::ExportGifPathChosen(Some(path.clone())));
        assert!(path.exists(), "GIF file should be written");
        assert!(
            state
                .message
                .as_ref()
                .is_some_and(|m| m.contains("GIF saved")),
            "success banner expected, got {:?}",
            state.message
        );
    }

    #[test]
    fn export_gif_empty_guide_sets_error_and_writes_nothing() {
        let mut state = ws(synthetic_recording(0));
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("summary.gif");
        let _ = update(&mut state, Message::ExportGifPathChosen(Some(path.clone())));
        assert!(!path.exists(), "empty guide must not write a file");
        assert!(
            state.message.is_some(),
            "failure surfaces an inline message"
        );
    }

    #[test]
    fn export_gif_cancelled_picker_is_a_no_op() {
        let mut state = ws(recording_from_frames());
        let _ = update(&mut state, Message::ExportGifPathChosen(None));
        assert!(state.message.is_none());
    }
}
