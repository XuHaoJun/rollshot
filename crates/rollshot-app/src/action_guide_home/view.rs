use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Length};

use super::recent::RecentEntry;
use super::update::{ActionGuideHome, Message};
use super::video_import::ImportState;

pub fn view<'a>(state: &'a ActionGuideHome) -> Element<'a, Message> {
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
