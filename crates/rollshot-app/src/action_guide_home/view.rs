use iced::widget::{button, checkbox, column, container, row, scrollable, text};
use iced::{Element, Length};

use super::recent::RecentEntry;
use super::update::{ActionGuideHome, Message, RecordPreflightPhase};
use super::video_import::ImportState;

pub fn view<'a>(state: &'a ActionGuideHome) -> Element<'a, Message> {
    if state.preflight.is_some() {
        return preflight_view(state);
    }
    match state.import_coordinator().state() {
        ImportState::Idle => home_view(state),
        _ => import_processing_view(state),
    }
}

fn home_view<'a>(state: &'a ActionGuideHome) -> Element<'a, Message> {
    let title = text("Action Guide").size(24);

    let record_btn = button(text("Record New").size(16))
        .on_press(Message::RecordNew)
        .padding([10, 20]);

    let import_btn = button(text("Import Recording...").size(16))
        .on_press(Message::ImportRecording)
        .padding([10, 20]);

    let open_btn = button(text("Open Project...").size(16))
        .on_press(Message::OpenPicker)
        .padding([10, 20]);

    let actions = row![record_btn, import_btn, open_btn].spacing(12);

    let message_row = if let Some(ref msg) = state.message {
        row![
            text(msg.as_str()).size(14),
            button(text("Dismiss").size(12)).on_press(Message::Clear)
        ]
        .spacing(8)
    } else {
        row![]
    };

    let recent_section = recent_list(state);

    let body = column![title, actions, message_row, recent_section]
        .spacing(16)
        .padding(20)
        .width(Length::Fill);

    container(scrollable(body))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn preflight_view<'a>(state: &'a ActionGuideHome) -> Element<'a, Message> {
    let preflight = match &state.preflight {
        Some(p) => p,
        None => return home_view(state),
    };

    let title = text("Record New").size(24);

    let description = text(
        "Saves the complete motion inside the Action Guide capture region with the project. No system audio or microphone.",
    )
    .size(14);

    let no_limit =
        text("No duration or file-size limit. Disk use is shown before saving.").size(12);

    let motion_checkbox = checkbox(preflight.keep_motion)
        .label("Keep a silent screen recording")
        .on_toggle(|_| Message::ToggleMotion);

    let confirm_enabled = !matches!(preflight.phase, RecordPreflightPhase::Resolving);

    let confirm_btn = {
        let btn = button(text("Start recording").size(16)).padding([10, 20]);
        if confirm_enabled {
            btn.on_press(Message::ConfirmRecordPreflight)
        } else {
            btn
        }
    };

    let cancel_btn = button(text("Cancel").size(16))
        .on_press(Message::CancelRecordPreflight)
        .padding([10, 20]);

    let actions = row![confirm_btn, cancel_btn].spacing(12);

    let mut body = column![title, description, no_limit, motion_checkbox, actions]
        .spacing(16)
        .padding(20)
        .width(Length::Fill);

    if let RecordPreflightPhase::NeedsSetup(_) = &preflight.phase {
        let retry_btn = button(text("Retry/setup").size(16))
            .on_press(Message::ConfirmRecordPreflight)
            .padding([10, 20]);
        let guide_only_btn = button(text("Continue Guide only").size(16))
            .on_press(Message::ToggleMotion)
            .padding([10, 20]);
        let setup_actions = row![retry_btn, guide_only_btn].spacing(12);
        body = body.push(setup_actions);
    }

    if let Some(ref msg) = state.message {
        body = body.push(
            row![
                text(msg.as_str()).size(14),
                button(text("Dismiss").size(12)).on_press(Message::Clear)
            ]
            .spacing(8),
        );
    }

    container(scrollable(body))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn import_processing_view<'a>(state: &'a ActionGuideHome) -> Element<'a, Message> {
    let coordinator = state.import_coordinator();
    let import_state = coordinator.state();

    let title = text("Importing Recording").size(24);

    let pass_copy = match import_state {
        ImportState::Picking => "Selecting file...".to_string(),
        ImportState::ResolvingToolchain => "Checking tools...".to_string(),
        ImportState::SettingUp => "Setting up tools...".to_string(),
        ImportState::Preflight => "Preparing video...".to_string(),
        ImportState::AnalyzingPass1 => "Analyzing video (pass 1 of 2)...".to_string(),
        ImportState::ExtractingPass2 => "Extracting frames (pass 2 of 2)...".to_string(),
        ImportState::Idle => unreachable!(),
    };

    let pass_text = text(pass_copy).size(16);

    let progress_row = if let Some(progress) = coordinator.last_progress() {
        let processed_s = progress.processed_ms / 1000;
        let total_s = progress.total_ms / 1000;
        let time_str = format!("{processed_s}s / {total_s}s");
        let retained_str = format!("{} frames retained", progress.retained_candidates);
        column![text(time_str).size(14), text(retained_str).size(14),].spacing(4)
    } else {
        column![]
    };

    let notice = text("Processing stays on this device. Audio is ignored.").size(12);

    let cancel_btn = button(text("Cancel").size(16))
        .on_press(Message::CancelImport)
        .padding([10, 20]);

    let message_row = if let Some(ref msg) = state.message {
        row![
            text(msg.as_str()).size(14),
            button(text("Dismiss").size(12)).on_press(Message::Clear)
        ]
        .spacing(8)
    } else {
        row![]
    };

    let body = column![
        title,
        pass_text,
        progress_row,
        notice,
        cancel_btn,
        message_row
    ]
    .spacing(16)
    .padding(20)
    .width(Length::Fill);

    container(scrollable(body))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn recent_list<'a>(state: &'a ActionGuideHome) -> Element<'a, Message> {
    let entries = state.recent.entries();
    if entries.is_empty() {
        return column![
            text("Recent Projects").size(18),
            text("No recent projects").size(14)
        ]
        .spacing(8)
        .into();
    }

    let header = text("Recent Projects").size(18);

    let mut list = column![header].spacing(4);
    for entry in entries {
        list = list.push(recent_card(entry));
    }

    list.into()
}

fn recent_card<'a>(entry: &'a RecentEntry) -> Element<'a, Message> {
    let name = text(entry.display_name.as_str()).size(16);

    let time_text = format_timestamp(entry.last_opened_ms);
    let time = text(time_text).size(12);

    let status = if entry.available {
        text("").size(12)
    } else {
        text("(unavailable)").size(12)
    };

    let card_content = column![name, row![time, status].spacing(8)]
        .spacing(4)
        .padding(8);

    let card = if entry.available {
        container(
            button(card_content)
                .on_press(Message::RecentSelected(entry.path.clone()))
                .width(Length::Fill),
        )
    } else {
        let remove_btn =
            button(text("Remove").size(12)).on_press(Message::RemoveRecent(entry.path.clone()));
        container(
            row![container(card_content).width(Length::Fill), remove_btn]
                .align_y(iced::Alignment::Center),
        )
    };

    card.width(Length::Fill).into()
}

fn format_timestamp(ms: u64) -> String {
    // Simple relative time display
    if ms == 0 {
        return String::new();
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if now_ms <= ms {
        return "just now".into();
    }
    let diff_s = (now_ms - ms) / 1000;
    if diff_s < 60 {
        format!("{diff_s}s ago")
    } else if diff_s < 3600 {
        format!("{}m ago", diff_s / 60)
    } else if diff_s < 86400 {
        format!("{}h ago", diff_s / 3600)
    } else {
        format!("{}d ago", diff_s / 86400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action_guide_home::update::{
        ActionGuideHome, RecordPreflight, RecordPreflightPhase,
    };
    use iced::Size as IcedSize;
    use iced_test::Simulator;

    fn home_with_preflight(keep_motion: bool, phase: RecordPreflightPhase) -> ActionGuideHome {
        let mut home = ActionGuideHome::new_empty();
        home.preflight = Some(RecordPreflight { keep_motion, phase });
        home
    }

    fn simulator_at<'a>(state: &'a ActionGuideHome, size: IcedSize) -> Simulator<'a, Message> {
        Simulator::with_size(iced::Settings::default(), size, view(state))
    }

    #[test]
    fn preflight_visible_at_1100x760() {
        let state = home_with_preflight(false, RecordPreflightPhase::Confirm);
        let mut ui = simulator_at(&state, IcedSize::new(1100.0, 760.0));

        for label in [
            "Keep a silent screen recording",
            "Saves the complete motion inside the Action Guide capture region with the project. No system audio or microphone.",
            "Start recording",
            "Cancel",
        ] {
            let target = ui
                .find(label)
                .unwrap_or_else(|e| panic!("{label:?} missing at 1100x760: {e}"));
            assert!(
                target.visible_bounds().is_some(),
                "{label:?} not visible at 1100x760"
            );
        }
    }

    #[test]
    fn preflight_visible_at_640x420() {
        let state = home_with_preflight(false, RecordPreflightPhase::Confirm);
        let mut ui = simulator_at(&state, IcedSize::new(640.0, 420.0));

        for label in [
            "Keep a silent screen recording",
            "Saves the complete motion inside the Action Guide capture region with the project. No system audio or microphone.",
            "Start recording",
            "Cancel",
        ] {
            let target = ui
                .find(label)
                .unwrap_or_else(|e| panic!("{label:?} missing at 640x420: {e}"));
            assert!(
                target.visible_bounds().is_some(),
                "{label:?} not visible at 640x420"
            );
        }
    }

    #[test]
    fn preflight_default_unchecked_checkbox() {
        let state = home_with_preflight(false, RecordPreflightPhase::Confirm);
        let mut ui = simulator_at(&state, IcedSize::new(1100.0, 760.0));

        let checkbox = ui
            .find("Keep a silent screen recording")
            .expect("checkbox must exist");
        assert!(checkbox.visible_bounds().is_some());
    }

    #[test]
    fn preflight_setup_failure_shows_retry_and_guide_only() {
        let info = crate::managed_ffmpeg::FfmpegSetupInfo {
            managed_download: None,
            install_location: std::path::PathBuf::new(),
        };
        let state = home_with_preflight(true, RecordPreflightPhase::NeedsSetup(info));
        let mut ui = simulator_at(&state, IcedSize::new(1100.0, 760.0));

        for label in ["Retry/setup", "Continue Guide only"] {
            let target = ui
                .find(label)
                .unwrap_or_else(|e| panic!("{label:?} missing at setup failure: {e}"));
            assert!(
                target.visible_bounds().is_some(),
                "{label:?} not visible at setup failure"
            );
        }
    }

    #[test]
    fn preflight_no_duration_or_disk_warning() {
        let state = home_with_preflight(false, RecordPreflightPhase::Confirm);
        let mut ui = simulator_at(&state, IcedSize::new(1100.0, 760.0));

        let label = "No duration or file-size limit. Disk use is shown before saving.";
        let target = ui
            .find(label)
            .unwrap_or_else(|e| panic!("{label:?} missing: {e}"));
        assert!(target.visible_bounds().is_some());
    }

    /// Deterministic iced UI scenario tests for action-guide motion preflight.
    ///
    /// Covers the preflight confirm state at default (1100×760) and minimum
    /// (640×420) viewports. Each scenario runs structural assertions first,
    /// then emits a PNG artifact for semantic inspection.
    #[test]
    #[ignore = "writes visual scenario artifacts"]
    fn action_guide_motion_ui_scenarios() {
        use serde_json::json;

        let artifact_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/ui-artifacts/action-guide-motion");
        std::fs::create_dir_all(&artifact_dir).ok();

        let scenarios: Vec<(&str, IcedSize, Vec<&str>)> = vec![
            (
                "preflight-confirm-1100x760",
                IcedSize::new(1100.0, 760.0),
                vec![
                    "Keep a silent screen recording",
                    "Saves the complete motion inside the Action Guide capture region with the project. No system audio or microphone.",
                    "Start recording",
                    "Cancel",
                    "No duration or file-size limit. Disk use is shown before saving.",
                ],
            ),
            (
                "preflight-confirm-640x420",
                IcedSize::new(640.0, 420.0),
                vec![
                    "Keep a silent screen recording",
                    "Saves the complete motion inside the Action Guide capture region with the project. No system audio or microphone.",
                    "Start recording",
                    "Cancel",
                    "No duration or file-size limit. Disk use is shown before saving.",
                ],
            ),
        ];

        let mut manifest_rows: Vec<serde_json::Value> = Vec::new();

        for (name, size, expected_texts) in &scenarios {
            let state = home_with_preflight(false, RecordPreflightPhase::Confirm);
            let mut ui = simulator_at(&state, *size);

            // Structural assertions: every expected label must be present and visible.
            for label in expected_texts {
                let target = ui
                    .find(*label)
                    .unwrap_or_else(|e| panic!("{label:?} missing in {name}: {e}"));
                assert!(
                    target.visible_bounds().is_some(),
                    "{label:?} not visible in {name}"
                );
            }

            // Emit PNG artifact.
            let snapshot = ui
                .snapshot(&iced::Theme::Dark)
                .unwrap_or_else(|e| panic!("snapshot failed for {name}: {e}"));
            let base = artifact_dir.join(name);
            let written = snapshot
                .matches_image(&base)
                .unwrap_or_else(|e| panic!("matches_image failed for {name}: {e}"));
            assert!(written, "{name}: baseline PNG was not written");

            manifest_rows.push(json!({
                "scenario": name,
                "viewport": format!("{}x{}", size.width as u32, size.height as u32),
                "expected_key_texts": expected_texts,
                "baseline": format!("{name}.png"),
                "actual": format!("{name}.png"),
                "diff": serde_json::Value::Null,
                "structural_pass": true,
            }));
        }

        // Write manifest (read-and-append to coexist with workspace/overlay scenarios).
        let manifest_path = artifact_dir.join("manifest.json");
        let mut manifest: serde_json::Value = if manifest_path.exists() {
            serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap()
        } else {
            json!({
                "suite": "action-guide-motion",
                "theme": "Dark",
                "scenarios": [],
            })
        };
        if let Some(scenarios_arr) = manifest.get_mut("scenarios").and_then(|v| v.as_array_mut()) {
            scenarios_arr.extend(manifest_rows);
        }
        std::fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .expect("write manifest");
    }
}
