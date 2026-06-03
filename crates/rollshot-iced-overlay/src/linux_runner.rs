use iced::{event, keyboard, mouse, window, Event, Point, Task};
use iced_layershell::actions::ActionCallback;
use iced_layershell::build_pattern::application;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, StartMode};
use iced_layershell::to_layer_message;
use iced_layershell::Settings;

use iced::futures::StreamExt;
use std::sync::Mutex;

use crate::app::{self, OverlayMessage, OverlayState as Overlay};
use crate::coords::LogicalRect;
use crate::driver::Driver;
use crate::CaptureResult;
use crate::OverlayConfig;
use crate::OverlayError;

static PREVIEW_RX: Mutex<
    Option<iced::futures::channel::mpsc::UnboundedReceiver<crate::driver::LiveOverlayEvent>>,
> = Mutex::new(None);
static RESULT_SLOT: Mutex<Option<Result<Option<CaptureResult>, String>>> = Mutex::new(None);

// Capture starts in `run()` before the overlay surface exists, so the portal
// screen-share picker dialog appears + dismisses on a clean desktop and never
// lands in a captured frame. The live Driver is stashed here for the update fn
// to drive: `begin_stitch` on Finish, `finalize`/`cancel` on Esc.
static DRIVER_SLOT: Mutex<Option<Driver>> = Mutex::new(None);

#[to_layer_message]
#[derive(Debug, Clone)]
pub enum Message {
    IcedEvent(Event),
    Finish,
    Cancel,
    LiveEvent(crate::driver::LiveOverlayEvent),
    Tick,
}

fn namespace() -> String {
    "rollshot-iced-overlay".to_string()
}

fn preview_stream() -> iced::Subscription<Message> {
    iced::Subscription::run(|| {
        let rx = PREVIEW_RX
            .lock()
            .unwrap()
            .take()
            .expect("preview channel already consumed");

        rx.map(Message::LiveEvent)
    })
}

fn subscription(_: &Overlay) -> iced::Subscription<Message> {
    iced::Subscription::batch([
        event::listen().map(Message::IcedEvent),
        preview_stream(),
        iced::time::every(std::time::Duration::from_millis(250)).map(|_| Message::Tick),
    ])
}

fn update(state: &mut Overlay, message: Message) -> Task<Message> {
    match message {
        Message::IcedEvent(Event::Window(window::Event::Opened { size, .. })) => {
            state.window_size = Some(size);
            Task::none()
        }
        Message::IcedEvent(Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)))
            if !state.crop_confirmed =>
        {
            state.drag_start = Some(Point::ORIGIN);
            state.crop = None;
            Task::none()
        }
        Message::IcedEvent(Event::Mouse(mouse::Event::CursorMoved { position })) => {
            if let Some(start) = state.drag_start {
                if start == Point::ORIGIN && state.crop.is_none() {
                    state.drag_start = Some(position);
                }
                if let Some(start) = state.drag_start {
                    let x = start.x.min(position.x);
                    let y = start.y.min(position.y);
                    let w = (position.x - start.x).abs();
                    let h = (position.y - start.y).abs();
                    state.crop = Some(iced::Rectangle {
                        x,
                        y,
                        width: w,
                        height: h,
                    });
                }
            }
            Task::none()
        }
        Message::IcedEvent(Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))) => {
            state.drag_start = None;
            if !state.crop_confirmed && state.crop.is_some_and(|c| c.width > 0.0 && c.height > 0.0)
            {
                Task::done(Message::Finish)
            } else {
                Task::none()
            }
        }
        Message::IcedEvent(Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        })) => {
            let driver = DRIVER_SLOT.lock().unwrap().take();
            let outcome = match (state.crop_confirmed, driver) {
                (true, Some(driver)) => driver.finalize().map(Some),
                (false, Some(driver)) => {
                    driver.cancel();
                    Ok(None)
                }
                (_, None) => Ok(None),
            };
            *RESULT_SLOT.lock().unwrap() = Some(outcome);
            iced::exit()
        }
        Message::IcedEvent(Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Enter),
            ..
        })) if !state.crop_confirmed && state.crop.is_some() => Task::done(Message::Finish),
        Message::Finish => {
            if state.crop_confirmed {
                return Task::none();
            }
            let crop = match state.crop {
                Some(c) if c.width >= 1.0 && c.height >= 1.0 => c,
                _ => return Task::none(),
            };
            let ws = match state.window_size {
                Some(ws) => ws,
                None => {
                    *RESULT_SLOT.lock().unwrap() = Some(Err(
                        "overlay surface size unknown (no Window::Opened event)".to_string(),
                    ));
                    return iced::exit();
                }
            };

            state.crop_confirmed = true;

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

            let input_rect = app::toolbar_input_rect(crop, ws);
            Task::done(Message::SetInputRegion(ActionCallback::new(
                move |region| {
                    if let Some((x, y, w, h)) = input_rect {
                        region.add(x, y, w, h);
                    }
                },
            )))
        }
        Message::Cancel => {
            if let Some(driver) = DRIVER_SLOT.lock().unwrap().take() {
                driver.cancel();
            }
            *RESULT_SLOT.lock().unwrap() = Some(Ok(None));
            iced::exit()
        }
        Message::LiveEvent(crate::driver::LiveOverlayEvent::Preview(handle)) => {
            state.preview = Some(handle);
            Task::none()
        }
        Message::LiveEvent(crate::driver::LiveOverlayEvent::CaptureMiss(miss)) => {
            if miss.warn {
                state.capture_miss_warn = true;
                state.capture_miss_message_expires_at =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
            }
            Task::none()
        }
        Message::Tick => {
            if state
                .capture_miss_message_expires_at
                .is_some_and(|deadline| std::time::Instant::now() >= deadline)
            {
                state.capture_miss_warn = false;
                state.capture_miss_message_expires_at = None;
            }
            Task::none()
        }
        _ => Task::none(),
    }
}

fn view(state: &Overlay) -> iced::Element<'_, Message> {
    crate::app::view(state).map(|msg| match msg {
        OverlayMessage::IcedEvent(e) => Message::IcedEvent(e),
        OverlayMessage::Finish => Message::Finish,
        OverlayMessage::Cancel => Message::Cancel,
        OverlayMessage::LiveEvent(e) => Message::LiveEvent(e),
        OverlayMessage::Tick => Message::Tick,
    })
}

pub fn run(config: OverlayConfig) -> Result<Option<CaptureResult>, OverlayError> {
    let (preview_tx, preview_rx) = iced::futures::channel::mpsc::unbounded();

    *PREVIEW_RX.lock().unwrap() = Some(preview_rx);
    *DRIVER_SLOT.lock().unwrap() = None;
    *RESULT_SLOT.lock().unwrap() = None;

    // Start capture BEFORE building the overlay: the portal screen-share picker
    // then appears (and dismisses) on a clean desktop, so it is never composited
    // into a captured frame. Blocks until the user clicks Share and the first
    // frame arrives.
    let driver = Driver::start_capture(&config.backend, config.fps, config.show_cursor, preview_tx)
        .map_err(OverlayError::Capture)?;
    *DRIVER_SLOT.lock().unwrap() = Some(driver);

    let run_result = application(Overlay::default, namespace, update, view)
        .style(app::style)
        .subscription(subscription)
        .settings(Settings {
            layer_settings: LayerShellSettings {
                anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
                layer: Layer::Overlay,
                // -1 = extend to all anchored edges, covering the full output
                // (panels/taskbars included). 0 would let the compositor shrink
                // us to the work area, but the PipeWire capture is FullSource
                // (whole monitor), so a shorter overlay inflates scale_y in
                // map_crop_to_frame and over-captures below the crop (worse
                // toward the bottom). Must match the capture's coordinate space.
                exclusive_zone: -1,
                size: None,
                margin: (0, 0, 0, 0),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                start_mode: StartMode::Active,
                events_transparent: false,
            },
            ..Default::default()
        })
        .run();

    // Safety net: if the loop exited without finalize/cancel taking the driver,
    // tear capture down so the PipeWire stream + reader thread don't leak.
    if let Some(driver) = DRIVER_SLOT.lock().unwrap().take() {
        driver.cancel();
    }

    run_result.map_err(|e| OverlayError::Overlay(e.to_string()))?;

    // After the iced app exits cleanly, read the result slot.
    match RESULT_SLOT.lock().unwrap().take().unwrap_or(Ok(None)) {
        Ok(opt) => Ok(opt),
        Err(e) => Err(OverlayError::Capture(e)),
    }
}
