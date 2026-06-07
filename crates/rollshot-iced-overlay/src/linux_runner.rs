use iced::{event, Task};
use iced_layershell::actions::ActionCallback;
use iced_layershell::build_pattern::application;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, StartMode};
use iced_layershell::to_layer_message;
use iced_layershell::Settings;

use iced::futures::StreamExt;
use std::sync::Mutex;

use crate::app::{self, OverlayState as Overlay};
use crate::coords::LogicalRect;
use crate::driver::Driver;
use crate::CaptureResult;
use crate::OverlayConfig;
use crate::OverlayError;

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

#[allow(clippy::type_complexity)]
pub(crate) struct ResourceFactories {
    pub streaming: Box<
        dyn Fn(
            &OverlayConfig,
            iced::futures::channel::mpsc::UnboundedSender<crate::driver::LiveOverlayEvent>,
        ) -> Result<Driver, String>,
    >,
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
    Option<iced::futures::channel::mpsc::UnboundedReceiver<crate::driver::LiveOverlayEvent>>,
> = Mutex::new(None);
static RESULT_SLOT: Mutex<Option<Result<Option<CaptureResult>, String>>> = Mutex::new(None);

// Capture starts in `run()` before the overlay surface exists, so the portal
// screen-share picker dialog appears + dismisses on a clean desktop and never
// lands in a captured frame. The live Driver is stashed here for the update fn
// to drive: `begin_stitch` on BeginStitch effect, `finalize`/`cancel` on
// Finish/Cancel effects.
static DRIVER_SLOT: Mutex<Option<Driver>> = Mutex::new(None);

// One-shot capture for screenshot mode. The update fn reads this on the Finish
// effect (emitted immediately on a valid screenshot release) to crop and return
// the frozen image.
static ONE_SHOT_SLOT: Mutex<Option<rollshot_capture::OneShotCapture>> = Mutex::new(None);

// Active capture mode, set at startup by `acquire_resource`.
static CAPTURE_MODE: Mutex<Option<CaptureMode>> = Mutex::new(None);

#[to_layer_message]
#[derive(Debug, Clone)]
pub(crate) enum Message {
    Overlay(app::OverlayMessage),
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

        rx.map(|e| Message::Overlay(app::OverlayMessage::LiveEvent(e)))
    })
}

fn subscription(state: &Overlay) -> iced::Subscription<Message> {
    use crate::workspace::WorkspacePhase;
    let mut subs =
        vec![event::listen().map(|e| Message::Overlay(app::OverlayMessage::IcedEvent(e)))];
    if state.workspace.phase() == WorkspacePhase::ScrollingCapture {
        if PREVIEW_RX.lock().unwrap().is_some() {
            subs.push(preview_stream());
        }
        subs.push(
            iced::time::every(std::time::Duration::from_millis(250))
                .map(|_| Message::Overlay(app::OverlayMessage::Tick)),
        );
    }
    iced::Subscription::batch(subs)
}

/// After the layer surface opens, validate that a screenshot one-shot image is a
/// provable single-output match for the active surface (spec: non-KDE portal and
/// KWin captures must map to exactly the opened output). On mismatch — e.g. a
/// multi-monitor portal composite, or a layer surface that opened on a different
/// output than KWin captured — record an explicit mapping error and exit instead
/// of cropping against the wrong geometry. Returns `Some(exit_task)` on failure.
fn validate_screenshot_surface_or_exit(state: &Overlay) -> Option<Task<Message>> {
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
    // The capture's own physical/logical ratio is its scale: KWin reports it, and
    // the portal sets logical == physical (scale 1.0). Validating the physical
    // image against the opened surface's logical size at that scale rejects both
    // composites and wrong-output surfaces, while accepting an exact single
    // output (HiDPI portal images, whose scale the portal cannot prove, are
    // rejected as ambiguous — matching the spec's provable-single-output gate).
    let scale = target.physical_size.width as f64 / target.logical_region.width.max(1) as f64;
    match rollshot_capture::validate_surface_mapping(target.physical_size, overlay_logical, scale) {
        Ok(()) => None,
        Err(e) => {
            *RESULT_SLOT.lock().unwrap() = Some(Err(e.to_string()));
            Some(iced::exit())
        }
    }
}

fn perform_output_action(
    state: &mut Overlay,
    action: crate::workspace::OutputAction,
) -> Task<Message> {
    let result_guard = RESULT_SLOT.lock().unwrap();
    let result = match result_guard.as_ref() {
        Some(Ok(Some(result))) => result,
        _ => {
            state.transient_error = Some("No result available".to_string());
            return Task::none();
        }
    };
    let mut output_service = crate::output::ArboardOutput::new();
    let outcome = crate::output::perform_output(&mut output_service, action, &result.image);
    drop(result_guard);
    match crate::output::outcome_to_phase_decision(&outcome, state.workspace.phase()) {
        crate::output::WorkspaceTransition::Exit => iced::exit(),
        crate::output::WorkspaceTransition::StayInResultReview
        | crate::output::WorkspaceTransition::EnterResultReview => {
            if let crate::output::OutputOutcome::Error(err) = outcome {
                state.transient_error = Some(err);
            }
            Task::none()
        }
    }
}

fn update(state: &mut Overlay, message: Message) -> Task<Message> {
    match message {
        Message::Overlay(msg) => {
            let opened = matches!(
                &msg,
                app::OverlayMessage::IcedEvent(iced::Event::Window(
                    iced::window::Event::Opened { .. }
                )) | app::OverlayMessage::WindowOpened { .. }
            );
            let (effect, _region_mode) = app::update(state, msg);
            if opened {
                if let Some(exit) = validate_screenshot_surface_or_exit(state) {
                    return exit;
                }
            }
            let task = match effect {
                app::OverlayEffect::None => Task::none(),
                app::OverlayEffect::BeginStitch => {
                    let crop = state.crop.unwrap();
                    let ws = match state.window_size {
                        Some(ws) => ws,
                        None => {
                            state.transient_error =
                                Some("overlay surface size unknown".to_string());
                            return Task::none();
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
                    Task::none()
                }
                app::OverlayEffect::Finish => {
                    let mode = *CAPTURE_MODE.lock().unwrap();
                    if mode == Some(CaptureMode::Screenshot) {
                        let crop = state.crop.unwrap();
                        let ws = match state.window_size {
                            Some(ws) => ws,
                            None => {
                                *RESULT_SLOT.lock().unwrap() = Some(Err(
                                    "overlay surface size unknown (no Window::Opened event)"
                                        .to_string(),
                                ));
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
                        iced::exit()
                    } else {
                        let driver = DRIVER_SLOT.lock().unwrap().take();
                        let outcome = match driver {
                            Some(driver) => driver.finalize().map(Some),
                            None => Ok(None),
                        };
                        *RESULT_SLOT.lock().unwrap() = Some(outcome);
                        iced::exit()
                    }
                }
                app::OverlayEffect::Cancel => {
                    if let Some(driver) = DRIVER_SLOT.lock().unwrap().take() {
                        driver.cancel();
                    }
                    ONE_SHOT_SLOT.lock().unwrap().take();
                    *RESULT_SLOT.lock().unwrap() = Some(Ok(None));
                    iced::exit()
                }
                app::OverlayEffect::EnablePassthrough | app::OverlayEffect::DisablePassthrough => {
                    Task::none()
                }
                app::OverlayEffect::ActivateMode(new_mode) => {
                    // Stop/discard current workflow.
                    if let Some(driver) = DRIVER_SLOT.lock().unwrap().take() {
                        driver.cancel();
                    }
                    ONE_SHOT_SLOT.lock().unwrap().take();
                    *PREVIEW_RX.lock().unwrap() = None;

                    let config = OverlayConfig {
                        backend: "auto".to_string(),
                        fps: 5,
                        show_cursor: false,
                        initial_mode: new_mode,
                    };
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

                    match acquire_resource(new_mode, &config, &factories) {
                        Ok(Some(resource)) => {
                            *CAPTURE_MODE.lock().unwrap() = Some(new_mode);
                            match resource {
                                CaptureResource::Streaming(mut driver) => {
                                    if let (Some(crop), Some(ws)) = (state.crop, state.window_size)
                                    {
                                        state.workspace.begin_scrolling();
                                        driver.begin_stitch(
                                            LogicalRect {
                                                x: crop.x,
                                                y: crop.y,
                                                width: crop.width,
                                                height: crop.height,
                                            },
                                            rollshot_capture::Size {
                                                width: ws.width as u32,
                                                height: ws.height as u32,
                                            },
                                            app::preview_constraints(crop, ws),
                                        );
                                    }
                                    *DRIVER_SLOT.lock().unwrap() = Some(driver);
                                }
                                CaptureResource::OneShot(capture) => {
                                    let img = capture.image();
                                    state.frozen = Some(iced::widget::image::Handle::from_rgba(
                                        img.width(),
                                        img.height(),
                                        img.as_raw().clone(),
                                    ));
                                    *ONE_SHOT_SLOT.lock().unwrap() = Some(capture);
                                }
                            }
                            Task::none()
                        }
                        Ok(None) => {
                            // User cancelled portal picker.
                            state.transient_error = Some("Capture cancelled".to_string());
                            Task::none()
                        }
                        Err(e) => {
                            state.transient_error = Some(format!("Capture failed: {e}"));
                            Task::none()
                        }
                    }
                }
                app::OverlayEffect::PerformOutput(action) => perform_output_action(state, action),
                app::OverlayEffect::PrepareScreenshot(output) => {
                    let crop = state.crop.unwrap();
                    let ws = match state.window_size {
                        Some(ws) => ws,
                        None => {
                            state.transient_error =
                                Some("overlay surface size unknown".to_string());
                            return Task::none();
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
                    match outcome {
                        Ok(Some(result)) => {
                            let handle = crate::result_review::build_result_handle(&result.image);
                            let size = iced::Size::new(
                                result.image.width() as f32,
                                result.image.height() as f32,
                            );
                            state.result_handle = Some(handle);
                            state.result_size = Some(size);
                            state.workspace.enter_result_review();
                            *RESULT_SLOT.lock().unwrap() = Some(Ok(Some(result)));
                            output.map_or_else(Task::none, |action| {
                                perform_output_action(state, action)
                            })
                        }
                        Ok(None) => {
                            state.transient_error = Some("No capture available".to_string());
                            Task::none()
                        }
                        Err(e) => {
                            state.transient_error = Some(e);
                            Task::none()
                        }
                    }
                }
                app::OverlayEffect::FinalizeScrolling(output) => {
                    let driver = DRIVER_SLOT.lock().unwrap().take();
                    let outcome = match driver {
                        Some(driver) => driver.finalize(),
                        None => Err("No driver available".to_string()),
                    };
                    match outcome {
                        Ok(result) => {
                            let handle = crate::result_review::build_result_handle(&result.image);
                            let size = iced::Size::new(
                                result.image.width() as f32,
                                result.image.height() as f32,
                            );
                            state.result_handle = Some(handle);
                            state.result_size = Some(size);
                            state.workspace.enter_result_review();
                            *RESULT_SLOT.lock().unwrap() = Some(Ok(Some(result)));
                            output.map_or_else(Task::none, |action| {
                                perform_output_action(state, action)
                            })
                        }
                        Err(e) => {
                            state.transient_error = Some(e);
                            state.workspace.revert_to_scrolling();
                            Task::none()
                        }
                    }
                }
            };

            // Apply input region based on workspace phase.
            let input_task = match input_mode_for(state.workspace.phase()) {
                app::InputRegionMode::None => Task::none(),
                app::InputRegionMode::FullOverlay => {
                    let ws = state.window_size.unwrap_or(iced::Size::new(0.0, 0.0));
                    Task::batch([
                        Task::done(Message::KeyboardInteractivityChange(
                            KeyboardInteractivity::OnDemand,
                        )),
                        Task::done(Message::SetInputRegion(ActionCallback::new(
                            move |region| {
                                region.add(0, 0, ws.width as i32, ws.height as i32);
                            },
                        ))),
                    ])
                }
                app::InputRegionMode::ToolbarOnly => {
                    let region = input_region_for(state);
                    Task::batch([
                        Task::done(Message::KeyboardInteractivityChange(
                            KeyboardInteractivity::OnDemand,
                        )),
                        Task::done(Message::SetInputRegion(ActionCallback::new(
                            move |input_region| {
                                if let Some((x, y, w, h)) = region {
                                    input_region.add(x, y, w, h);
                                }
                            },
                        ))),
                    ])
                }
            };

            Task::batch([task, input_task])
        }
        _ => Task::none(),
    }
}

fn view(state: &Overlay) -> iced::Element<'_, Message> {
    crate::app::view(state).map(Message::Overlay)
}

fn input_region_for(state: &Overlay) -> Option<(i32, i32, i32, i32)> {
    use crate::workspace::WorkspacePhase;
    if state.workspace.phase() != WorkspacePhase::ScrollingCapture
        || !app::toolbar_is_visible(state)
    {
        return None;
    }
    let rect = app::toolbar_rect_for(state)?;
    Some((
        rect.x as i32,
        rect.y as i32,
        rect.width as i32,
        rect.height as i32,
    ))
}

#[allow(dead_code)]
fn input_mode_for(phase: crate::workspace::WorkspacePhase) -> app::InputRegionMode {
    use crate::workspace::WorkspacePhase;
    match phase {
        WorkspacePhase::ScrollingCapture => app::InputRegionMode::ToolbarOnly,
        _ => app::InputRegionMode::FullOverlay,
    }
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

pub fn run(config: OverlayConfig) -> Result<Option<CaptureResult>, OverlayError> {
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

    // Defense-in-depth: reject KWin captures that somehow succeeded without an
    // output name.  The backend should already enforce this, but the runner
    // must not silently downgrade to StartMode::Active.
    if let CaptureResource::OneShot(ref capture) = resource {
        if capture.target_display().output_name.is_none() {
            let kind = rollshot_capture::OneShotBackendKind::from_environment("auto")
                .map_err(|e| OverlayError::Capture(e.to_string()))?;
            if kind == rollshot_capture::OneShotBackendKind::LinuxKwin {
                return Err(OverlayError::Capture(
                    "KWin capture missing output name".to_string(),
                ));
            }
        }
    }

    *CAPTURE_MODE.lock().unwrap() = Some(config.initial_mode);

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

    let start_mode = match &resource {
        CaptureResource::OneShot(capture) => {
            match capture.target_display().output_name.as_deref() {
                Some(name) => StartMode::TargetScreen(name.to_string()),
                None => StartMode::Active,
            }
        }
        CaptureResource::Streaming(_) => StartMode::Active,
    };

    match resource {
        CaptureResource::Streaming(driver) => {
            *DRIVER_SLOT.lock().unwrap() = Some(driver);
        }
        CaptureResource::OneShot(capture) => {
            *ONE_SHOT_SLOT.lock().unwrap() = Some(capture);
        }
    }

    let run_result = application(
        move || Overlay {
            mode,
            frozen: frozen_handle.clone(),
            ..Overlay::default()
        },
        namespace,
        update,
        view,
    )
    .style(app::style)
    .subscription(subscription)
    .settings(Settings {
        layer_settings: LayerShellSettings {
            anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
            layer: Layer::Overlay,
            exclusive_zone: -1,
            size: None,
            margin: (0, 0, 0, 0),
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            start_mode,
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
    ONE_SHOT_SLOT.lock().unwrap().take();

    run_result.map_err(|e| OverlayError::Overlay(e.to_string()))?;

    // After the iced app exits cleanly, read the result slot.
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

    #[allow(clippy::type_complexity)]
    fn fake_streaming_factory(
        streaming_count: &'static AtomicUsize,
    ) -> Box<
        dyn Fn(
            &OverlayConfig,
            iced::futures::channel::mpsc::UnboundedSender<crate::driver::LiveOverlayEvent>,
        ) -> Result<Driver, String>,
    > {
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
    fn kwin_target_output_produces_target_screen_start_mode() {
        let config = test_config();
        let factories = ResourceFactories {
            streaming: Box::new(|_c, _p| Err("unused".to_string())),
            one_shot: Box::new(|_| {
                let img = RgbaImage::new(1920, 1080);
                Ok(rollshot_capture::OneShotCapture::new(
                    img,
                    DisplayTarget {
                        output_name: Some("DP-2".to_string()),
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
                .unwrap())
            }),
        };

        let result = acquire_resource(CaptureMode::Screenshot, &config, &factories)
            .unwrap()
            .unwrap();

        if let CaptureResource::OneShot(ref capture) = result {
            assert_eq!(
                capture.target_display().output_name.as_deref(),
                Some("DP-2")
            );
        } else {
            panic!("expected OneShot");
        }
    }

    #[test]
    fn kwin_capture_with_missing_output_name_is_rejected() {
        let config = test_config();
        let factories = ResourceFactories {
            streaming: Box::new(|_c, _p| Err("unused".to_string())),
            one_shot: Box::new(|_| {
                Err(CaptureError::Mapping {
                    message: "KWin returned empty screen name".to_string(),
                })
            }),
        };

        let result = acquire_resource(CaptureMode::Screenshot, &config, &factories);
        match result {
            Err(OverlayError::Capture(msg)) => {
                assert!(msg.contains("empty screen name"), "msg: {msg}");
            }
            other => panic!("expected Capture error, got {other:?}"),
        }
    }

    #[test]
    fn unresolved_portal_target_returns_one_shot_with_no_output_name() {
        let config = test_config();
        let factories = ResourceFactories {
            streaming: Box::new(|_c, _p| Err("unused".to_string())),
            one_shot: Box::new(|_| {
                let img = RgbaImage::new(1920, 1080);
                Ok(rollshot_capture::OneShotCapture::new(
                    img,
                    DisplayTarget {
                        output_name: None,
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
                .unwrap())
            }),
        };

        let result = acquire_resource(CaptureMode::Screenshot, &config, &factories)
            .unwrap()
            .unwrap();

        if let CaptureResource::OneShot(ref capture) = result {
            assert!(capture.target_display().output_name.is_none());
        } else {
            panic!("expected OneShot");
        }
    }

    #[test]
    fn portal_cancellation_returns_ok_none() {
        let config = test_config();
        let factories = ResourceFactories {
            streaming: Box::new(|_c, _p| Err("unused".to_string())),
            one_shot: Box::new(|_| Err(CaptureError::UserCancelled)),
        };

        let result = acquire_resource(CaptureMode::Screenshot, &config, &factories).unwrap();
        assert!(result.is_none(), "expected Ok(None) for cancellation");
    }

    #[test]
    fn screenshot_mode_creates_one_shot_not_driver() {
        let config = test_config();
        let factories = ResourceFactories {
            streaming: Box::new(|_c, _p| Err("unused".to_string())),
            one_shot: Box::new(|_| Ok(fake_one_shot_capture())),
        };

        let result = acquire_resource(CaptureMode::Screenshot, &config, &factories)
            .unwrap()
            .unwrap();

        assert!(matches!(result, CaptureResource::OneShot(_)));
    }

    fn one_shot_capture(
        physical: (u32, u32),
        logical: (u32, u32),
    ) -> rollshot_capture::OneShotCapture {
        let img = RgbaImage::new(physical.0, physical.1);
        rollshot_capture::OneShotCapture::new(
            img,
            DisplayTarget {
                output_name: None,
                logical_region: Region {
                    x: 0,
                    y: 0,
                    width: logical.0,
                    height: logical.1,
                },
                physical_size: Size {
                    width: physical.0,
                    height: physical.1,
                },
            },
        )
        .expect("test capture")
    }

    fn clear_screenshot_globals() {
        *CAPTURE_MODE.lock().unwrap() = None;
        *ONE_SHOT_SLOT.lock().unwrap() = None;
        *RESULT_SLOT.lock().unwrap() = None;
    }

    #[test]
    fn screenshot_surface_validation_accepts_exact_single_output() {
        let _guard = TEST_MUTEX.lock().unwrap();
        *CAPTURE_MODE.lock().unwrap() = Some(CaptureMode::Screenshot);
        *RESULT_SLOT.lock().unwrap() = None;
        // 1x portal image: physical == logical, overlay surface matches exactly.
        *ONE_SHOT_SLOT.lock().unwrap() = Some(one_shot_capture((200, 100), (200, 100)));

        let state = Overlay {
            window_size: Some(iced::Size::new(200.0, 100.0)),
            ..Overlay::default()
        };

        let exit = validate_screenshot_surface_or_exit(&state);
        assert!(exit.is_none(), "exact single output must pass");
        assert!(RESULT_SLOT.lock().unwrap().is_none());
        clear_screenshot_globals();
    }

    #[test]
    fn screenshot_surface_validation_rejects_multi_output_composite() {
        let _guard = TEST_MUTEX.lock().unwrap();
        *CAPTURE_MODE.lock().unwrap() = Some(CaptureMode::Screenshot);
        *RESULT_SLOT.lock().unwrap() = None;
        // Portal returned a two-output composite (400x100), but the layer surface
        // opened on a single 200x100 output.
        *ONE_SHOT_SLOT.lock().unwrap() = Some(one_shot_capture((400, 100), (400, 100)));

        let state = Overlay {
            window_size: Some(iced::Size::new(200.0, 100.0)),
            ..Overlay::default()
        };

        let exit = validate_screenshot_surface_or_exit(&state);
        assert!(exit.is_some(), "composite image must be rejected");
        assert!(
            RESULT_SLOT.lock().unwrap().as_ref().unwrap().is_err(),
            "composite rejection must record a mapping error"
        );
        clear_screenshot_globals();
    }

    #[test]
    fn surface_validation_skipped_in_scrolling_mode() {
        let _guard = TEST_MUTEX.lock().unwrap();
        *CAPTURE_MODE.lock().unwrap() = Some(CaptureMode::Scrolling);
        *RESULT_SLOT.lock().unwrap() = None;
        *ONE_SHOT_SLOT.lock().unwrap() = None;

        let state = Overlay {
            window_size: Some(iced::Size::new(200.0, 100.0)),
            ..Overlay::default()
        };

        assert!(validate_screenshot_surface_or_exit(&state).is_none());
        clear_screenshot_globals();
    }

    use crate::workspace::{CropRect, WorkspacePhase, WorkspaceState};
    use iced::Rectangle;

    fn scrolling_workspace() -> Overlay {
        let mut ws = WorkspaceState::new(CaptureMode::Scrolling);
        let crop = Rectangle {
            x: 10.0,
            y: 20.0,
            width: 260.0,
            height: 200.0,
        };
        let window_size = iced::Size::new(800.0, 600.0);
        ws.set_crop(Some(CropRect {
            x: crop.x,
            y: crop.y,
            width: crop.width,
            height: crop.height,
        }));
        ws.complete_selection();
        ws.begin_scrolling();
        Overlay {
            workspace: ws,
            crop: Some(crop),
            window_size: Some(window_size),
            ..Overlay::default()
        }
    }

    fn workspace_with_visible_toolbar() -> Overlay {
        let mut state = scrolling_workspace();
        state
            .workspace
            .auto_hide_mut()
            .accepted_frame(std::time::Instant::now() - std::time::Duration::from_millis(600));
        state
    }

    fn workspace_with_hidden_auto_hide() -> Overlay {
        let mut state = scrolling_workspace();
        let crop = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        state.crop = Some(crop);
        state.workspace.set_crop(Some(CropRect {
            x: crop.x,
            y: crop.y,
            width: crop.width,
            height: crop.height,
        }));
        state.workspace.begin_scrolling();
        state
    }

    #[test]
    fn scrolling_input_region_only_contains_visible_toolbar() {
        let state = workspace_with_visible_toolbar();
        let rect = app::toolbar_rect_for(&state).expect("toolbar rect");
        assert_eq!(
            input_region_for(&state),
            Some((
                rect.x as i32,
                rect.y as i32,
                rect.width as i32,
                rect.height as i32
            ))
        );
    }

    #[test]
    fn hidden_auto_hide_toolbar_has_no_input_region() {
        let state = workspace_with_hidden_auto_hide();
        assert_eq!(input_region_for(&state), None);
    }

    #[test]
    fn result_review_accepts_input_across_crop_and_toolbar() {
        assert_eq!(
            input_mode_for(WorkspacePhase::ResultReview),
            app::InputRegionMode::FullOverlay
        );
    }

    #[test]
    fn scrolling_uses_toolbar_only_input_and_selected_uses_full_overlay() {
        assert_eq!(
            input_mode_for(WorkspacePhase::ScrollingCapture),
            app::InputRegionMode::ToolbarOnly
        );
        assert_eq!(
            input_mode_for(WorkspacePhase::Selected),
            app::InputRegionMode::FullOverlay
        );
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
