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
        event::listen().map(|e| Message::Overlay(OverlayMessage::IcedEvent(e))),
        preview_stream(),
        iced::time::every(Duration::from_millis(250))
            .map(|_| Message::Overlay(OverlayMessage::Tick)),
    ])
}

fn update(state: &mut OverlayState, message: Message) -> Task<Message> {
    match message {
        Message::WindowPatched(Ok(())) => Task::none(),
        Message::WindowPatched(Err(err)) => {
            eprintln!("macOS window patch failed: {err}");
            Task::none()
        }
        Message::Overlay(msg) => {
            // Intercept Window::Opened to apply macOS-specific AppKit patch.
            let patch_task = if let OverlayMessage::IcedEvent(Event::Window(
                window::Event::Opened { .. },
            )) = &msg
            {
                window::run(window::Id::default(), |w| {
                    Message::WindowPatched(crate::macos_window::apply_overlay_window_patch(w))
                })
            } else {
                Task::none()
            };
            let effect = app::update(state, msg);
            let effect_task = match effect {
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
                    // iced 0.14 creates a single main window; Id::default() is
                    // always the correct id for enable_mouse_passthrough.
                    window::enable_mouse_passthrough(window::Id::default())
                        .map(|_| Message::Overlay(OverlayMessage::Tick))
                }
                OverlayEffect::Finish => {
                    let driver = DRIVER_SLOT.lock().unwrap().take();
                    let outcome = match driver {
                        Some(driver) => driver.finalize().map(Some),
                        None => Ok(None),
                    };
                    *RESULT_SLOT.lock().unwrap() = Some(outcome);
                    return iced::exit();
                }
                OverlayEffect::Cancel => {
                    if let Some(driver) = DRIVER_SLOT.lock().unwrap().take() {
                        driver.cancel();
                    }
                    *RESULT_SLOT.lock().unwrap() = Some(Ok(None));
                    return iced::exit();
                }
                OverlayEffect::EnablePassthrough | OverlayEffect::DisablePassthrough => {
                    Task::none()
                }
            };
            Task::batch([patch_task, effect_task])
        }
    }
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
        .theme(|_| iced::Theme::Dark)
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
