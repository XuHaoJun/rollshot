//! TEMPORARY compatibility adapter: a thin iced-daemon host around
//! [`crate::macos_capture::Component`].
//!
//! This keeps `run_overlay`'s macOS behavior working during the migration. It
//! owns the daemon and process lifetime: it maps the component's
//! [`HostEffect::Completed`] into a result slot and exits, maps `Cancelled` /
//! `Fatal` to exit, and drives `Task`s. It holds NO duplicate capture state,
//! driver resources, or capture-specific update logic — all of that lives in
//! `macos_capture::Component`.
//!
//! Task 8 deletes this module once `rollshot-app` owns the daemon directly.

use std::sync::Mutex;

use iced::{window, Element, Point, Task};

use crate::macos_capture::{Component, HostEffect, Message};
use crate::{CaptureResult, OverlayConfig, OverlayError};

use rollshot_capture::CaptureMode;

/// Bridges the component's terminal `HostEffect` to `run`'s blocking return.
static RESULT_SLOT: Mutex<Option<Result<Option<CaptureResult>, String>>> = Mutex::new(None);

/// Hand-off slot for the single [`Component`], acquired once in `run` (so the
/// screen capture starts before the overlay surface exists) and moved into the
/// daemon by the `boot` closure. `iced::daemon`'s `boot` is `Fn`, so it cannot
/// own a non-`Clone` value directly; the daemon takes the component from here.
static COMPONENT_SLOT: Mutex<Option<Component>> = Mutex::new(None);

/// Apply a [`HostEffect`] to the daemon: stage terminal results / errors and
/// exit, or forward a task. The daemon's process lifetime is owned here.
fn handle_host_effect(effect: HostEffect) -> Task<Message> {
    match effect {
        HostEffect::None => Task::none(),
        HostEffect::Task(task) => task,
        HostEffect::Completed(result) => {
            *RESULT_SLOT.lock().unwrap() = Some(Ok(Some(result)));
            iced::exit()
        }
        HostEffect::Cancelled => {
            *RESULT_SLOT.lock().unwrap() = Some(Ok(None));
            iced::exit()
        }
        HostEffect::Fatal(error) => {
            *RESULT_SLOT.lock().unwrap() = Some(Err(error));
            iced::exit()
        }
    }
}

fn update(component: &mut Component, message: Message) -> Task<Message> {
    handle_host_effect(component.update(message))
}

fn view(component: &Component, window: window::Id) -> Element<'_, Message> {
    component.view(window)
}

fn theme(component: &Component, window: window::Id) -> iced::Theme {
    component.theme(window)
}

fn style(component: &Component, theme: &iced::Theme) -> iced::theme::Style {
    component.style(theme)
}

fn subscription(component: &Component) -> iced::Subscription<Message> {
    component.subscription()
}

fn overlay_window_settings(window_size: iced::Size, window_origin: Point) -> window::Settings {
    window::Settings {
        size: window_size,
        position: window::Position::Specific(window_origin),
        decorations: false,
        transparent: true,
        level: window::Level::AlwaysOnTop,
        resizable: false,
        ..window::Settings::default()
    }
}

pub(crate) fn run(config: OverlayConfig) -> Result<Option<CaptureResult>, OverlayError> {
    *RESULT_SLOT.lock().unwrap() = None;

    // Acquire the initial capture resource inside the component before the
    // overlay surface exists (so a screen-share picker, if any, dismisses on a
    // clean desktop). `Ok(None)` means the user cancelled before any capture.
    let component = match Component::new(&config)? {
        Some(c) => c,
        None => return Ok(None),
    };

    // Resolve window geometry from the acquired resource.
    let geom = component.window_geometry();
    let scale = geom.scale;
    let source_size = geom.source_size;

    // iced window sizes are logical points but `source_size` is physical pixels;
    // on a Retina display creating the window at `source_size` makes it `scale`x
    // oversized and collapses `map_crop_to_frame`'s scale ratio to 1.0 (the crop
    // then captures only the top-left fraction of the selection). Size the window
    // at the logical screen size so it covers the display 1:1 and the crop maps
    // at the true device scale.
    let window_size = iced::Size::new(
        source_size.width as f32 / scale as f32,
        source_size.height as f32 / scale as f32,
    );

    let window_origin = match config.initial_mode {
        CaptureMode::Screenshot => {
            if let Some(did) = geom.display_id {
                let display_geom = crate::macos_window::display_screen_geometry(did)
                    .map_err(OverlayError::Capture)?;
                iced::Point::new(
                    display_geom.logical_origin.0 as f32,
                    display_geom.logical_origin.1 as f32,
                )
            } else {
                iced::Point::ORIGIN
            }
        }
        CaptureMode::Scrolling => iced::Point::ORIGIN,
    };

    let settings = overlay_window_settings(window_size, window_origin);

    // Stash the single component for the daemon `boot` closure to take.
    *COMPONENT_SLOT.lock().unwrap() = Some(component);

    let run_result = iced::daemon(
        move || {
            let mut component = COMPONENT_SLOT
                .lock()
                .unwrap()
                .take()
                .expect("component already taken by daemon boot");
            let (overlay_window, open_overlay) = window::open(settings.clone());
            let boot_task = component.boot(overlay_window);
            (
                component,
                Task::batch([boot_task, open_overlay.map(Message::overlay_window_ready)]),
            )
        },
        update,
        view,
    )
    .subscription(subscription)
    .theme(theme)
    .style(style)
    .run();

    // Safety net: if the daemon exited before taking the component (e.g. it
    // failed to start), tear capture down so the stream + reader thread don't
    // leak.
    if let Some(mut component) = COMPONENT_SLOT.lock().unwrap().take() {
        component.shutdown();
    }

    run_result.map_err(|e| OverlayError::Overlay(e.to_string()))?;

    match RESULT_SLOT.lock().unwrap().take().unwrap_or(Ok(None)) {
        Ok(opt) => Ok(opt),
        Err(e) => Err(OverlayError::Capture(e)),
    }
}
