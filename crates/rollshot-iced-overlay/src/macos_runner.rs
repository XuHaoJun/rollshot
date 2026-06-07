use std::sync::Mutex;
use std::time::Duration;

use iced::futures::StreamExt;
use iced::widget::container;
use iced::{event, window, Element, Event, Length, Point, Size, Task};

use crate::app::{self, OverlayEffect, OverlayMessage, OverlayState};
use crate::coords::LogicalRect;
use crate::driver::{Driver, LiveOverlayEvent};
use crate::{CaptureResult, OverlayConfig, OverlayError};

use rollshot_capture::CaptureMode;

pub(crate) enum CaptureResource {
    Streaming(Driver),
    OneShot(rollshot_capture::OneShotCapture),
}

impl std::fmt::Debug for CaptureResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Streaming(_) => f.debug_tuple("Streaming").field(&"..").finish(),
            Self::OneShot(c) => f.debug_tuple("OneShot").field(&c.target_display()).finish(),
        }
    }
}

type StreamingFactory = dyn Fn(
    &OverlayConfig,
    iced::futures::channel::mpsc::UnboundedSender<LiveOverlayEvent>,
) -> Result<Driver, String>;

pub(crate) struct ResourceFactories {
    pub streaming: Box<StreamingFactory>,
    pub one_shot: Box<
        dyn Fn(bool) -> Result<rollshot_capture::OneShotCapture, rollshot_capture::CaptureError>,
    >,
}

impl std::fmt::Debug for ResourceFactories {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourceFactories").finish()
    }
}

pub(crate) fn acquire_resource(
    mode: CaptureMode,
    config: &OverlayConfig,
    factories: &ResourceFactories,
) -> Result<Option<CaptureResource>, OverlayError> {
    match mode {
        CaptureMode::Scrolling => {
            let (preview_tx, preview_rx) = iced::futures::channel::mpsc::unbounded();
            *PREVIEW_RX.lock().unwrap() = Some(preview_rx);
            let driver =
                (factories.streaming)(config, preview_tx).map_err(OverlayError::Capture)?;
            Ok(Some(CaptureResource::Streaming(driver)))
        }
        CaptureMode::Screenshot => {
            let capture = match (factories.one_shot)(config.show_cursor) {
                Ok(c) => c,
                Err(rollshot_capture::CaptureError::UserCancelled) => return Ok(None),
                Err(e) => return Err(OverlayError::Capture(e.to_string())),
            };
            Ok(Some(CaptureResource::OneShot(capture)))
        }
    }
}

static PREVIEW_RX: Mutex<
    Option<iced::futures::channel::mpsc::UnboundedReceiver<LiveOverlayEvent>>,
> = Mutex::new(None);
static RESULT_SLOT: Mutex<Option<Result<Option<CaptureResult>, String>>> = Mutex::new(None);
static DRIVER_SLOT: Mutex<Option<Driver>> = Mutex::new(None);
static ONE_SHOT_SLOT: Mutex<Option<rollshot_capture::OneShotCapture>> = Mutex::new(None);
static CAPTURE_MODE: Mutex<Option<CaptureMode>> = Mutex::new(None);

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
        let rx = PREVIEW_RX.lock().unwrap().take();
        match rx {
            Some(rx) => rx
                .map(|e| Message::Overlay(OverlayMessage::LiveEvent(e)))
                .boxed(),
            None => iced::futures::stream::pending().boxed(),
        }
    })
}

fn subscription(_: &MacOverlayState) -> iced::Subscription<Message> {
    let mode = *CAPTURE_MODE.lock().unwrap();
    let mut subs = vec![event::listen_with(overlay_event_message)];
    if mode == Some(CaptureMode::Scrolling) {
        subs.push(preview_stream());
        subs.push(
            iced::time::every(Duration::from_millis(250))
                .map(|_| Message::Overlay(OverlayMessage::Tick)),
        );
    }
    iced::Subscription::batch(subs)
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

/// After the overlay window opens, validate that a screenshot one-shot image is a
/// provable single-output match for the resolved display (mirrors the Linux
/// runner gate). On mismatch, record an explicit mapping error and exit instead
/// of cropping against the wrong geometry. Returns `Some(exit_task)` on failure.
fn validate_screenshot_surface_or_exit(state: &OverlayState) -> Option<Task<Message>> {
    if *CAPTURE_MODE.lock().unwrap() != Some(CaptureMode::Screenshot) {
        return None;
    }
    let ws = state.window_size?;
    let target = {
        let guard = ONE_SHOT_SLOT.lock().unwrap();
        guard.as_ref()?.target_display().clone()
    };

    let overlay_logical = rollshot_capture::Size {
        width: ws.width as u32,
        height: ws.height as u32,
    };
    let scale = target.physical_size.width as f64 / target.logical_region.width.max(1) as f64;
    match rollshot_capture::validate_surface_mapping(target.physical_size, overlay_logical, scale) {
        Ok(()) => None,
        Err(e) => {
            *RESULT_SLOT.lock().unwrap() = Some(Err(e.to_string()));
            Some(iced::exit())
        }
    }
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
                if let Some(exit) = validate_screenshot_surface_or_exit(&state.overlay) {
                    return exit;
                }
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
                    let mode = *CAPTURE_MODE.lock().unwrap();
                    if mode != Some(CaptureMode::Scrolling) {
                        return Task::none();
                    }
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
                    let mode = *CAPTURE_MODE.lock().unwrap();
                    if mode == Some(CaptureMode::Screenshot) {
                        let crop = state.overlay.crop.unwrap();
                        let ws = match state.overlay.window_size {
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
                        let capture = ONE_SHOT_SLOT.lock().unwrap().take();
                        let outcome = match capture {
                            Some(cap) => crate::screenshot::finish_screenshot(
                                &cap,
                                crop_logical,
                                overlay_logical,
                            )
                            .map(Some),
                            None => Ok(None),
                        };
                        *RESULT_SLOT.lock().unwrap() = Some(outcome);
                        return iced::exit();
                    }
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
                    ONE_SHOT_SLOT.lock().unwrap().take();
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

#[cfg(not(test))]
fn real_factories() -> ResourceFactories {
    ResourceFactories {
        streaming: Box::new(|cfg, preview_tx| {
            Driver::start_capture(&cfg.backend, cfg.fps, cfg.show_cursor, preview_tx)
        }),
        one_shot: Box::new(|show_cursor| {
            let kind = rollshot_capture::OneShotBackendKind::from_environment("auto")?;
            kind.capture_once(show_cursor)
        }),
    }
}

pub(crate) fn run(config: OverlayConfig) -> Result<Option<CaptureResult>, OverlayError> {
    *PREVIEW_RX.lock().unwrap() = None;
    *DRIVER_SLOT.lock().unwrap() = None;
    *ONE_SHOT_SLOT.lock().unwrap() = None;
    *RESULT_SLOT.lock().unwrap() = None;
    *CAPTURE_MODE.lock().unwrap() = None;

    #[cfg(not(test))]
    let factories = real_factories();
    #[cfg(test)]
    let factories = ResourceFactories {
        streaming: Box::new(|_cfg, _preview_tx| Err("test mode".to_string())),
        one_shot: Box::new(|_| {
            Err(rollshot_capture::CaptureError::Unsupported {
                message: "test mode".to_string(),
            })
        }),
    };

    let resource = acquire_resource(config.initial_mode, &config, &factories)?;
    let resource = match resource {
        Some(r) => r,
        None => return Ok(None),
    };

    *CAPTURE_MODE.lock().unwrap() = Some(config.initial_mode);

    let (source_size, scale, display_id) = match &resource {
        CaptureResource::Streaming(driver) => {
            let source_size = driver.source_size();
            let scale = crate::macos_window::main_screen_scale_factor()
                .filter(|s| *s > 0.0)
                .unwrap_or(1.0);
            (source_size, scale, None)
        }
        CaptureResource::OneShot(capture) => {
            let target = capture.target_display();
            let source_size = target.physical_size;
            let scale =
                target.physical_size.width as f64 / target.logical_region.width.max(1) as f64;
            let did = target
                .output_name
                .as_deref()
                .and_then(|s| s.parse::<u32>().ok());
            (source_size, scale, did)
        }
    };

    // Build the frozen background handle once (screenshot mode only). This is the
    // single full-image copy in the two-buffer render model; `view()` clones only
    // the cheap handle per redraw.
    let frozen_handle = match &resource {
        CaptureResource::OneShot(capture) => {
            let img = capture.image();
            Some(iced::widget::image::Handle::from_rgba(
                img.width(),
                img.height(),
                img.as_raw().clone(),
            ))
        }
        CaptureResource::Streaming(_) => None,
    };
    let mode = config.initial_mode;

    match resource {
        CaptureResource::Streaming(d) => {
            *DRIVER_SLOT.lock().unwrap() = Some(d);
        }
        CaptureResource::OneShot(c) => {
            *ONE_SHOT_SLOT.lock().unwrap() = Some(c);
        }
    }

    // iced window sizes are logical points but `source_size` is physical
    // pixels; on a Retina display creating the window at `source_size` makes it
    // `scale`× oversized and collapses `map_crop_to_frame`'s scale ratio to 1.0
    // (the crop then captures only the top-left fraction of the selection).
    // Size the window at the logical screen size so it covers the display 1:1
    // and the crop maps at the true device scale.
    let window_size = iced::Size::new(
        source_size.width as f32 / scale as f32,
        source_size.height as f32 / scale as f32,
    );

    let window_origin = match config.initial_mode {
        CaptureMode::Screenshot => {
            if let Some(did) = display_id {
                let geom = crate::macos_window::display_screen_geometry(did)
                    .map_err(OverlayError::Capture)?;
                iced::Point::new(geom.logical_origin.0 as f32, geom.logical_origin.1 as f32)
            } else {
                iced::Point::ORIGIN
            }
        }
        CaptureMode::Scrolling => iced::Point::ORIGIN,
    };

    let settings = window::Settings {
        size: window_size,
        position: window::Position::Specific(window_origin),
        decorations: false,
        transparent: true,
        level: window::Level::AlwaysOnTop,
        resizable: false,
        ..window::Settings::default()
    };

    let run_result = iced::daemon(
        move || {
            let (mut state, task) = boot(settings.clone());
            state.overlay.mode = mode;
            state.overlay.frozen = frozen_handle.clone();
            (state, task)
        },
        update,
        view,
    )
    .subscription(subscription)
    .theme(theme)
    .style(style)
    .run();

    // Safety net: if the loop exited without finalize/cancel taking the driver,
    // tear capture down so the stream + reader thread don't leak.
    if let Some(driver) = DRIVER_SLOT.lock().unwrap().take() {
        driver.cancel();
    }
    ONE_SHOT_SLOT.lock().unwrap().take();

    run_result.map_err(|e| OverlayError::Overlay(e.to_string()))?;

    match RESULT_SLOT.lock().unwrap().take().unwrap_or(Ok(None)) {
        Ok(opt) => Ok(opt),
        Err(e) => Err(OverlayError::Capture(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;
    use rollshot_capture::one_shot::DisplayTarget;
    use rollshot_capture::{CaptureError, Region, Size};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    fn test_config() -> OverlayConfig {
        OverlayConfig {
            backend: "auto".to_string(),
            fps: 5,
            show_cursor: false,
            initial_mode: CaptureMode::Scrolling,
        }
    }

    fn fake_one_shot_capture() -> rollshot_capture::OneShotCapture {
        let img = RgbaImage::new(1920, 1080);
        rollshot_capture::OneShotCapture::new(
            img,
            DisplayTarget {
                output_name: Some("eDP-1".to_string()),
                logical_region: Region {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                physical_size: Size {
                    width: 1920,
                    height: 1080,
                },
            },
        )
        .expect("test capture")
    }

    fn fake_streaming_factory(streaming_count: &'static AtomicUsize) -> Box<StreamingFactory> {
        Box::new(move |_config, _preview_tx| {
            streaming_count.fetch_add(1, Ordering::SeqCst);
            Err("fake streaming driver".to_string())
        })
    }

    fn fake_one_shot_factory(
        one_shot_count: &'static AtomicUsize,
    ) -> Box<dyn Fn(bool) -> Result<rollshot_capture::OneShotCapture, CaptureError>> {
        Box::new(move |_show_cursor| {
            one_shot_count.fetch_add(1, Ordering::SeqCst);
            Ok(fake_one_shot_capture())
        })
    }

    #[test]
    fn scrolling_calls_only_streaming_factory() {
        let _guard = TEST_MUTEX.lock().unwrap();
        static STREAMING_COUNT: AtomicUsize = AtomicUsize::new(0);
        static ONE_SHOT_COUNT: AtomicUsize = AtomicUsize::new(0);

        let config = test_config();
        let factories = ResourceFactories {
            streaming: fake_streaming_factory(&STREAMING_COUNT),
            one_shot: fake_one_shot_factory(&ONE_SHOT_COUNT),
        };

        let _ = acquire_resource(CaptureMode::Scrolling, &config, &factories);

        assert_eq!(STREAMING_COUNT.load(Ordering::SeqCst), 1);
        assert_eq!(ONE_SHOT_COUNT.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn screenshot_calls_only_one_shot_factory() {
        let _guard = TEST_MUTEX.lock().unwrap();
        static STREAMING_COUNT: AtomicUsize = AtomicUsize::new(0);
        static ONE_SHOT_COUNT: AtomicUsize = AtomicUsize::new(0);

        let config = test_config();
        let factories = ResourceFactories {
            streaming: fake_streaming_factory(&STREAMING_COUNT),
            one_shot: fake_one_shot_factory(&ONE_SHOT_COUNT),
        };

        let result = acquire_resource(CaptureMode::Screenshot, &config, &factories);
        assert!(result.unwrap().is_some());

        assert_eq!(STREAMING_COUNT.load(Ordering::SeqCst), 0);
        assert_eq!(ONE_SHOT_COUNT.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn acquire_resource_can_be_called_again_after_drop() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let config = test_config();

        let factories_1 = ResourceFactories {
            streaming: Box::new(|_c, _p| Err("first".to_string())),
            one_shot: Box::new(|_| Ok(fake_one_shot_capture())),
        };
        let result_1 = acquire_resource(CaptureMode::Screenshot, &config, &factories_1)
            .unwrap()
            .unwrap();
        drop(result_1);

        let factories_2 = ResourceFactories {
            streaming: Box::new(|_c, _p| Err("second".to_string())),
            one_shot: Box::new(|_| Ok(fake_one_shot_capture())),
        };
        let result_2 = acquire_resource(CaptureMode::Screenshot, &config, &factories_2)
            .unwrap()
            .unwrap();
        drop(result_2);
    }

    #[test]
    fn screenshot_release_returns_no_stats_result() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let capture = fake_one_shot_capture();
        let crop = LogicalRect {
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 100.0,
        };
        let overlay_logical = Size {
            width: 1920,
            height: 1080,
        };

        let result = crate::screenshot::finish_screenshot(&capture, crop, overlay_logical)
            .expect("screenshot should succeed");
        assert!(result.stats.is_none());
    }

    #[test]
    fn screenshot_mode_does_not_consume_preview_receiver() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let config = test_config();
        let factories = ResourceFactories {
            streaming: Box::new(|_c, _p| Err("unused".to_string())),
            one_shot: Box::new(|_| Ok(fake_one_shot_capture())),
        };

        *PREVIEW_RX.lock().unwrap() = None;

        let _result = acquire_resource(CaptureMode::Screenshot, &config, &factories).unwrap();

        assert!(
            PREVIEW_RX.lock().unwrap().is_none(),
            "screenshot mode should not set up preview channel"
        );
    }
}
