use std::sync::Mutex;
use std::time::Duration;

use iced::futures::StreamExt;
use iced::{event, window, Event, Task};

use crate::app::{self, OverlayEffect, OverlayMessage, OverlayState};
use crate::coords::LogicalRect;
use crate::driver::{Driver, LiveOverlayEvent};
use crate::{CaptureResult, OverlayConfig, OverlayError};

static PREVIEW_RX: Mutex<
    Option<iced::futures::channel::mpsc::UnboundedReceiver<LiveOverlayEvent>>,
> = Mutex::new(None);
static RESULT_SLOT: Mutex<Option<Result<Option<CaptureResult>, String>>> = Mutex::new(None);
static DRIVER_SLOT: Mutex<Option<Driver>> = Mutex::new(None);

#[derive(Debug, Clone)]
enum Message {
    Overlay(OverlayMessage),
    WindowPatched(Result<(), String>),
    PassthroughEnabled,
    PassthroughDisabledThenExit,
}

fn preview_stream() -> iced::Subscription<Message> {
    iced::Subscription::run(|| {
        let rx = PREVIEW_RX
            .lock()
            .unwrap()
            .take()
            .expect("preview channel already consumed");
        rx.map(|e| Message::Overlay(OverlayMessage::LiveEvent(e)))
    })
}

fn subscription(_: &OverlayState) -> iced::Subscription<Message> {
    iced::Subscription::batch([
        event::listen_with(overlay_event_message),
        preview_stream(),
        iced::time::every(Duration::from_millis(250))
            .map(|_| Message::Overlay(OverlayMessage::Tick)),
    ])
}

fn overlay_event_message(
    event: Event,
    status: event::Status,
    window_id: window::Id,
) -> Option<Message> {
    match event {
        Event::Window(window::Event::Opened { size, .. }) => {
            Some(Message::Overlay(OverlayMessage::WindowOpened {
                id: window_id,
                size,
            }))
        }
        event if status == event::Status::Ignored => {
            Some(Message::Overlay(OverlayMessage::IcedEvent(event)))
        }
        _ => None,
    }
}

fn update(state: &mut OverlayState, message: Message) -> Task<Message> {
    match message {
        Message::Overlay(msg) => {
            let opened_window_id = match &msg {
                OverlayMessage::WindowOpened { id, .. } => Some(*id),
                _ => None,
            };
            let effect = app::update(state, msg);
            if let Some(id) = opened_window_id {
                return window::run(id, crate::macos_window::apply_overlay_window_patch)
                    .map(Message::WindowPatched);
            }

            match effect {
                OverlayEffect::None => Task::none(),
                OverlayEffect::BeginStitch => {
                    let crop = state.crop.unwrap();
                    let ws = match state.window_size {
                        Some(ws) => ws,
                        None => {
                            *RESULT_SLOT.lock().unwrap() =
                                Some(Err("overlay surface size unknown".to_string()));
                            return iced::exit();
                        }
                    };
                    let window_id = match state.window_id {
                        Some(id) => id,
                        None => {
                            *RESULT_SLOT.lock().unwrap() =
                                Some(Err("overlay window id unknown".to_string()));
                            return iced::exit();
                        }
                    };
                    let crop_logical = LogicalRect {
                        x: crop.x,
                        y: crop.y,
                        width: crop.width,
                        height: crop.height,
                    };
                    let overlay_logical = rollshot_capture::Size {
                        width: ws.width as u32,
                        height: ws.height as u32,
                    };
                    let preview_constraints = app::preview_constraints(crop, ws);
                    if let Some(driver) = DRIVER_SLOT.lock().unwrap().as_mut() {
                        driver.begin_stitch(crop_logical, overlay_logical, preview_constraints);
                    }
                    window::enable_mouse_passthrough(window_id)
                        .chain(Task::done(Message::PassthroughEnabled))
                }
                OverlayEffect::Finish => {
                    let should_disable_passthrough = state.mouse_passthrough_active;
                    let window_id = state.window_id;
                    let driver = DRIVER_SLOT.lock().unwrap().take();
                    let outcome = match driver {
                        Some(driver) => driver.finalize().map(Some),
                        None => Ok(None),
                    };
                    *RESULT_SLOT.lock().unwrap() = Some(outcome);
                    if should_disable_passthrough {
                        if let Some(id) = window_id {
                            return window::disable_mouse_passthrough(id)
                                .chain(Task::done(Message::PassthroughDisabledThenExit));
                        }
                    }
                    iced::exit()
                }
                OverlayEffect::Cancel => {
                    let should_disable_passthrough = state.mouse_passthrough_active;
                    let window_id = state.window_id;
                    if let Some(driver) = DRIVER_SLOT.lock().unwrap().take() {
                        driver.cancel();
                    }
                    *RESULT_SLOT.lock().unwrap() = Some(Ok(None));
                    if should_disable_passthrough {
                        if let Some(id) = window_id {
                            return window::disable_mouse_passthrough(id)
                                .chain(Task::done(Message::PassthroughDisabledThenExit));
                        }
                    }
                    iced::exit()
                }
                OverlayEffect::EnablePassthrough => match state.window_id {
                    Some(id) => window::enable_mouse_passthrough(id)
                        .chain(Task::done(Message::PassthroughEnabled)),
                    None => Task::none(),
                },
                OverlayEffect::DisablePassthrough => match state.window_id {
                    Some(id) => window::disable_mouse_passthrough(id)
                        .chain(Task::done(Message::PassthroughDisabledThenExit)),
                    None => Task::none(),
                },
            }
        }
        Message::WindowPatched(result) => {
            if let Err(err) = result {
                eprintln!("failed to patch macOS iced overlay window: {err}");
            }
            Task::none()
        }
        Message::PassthroughEnabled => {
            state.mouse_passthrough_active = true;
            Task::none()
        }
        Message::PassthroughDisabledThenExit => {
            state.mouse_passthrough_active = false;
            iced::exit()
        }
    }
}

fn theme(_: &OverlayState) -> iced::Theme {
    iced::Theme::Dark
}

fn view(state: &OverlayState) -> iced::Element<'_, Message> {
    crate::app::view(state).map(Message::Overlay)
}

pub(crate) fn run(config: OverlayConfig) -> Result<Option<CaptureResult>, OverlayError> {
    let (preview_tx, preview_rx) = iced::futures::channel::mpsc::unbounded();

    *PREVIEW_RX.lock().unwrap() = Some(preview_rx);
    *DRIVER_SLOT.lock().unwrap() = None;
    *RESULT_SLOT.lock().unwrap() = None;

    let driver = Driver::start_capture(&config.backend, config.fps, config.show_cursor, preview_tx)
        .map_err(OverlayError::Capture)?;
    let source_size = driver.source_size();
    *DRIVER_SLOT.lock().unwrap() = Some(driver);

    let settings = window::Settings {
        size: iced::Size::new(source_size.width as f32, source_size.height as f32),
        position: window::Position::Specific(iced::Point::ORIGIN),
        decorations: false,
        transparent: true,
        level: window::Level::AlwaysOnTop,
        resizable: false,
        ..window::Settings::default()
    };

    let run_result = iced::application(OverlayState::default, update, view)
        .window(settings)
        .subscription(subscription)
        .theme(theme)
        .style(app::style)
        .run();

    // Safety net: if the loop exited without finalize/cancel taking the driver,
    // tear capture down so the stream + reader thread don't leak.
    if let Some(driver) = DRIVER_SLOT.lock().unwrap().take() {
        driver.cancel();
    }

    run_result.map_err(|e| OverlayError::Overlay(e.to_string()))?;

    match RESULT_SLOT.lock().unwrap().take().unwrap_or(Ok(None)) {
        Ok(opt) => Ok(opt),
        Err(e) => Err(OverlayError::Capture(e)),
    }
}
