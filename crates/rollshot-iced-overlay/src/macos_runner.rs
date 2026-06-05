use std::sync::Mutex;
use std::time::Duration;

use iced::futures::StreamExt;
use iced::widget::container;
use iced::{event, window, Element, Event, Length, Point, Size, Task};

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
    WindowOpened { id: window::Id, size: Size },
    OverlayWindowReady(window::Id),
    ControlsWindowReady(window::Id),
    WindowPatched(Result<(), String>),
    PassthroughEnabled,
    PassthroughDisabledThenExit,
}

#[derive(Default)]
struct MacOverlayState {
    overlay: OverlayState,
    overlay_window: Option<window::Id>,
    controls_window: Option<window::Id>,
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

fn subscription(_: &MacOverlayState) -> iced::Subscription<Message> {
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
        Event::Window(window::Event::Opened { size, .. }) => Some(Message::WindowOpened {
            id: window_id,
            size,
        }),
        event if status == event::Status::Ignored => {
            Some(Message::Overlay(OverlayMessage::IcedEvent(event)))
        }
        _ => None,
    }
}

fn controls_window_settings(x: i32, y: i32, w: i32, h: i32) -> window::Settings {
    window::Settings {
        size: Size::new(w.max(1) as f32, h.max(1) as f32),
        position: window::Position::Specific(Point::new(x.max(0) as f32, y.max(0) as f32)),
        decorations: false,
        transparent: true,
        level: window::Level::AlwaysOnTop,
        resizable: false,
        ..window::Settings::default()
    }
}

fn boot(settings: window::Settings) -> (MacOverlayState, Task<Message>) {
    let (overlay_window, open_overlay) = window::open(settings);
    (
        MacOverlayState {
            overlay_window: Some(overlay_window),
            ..MacOverlayState::default()
        },
        open_overlay.map(Message::OverlayWindowReady),
    )
}

fn update(state: &mut MacOverlayState, message: Message) -> Task<Message> {
    match message {
        Message::WindowOpened { id, size } => {
            let patch = window::run(id, crate::macos_window::apply_overlay_window_patch)
                .map(Message::WindowPatched);
            if Some(id) == state.overlay_window {
                app::update(
                    &mut state.overlay,
                    OverlayMessage::WindowOpened { id, size },
                );
            }
            patch
        }
        Message::OverlayWindowReady(id) => {
            state.overlay_window = Some(id);
            Task::none()
        }
        Message::ControlsWindowReady(id) => {
            state.controls_window = Some(id);
            Task::none()
        }
        Message::Overlay(msg) => {
            let effect = app::update(&mut state.overlay, msg);
            match effect {
                OverlayEffect::None => Task::none(),
                OverlayEffect::BeginStitch => {
                    let crop = state.overlay.crop.unwrap();
                    let ws = match state.overlay.window_size {
                        Some(ws) => ws,
                        None => {
                            *RESULT_SLOT.lock().unwrap() =
                                Some(Err("overlay surface size unknown".to_string()));
                            return iced::exit();
                        }
                    };
                    let window_id = match state.overlay.window_id {
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
                    let passthrough = window::enable_mouse_passthrough(window_id)
                        .chain(Task::done(Message::PassthroughEnabled));
                    let controls = app::toolbar_input_rect(crop, ws).map(|(x, y, w, h)| {
                        let (controls_window, open_controls) =
                            window::open(controls_window_settings(x, y, w, h));
                        state.controls_window = Some(controls_window);
                        open_controls.map(Message::ControlsWindowReady)
                    });

                    match controls {
                        Some(open_controls) => Task::batch([open_controls, passthrough]),
                        None => passthrough,
                    }
                }
                OverlayEffect::Finish => {
                    let should_disable_passthrough = state.overlay.mouse_passthrough_active;
                    let window_id = state.overlay.window_id;
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
                    let should_disable_passthrough = state.overlay.mouse_passthrough_active;
                    let window_id = state.overlay.window_id;
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
                OverlayEffect::EnablePassthrough => match state.overlay.window_id {
                    Some(id) => window::enable_mouse_passthrough(id)
                        .chain(Task::done(Message::PassthroughEnabled)),
                    None => Task::none(),
                },
                OverlayEffect::DisablePassthrough => match state.overlay.window_id {
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
            state.overlay.mouse_passthrough_active = true;
            Task::none()
        }
        Message::PassthroughDisabledThenExit => {
            state.overlay.mouse_passthrough_active = false;
            iced::exit()
        }
    }
}

fn theme(_: &MacOverlayState, _: window::Id) -> iced::Theme {
    iced::Theme::Dark
}

fn style(state: &MacOverlayState, theme: &iced::Theme) -> iced::theme::Style {
    app::style(&state.overlay, theme)
}

fn view(state: &MacOverlayState, window: window::Id) -> Element<'_, Message> {
    if Some(window) == state.controls_window {
        return container(app::capture_control_strip().map(Message::Overlay))
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }

    crate::app::view(&state.overlay).map(Message::Overlay)
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

    // iced window sizes are logical points but `source_size` is physical
    // pixels; on a Retina display creating the window at `source_size` makes it
    // `scale`× oversized and collapses `map_crop_to_frame`'s scale ratio to 1.0
    // (the crop then captures only the top-left fraction of the selection).
    // Size the window at the logical screen size so it covers the display 1:1
    // and the crop maps at the true device scale.
    let scale = crate::macos_window::main_screen_scale_factor()
        .filter(|s| *s > 0.0)
        .unwrap_or(1.0);
    let window_size = iced::Size::new(
        source_size.width as f32 / scale as f32,
        source_size.height as f32 / scale as f32,
    );

    let settings = window::Settings {
        size: window_size,
        position: window::Position::Specific(iced::Point::ORIGIN),
        decorations: false,
        transparent: true,
        level: window::Level::AlwaysOnTop,
        resizable: false,
        ..window::Settings::default()
    };

    let run_result = iced::daemon(move || boot(settings.clone()), update, view)
        .subscription(subscription)
        .theme(theme)
        .style(style)
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
