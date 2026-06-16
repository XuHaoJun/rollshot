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
use crate::diagnostics::TARGET_OVERLAY;
use crate::driver::Driver;
use crate::CaptureResult;
use crate::OverlayConfig;
use crate::OverlayError;

use rollshot_capture::{CaptureRequest, CaptureScope, Workflow};

pub(crate) enum CaptureResource {
    Streaming {
        driver: Driver,
        frozen: Option<rollshot_capture::OneShotCapture>,
    },
    OneShot(rollshot_capture::OneShotCapture),
}

impl std::fmt::Debug for CaptureResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Streaming { frozen, .. } => f
                .debug_struct("Streaming")
                .field("frozen", &frozen.is_some())
                .finish(),
            Self::OneShot(c) => f.debug_tuple("OneShot").field(&c.target_display()).finish(),
        }
    }
}

fn start_mode_for(resource: &CaptureResource) -> StartMode {
    match resource {
        CaptureResource::Streaming { frozen, .. } => {
            match frozen
                .as_ref()
                .and_then(|c| c.target_display().output_name.clone())
            {
                Some(name) => StartMode::TargetScreen(name),
                None => StartMode::Active,
            }
        }
        CaptureResource::OneShot(capture) => {
            match capture.target_display().output_name.as_deref() {
                Some(name) => StartMode::TargetScreen(name.to_string()),
                None => StartMode::Active,
            }
        }
    }
}

fn frozen_handle_for(resource: &CaptureResource) -> Option<iced::widget::image::Handle> {
    match resource {
        CaptureResource::Streaming {
            frozen: Some(capture),
            ..
        } => {
            let img = capture.image();
            Some(iced::widget::image::Handle::from_rgba(
                img.width(),
                img.height(),
                img.as_raw().clone(),
            ))
        }
        _ => None,
    }
}

fn frozen_for_stream_backend(
    frozen: Option<rollshot_capture::OneShotCapture>,
    backend: &str,
) -> Option<rollshot_capture::OneShotCapture> {
    if backend == "linux-kwin" {
        frozen
    } else {
        None
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
    workflow: Workflow,
    config: &OverlayConfig,
    factories: &ResourceFactories,
) -> Result<Option<CaptureResource>, OverlayError> {
    match workflow {
        Workflow::Scrolling => acquire_scrolling_resource(config, factories),
        Workflow::Screenshot => {
            tracing::debug!(target: TARGET_OVERLAY, "acquiring one-shot capture resource");
            let capture = match (factories.one_shot)(config.show_cursor) {
                Ok(c) => c,
                Err(rollshot_capture::CaptureError::UserCancelled) => return Ok(None),
                Err(e) => return Err(OverlayError::Capture(e.to_string())),
            };
            Ok(Some(CaptureResource::OneShot(capture)))
        }
        Workflow::ActionGuide => {
            /* wired in Task 5 */
            acquire_scrolling_resource(config, factories)
        }
    }
}

fn acquire_scrolling_resource(
    config: &OverlayConfig,
    factories: &ResourceFactories,
) -> Result<Option<CaptureResource>, OverlayError> {
    let backend = config.backend.as_str();
    let needs_kwin_one_shot = backend == "auto" || backend == "linux-kwin";

    if needs_kwin_one_shot {
        tracing::debug!(target: TARGET_OVERLAY, "trying KWin one-shot for scrolling resource");
        let one_shot_result = (factories.one_shot)(config.show_cursor);

        match one_shot_result {
            Ok(capture) => {
                let output_name = capture.target_display().output_name.clone();
                tracing::debug!(target: TARGET_OVERLAY, ?output_name, "KWin one-shot succeeded");
                let mut resolved_config = config.clone();
                resolved_config.target_output_name = output_name;
                let (preview_tx, preview_rx) = iced::futures::channel::mpsc::unbounded();
                *PREVIEW_RX.lock().unwrap() = Some(preview_rx);
                let driver = (factories.streaming)(&resolved_config, preview_tx)
                    .map_err(OverlayError::Capture)?;
                let frozen = frozen_for_stream_backend(Some(capture), driver.capture_backend());
                Ok(Some(CaptureResource::Streaming { driver, frozen }))
            }
            Err(native_error) => {
                if backend == "auto"
                    && rollshot_capture::linux::auto::is_fallback_eligible(&native_error)
                {
                    tracing::debug!(
                        target: TARGET_OVERLAY,
                        %native_error,
                        "KWin one-shot failed with fallback-eligible error, falling back to portal"
                    );
                    let (preview_tx, preview_rx) = iced::futures::channel::mpsc::unbounded();
                    *PREVIEW_RX.lock().unwrap() = Some(preview_rx);
                    let mut portal_config = config.clone();
                    portal_config.backend = "linux-portal".to_string();
                    portal_config.target_output_name = None;
                    let driver = (factories.streaming)(&portal_config, preview_tx)
                        .map_err(OverlayError::Capture)?;
                    Ok(Some(CaptureResource::Streaming {
                        driver,
                        frozen: None,
                    }))
                } else {
                    tracing::error!(
                        target: TARGET_OVERLAY,
                        %native_error,
                        "KWin one-shot failed, no fallback available"
                    );
                    Err(OverlayError::Capture(native_error.to_string()))
                }
            }
        }
    } else {
        tracing::debug!(target: TARGET_OVERLAY, "acquiring portal streaming resource");
        let (preview_tx, preview_rx) = iced::futures::channel::mpsc::unbounded();
        *PREVIEW_RX.lock().unwrap() = Some(preview_rx);
        let driver = (factories.streaming)(config, preview_tx).map_err(OverlayError::Capture)?;
        Ok(Some(CaptureResource::Streaming {
            driver,
            frozen: None,
        }))
    }
}

static PREVIEW_RX: Mutex<
    Option<iced::futures::channel::mpsc::UnboundedReceiver<crate::driver::LiveOverlayEvent>>,
> = Mutex::new(None);
static RESULT_SLOT: Mutex<Option<Result<Option<CaptureResult>, String>>> = Mutex::new(None);

#[cfg(feature = "action-guide")]
#[allow(clippy::type_complexity)]
static ACTION_RESULT_SLOT: Mutex<
    Option<Result<Option<(rollshot_action::Recording, rollshot_action::InputCapability)>, String>>,
> = Mutex::new(None);
#[cfg(feature = "action-guide")]
static ACTION_REGION_SLOT: Mutex<Option<rollshot_action::CaptureRegion>> = Mutex::new(None);
#[cfg(feature = "action-guide")]
static ACTION_INPUT_SLOT: Mutex<Option<Box<dyn rollshot_action::SemanticInputSource>>> =
    Mutex::new(None);

// Capture starts in `run()` before the overlay surface exists, so the portal
// screen-share picker dialog appears + dismisses on a clean desktop and never
// lands in a captured frame. The live Driver is stashed here for the update fn
// to drive: `begin_stitch` on BeginStitch effect, `finalize`/`cancel` on
// FinishScrolling/Cancel effects.
static DRIVER_SLOT: Mutex<Option<Driver>> = Mutex::new(None);

// One-shot capture for region mode. The update fn reads this on the
// FinishRegion effect (emitted immediately on a valid region release) to
// crop and return the frozen image.
static ONE_SHOT_SLOT: Mutex<Option<rollshot_capture::OneShotCapture>> = Mutex::new(None);

// Active capture workflow, set at startup by `acquire_resource`.
static CAPTURE_WORKFLOW: Mutex<Option<Workflow>> = Mutex::new(None);

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
        let rx = PREVIEW_RX.lock().unwrap().take();
        match rx {
            Some(rx) => rx
                .map(|e| Message::Overlay(app::OverlayMessage::LiveEvent(e)))
                .boxed(),
            None => iced::futures::stream::pending().boxed(),
        }
    })
}

fn subscription(state: &Overlay) -> iced::Subscription<Message> {
    use crate::workspace::WorkspacePhase;
    let mut subs =
        vec![event::listen().map(|e| Message::Overlay(app::OverlayMessage::IcedEvent(e)))];
    if state.workspace.phase() == WorkspacePhase::ScrollingCapture {
        subs.push(preview_stream());
        subs.push(
            iced::time::every(std::time::Duration::from_millis(250))
                .map(|_| Message::Overlay(app::OverlayMessage::Tick)),
        );
    }
    iced::Subscription::batch(subs)
}

/// After the layer surface opens, validate that a region one-shot image is a
/// provable single-output match for the active surface (spec: non-KDE portal and
/// KWin captures must map to exactly the opened output). On mismatch — e.g. a
/// multi-monitor portal composite, or a layer surface that opened on a different
/// output than KWin captured — record an explicit mapping error and exit instead
/// of cropping against the wrong geometry. Returns `Some(exit_task)` on failure.
fn validate_region_surface_or_exit(state: &Overlay) -> Option<Task<Message>> {
    if *CAPTURE_WORKFLOW.lock().unwrap() != Some(Workflow::Screenshot) {
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
                if let Some(exit) = validate_region_surface_or_exit(state) {
                    return exit;
                }
            }
            let task = match effect {
                app::OverlayEffect::None => Task::none(),
                app::OverlayEffect::BeginStitch => {
                    tracing::info!(target: TARGET_OVERLAY, "begin stitch requested");
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
                app::OverlayEffect::FinishRegion => {
                    tracing::info!(target: TARGET_OVERLAY, "finishing region capture");
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
                    let outcome = match ONE_SHOT_SLOT.lock().unwrap().take() {
                        Some(capture) => {
                            crate::region::finish_region(&capture, crop_logical, overlay_logical)
                                .map(Some)
                        }
                        None => Ok(None),
                    };
                    match outcome {
                        Ok(opt) => {
                            *RESULT_SLOT.lock().unwrap() = Some(Ok(opt));
                            iced::exit()
                        }
                        Err(e) => {
                            tracing::error!(target: TARGET_OVERLAY, %e, "region finish failed");
                            state.transient_error = Some(e);
                            Task::none()
                        }
                    }
                }
                app::OverlayEffect::FinishScrolling => {
                    tracing::info!(target: TARGET_OVERLAY, "finishing scrolling capture");
                    let outcome = match DRIVER_SLOT.lock().unwrap().take() {
                        Some(driver) => driver.finalize().map(Some),
                        None => Err("No driver available".to_string()),
                    };
                    match outcome {
                        Ok(opt) => {
                            *RESULT_SLOT.lock().unwrap() = Some(Ok(opt));
                            iced::exit()
                        }
                        Err(e) => {
                            tracing::error!(target: TARGET_OVERLAY, %e, "scrolling finish failed");
                            state.transient_error = Some(e);
                            Task::none()
                        }
                    }
                }
                app::OverlayEffect::Cancel => {
                    tracing::info!(target: TARGET_OVERLAY, "overlay cancel");
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
                app::OverlayEffect::ActivateWorkflow(new_workflow) => {
                    tracing::info!(target: TARGET_OVERLAY, ?new_workflow, "activating capture workflow");
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
                        request: CaptureRequest {
                            workflow: new_workflow,
                            scope: CaptureScope::Region,
                        },
                        target_output_name: None,
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

                    match acquire_resource(new_workflow, &config, &factories) {
                        Ok(Some(resource)) => {
                            *CAPTURE_WORKFLOW.lock().unwrap() = Some(new_workflow);
                            state.frozen = frozen_handle_for(&resource);
                            match resource {
                                CaptureResource::Streaming {
                                    mut driver,
                                    frozen: _,
                                } => {
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
                #[cfg(feature = "action-guide")]
                app::OverlayEffect::StartRecording => {
                    tracing::info!(target: TARGET_OVERLAY, "start recording requested");
                    if let Some(driver) = DRIVER_SLOT.lock().unwrap().as_mut() {
                        let crop = state.crop.unwrap_or(iced::Rectangle {
                            x: 0.0,
                            y: 0.0,
                            width: 0.0,
                            height: 0.0,
                        });
                        let ws = state.window_size.unwrap_or(iced::Size::new(1920.0, 1080.0));
                        let source_size = driver.source_size();
                        let region = crate::coords::map_crop_to_frame(
                            crate::coords::LogicalRect {
                                x: crop.x,
                                y: crop.y,
                                width: crop.width,
                                height: crop.height,
                            },
                            rollshot_capture::Size {
                                width: ws.width as u32,
                                height: ws.height as u32,
                            },
                            source_size,
                        );
                        let action_region = rollshot_action::CaptureRegion {
                            x: region.x,
                            y: region.y,
                            width: region.width,
                            height: region.height,
                        };
                        *ACTION_REGION_SLOT.lock().unwrap() = Some(action_region);
                        let source =
                            ACTION_INPUT_SLOT.lock().unwrap().take().unwrap_or_else(|| {
                                Box::new(rollshot_action::VisualOnlySource::new(
                                    rollshot_action::DegradedReason::SourceStartFailed,
                                ))
                            });
                        driver.begin_action_recording(action_region, source);
                    }
                    state.recording_started = Some(std::time::Instant::now());
                    Task::none()
                }
                #[cfg(feature = "action-guide")]
                app::OverlayEffect::FinishRecording => {
                    tracing::info!(target: TARGET_OVERLAY, "finish recording requested");
                    let outcome = match DRIVER_SLOT.lock().unwrap().take() {
                        Some(driver) => driver.finalize_action().map(Some),
                        None => Err("no driver for action recording".to_string()),
                    };
                    *ACTION_RESULT_SLOT.lock().unwrap() = Some(outcome);
                    state.recording_started = None;
                    state.recording_capability = None;
                    iced::exit()
                }
                #[cfg(not(feature = "action-guide"))]
                app::OverlayEffect::StartRecording => {
                    tracing::info!(
                        target: TARGET_OVERLAY,
                        "start recording requested (no action-guide feature)"
                    );
                    Task::none()
                }
                #[cfg(not(feature = "action-guide"))]
                app::OverlayEffect::FinishRecording => {
                    tracing::info!(
                        target: TARGET_OVERLAY,
                        "finish recording requested (no action-guide feature)"
                    );
                    Task::none()
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
            Driver::start_capture(
                &cfg.backend,
                cfg.fps,
                cfg.show_cursor,
                None,
                cfg.target_output_name.clone(),
                preview_tx,
            )
        }),
        one_shot: Box::new(|show_cursor| {
            let kind = rollshot_capture::OneShotBackendKind::from_environment("auto")?;
            kind.capture_once(show_cursor)
        }),
    }
}

fn run_initial_path<Direct, Overlay>(
    config: OverlayConfig,
    direct: Direct,
    overlay: Overlay,
) -> Result<Option<CaptureResult>, OverlayError>
where
    Direct: FnOnce(&OverlayConfig) -> Result<Option<CaptureResult>, OverlayError>,
    Overlay: FnOnce(OverlayConfig) -> Result<Option<CaptureResult>, OverlayError>,
{
    if !config.request.is_supported() {
        return Err(OverlayError::Capture(
            "unsupported capture request".to_string(),
        ));
    }
    if config.request.scope == CaptureScope::Fullscreen {
        return direct(&config);
    }
    overlay(config)
}

pub fn run(config: OverlayConfig) -> Result<Option<CaptureResult>, OverlayError> {
    run_initial_path(config, crate::fullscreen::capture, run_overlay_session)
}

#[cfg(feature = "action-guide")]
pub fn run_action_guide(
    config: OverlayConfig,
    input_source: Box<dyn rollshot_action::SemanticInputSource>,
) -> Result<
    Option<(
        rollshot_action::Recording,
        rollshot_action::InputCapability,
        rollshot_action::CaptureRegion,
    )>,
    OverlayError,
> {
    *ACTION_INPUT_SLOT.lock().unwrap() = Some(input_source);
    *ACTION_REGION_SLOT.lock().unwrap() = None;
    *ACTION_RESULT_SLOT.lock().unwrap() = None;
    run(config)?;
    let result = ACTION_RESULT_SLOT
        .lock()
        .unwrap()
        .take()
        .unwrap_or(Ok(None))
        .map_err(OverlayError::Capture)?;
    let region =
        ACTION_REGION_SLOT
            .lock()
            .unwrap()
            .take()
            .unwrap_or(rollshot_action::CaptureRegion {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            });
    Ok(result.map(|(recording, capability)| (recording, capability, region)))
}

fn run_overlay_session(config: OverlayConfig) -> Result<Option<CaptureResult>, OverlayError> {
    if !config.request.is_supported() {
        return Err(OverlayError::Capture(
            "unsupported capture request".to_string(),
        ));
    }
    if config.request.scope == CaptureScope::Fullscreen {
        return Err(OverlayError::Capture(
            "fullscreen must not reach the overlay runner".to_string(),
        ));
    }

    tracing::info!(target: TARGET_OVERLAY, request = ?config.request, "blocking overlay starting");
    *PREVIEW_RX.lock().unwrap() = None;
    *DRIVER_SLOT.lock().unwrap() = None;
    *ONE_SHOT_SLOT.lock().unwrap() = None;
    *RESULT_SLOT.lock().unwrap() = None;
    *CAPTURE_WORKFLOW.lock().unwrap() = None;
    #[cfg(feature = "action-guide")]
    {
        *ACTION_RESULT_SLOT.lock().unwrap() = None;
        *ACTION_REGION_SLOT.lock().unwrap() = None;
        *ACTION_INPUT_SLOT.lock().unwrap() = None;
    }

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

    let resource = acquire_resource(config.request.workflow, &config, &factories)?;
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

    *CAPTURE_WORKFLOW.lock().unwrap() = Some(config.request.workflow);

    // Build the frozen background handle once. This is the single full-image
    // copy in the two-buffer render model; `view()` clones only the cheap
    // handle per redraw.
    let frozen_handle = frozen_handle_for(&resource);
    let workflow = config.request.workflow;

    let start_mode = start_mode_for(&resource);

    match resource {
        CaptureResource::Streaming { driver, .. } => {
            *DRIVER_SLOT.lock().unwrap() = Some(driver);
        }
        CaptureResource::OneShot(capture) => {
            *ONE_SHOT_SLOT.lock().unwrap() = Some(capture);
        }
    }

    let run_result = application(
        move || Overlay {
            workflow,
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
        tracing::warn!(target: TARGET_OVERLAY, "safety-net teardown of leaked driver");
        driver.cancel();
    }
    ONE_SHOT_SLOT.lock().unwrap().take();

    run_result.map_err(|e| {
        tracing::error!(target: TARGET_OVERLAY, %e, "overlay loop error");
        OverlayError::Overlay(e.to_string())
    })?;

    // After the iced app exits cleanly, read the result slot.
    match RESULT_SLOT.lock().unwrap().take().unwrap_or(Ok(None)) {
        Ok(opt) => {
            if opt.is_some() {
                tracing::info!(target: TARGET_OVERLAY, "overlay completed with result");
            } else {
                tracing::info!(target: TARGET_OVERLAY, "overlay cancelled (no result)");
            }
            Ok(opt)
        }
        Err(e) => {
            tracing::error!(target: TARGET_OVERLAY, %e, "overlay result error");
            Err(OverlayError::Capture(e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;
    use rollshot_capture::one_shot::DisplayTarget;
    use rollshot_capture::{CaptureError, Region, Size};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::sync::Mutex;

    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    fn test_config() -> OverlayConfig {
        OverlayConfig {
            backend: "auto".to_string(),
            fps: 5,
            show_cursor: false,
            request: CaptureRequest::scrolling_region(),
            target_output_name: None,
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

        let config = OverlayConfig {
            backend: "linux-portal".to_string(),
            ..test_config()
        };
        let factories = ResourceFactories {
            streaming: fake_streaming_factory(&STREAMING_COUNT),
            one_shot: fake_one_shot_factory(&ONE_SHOT_COUNT),
        };

        let _ = acquire_resource(Workflow::Scrolling, &config, &factories);

        assert_eq!(STREAMING_COUNT.load(Ordering::SeqCst), 1);
        assert_eq!(ONE_SHOT_COUNT.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn auto_scrolling_calls_both_factories() {
        let _guard = TEST_MUTEX.lock().unwrap();
        static STREAMING_COUNT: AtomicUsize = AtomicUsize::new(0);
        static ONE_SHOT_COUNT: AtomicUsize = AtomicUsize::new(0);

        let config = test_config();
        let factories = ResourceFactories {
            streaming: fake_streaming_factory(&STREAMING_COUNT),
            one_shot: fake_one_shot_factory(&ONE_SHOT_COUNT),
        };

        let _ = acquire_resource(Workflow::Scrolling, &config, &factories);

        assert_eq!(ONE_SHOT_COUNT.load(Ordering::SeqCst), 1);
        assert_eq!(STREAMING_COUNT.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn region_calls_only_one_shot_factory() {
        static STREAMING_COUNT: AtomicUsize = AtomicUsize::new(0);
        static ONE_SHOT_COUNT: AtomicUsize = AtomicUsize::new(0);

        let config = test_config();
        let factories = ResourceFactories {
            streaming: fake_streaming_factory(&STREAMING_COUNT),
            one_shot: fake_one_shot_factory(&ONE_SHOT_COUNT),
        };

        let result = acquire_resource(Workflow::Screenshot, &config, &factories);
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
        let result_1 = acquire_resource(Workflow::Screenshot, &config, &factories_1)
            .unwrap()
            .unwrap();
        drop(result_1);

        let factories_2 = ResourceFactories {
            streaming: Box::new(|_c, _p| Err("second".to_string())),
            one_shot: Box::new(|_| Ok(fake_one_shot_capture())),
        };
        let result_2 = acquire_resource(Workflow::Screenshot, &config, &factories_2)
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

        let result = acquire_resource(Workflow::Screenshot, &config, &factories)
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

        let result = acquire_resource(Workflow::Screenshot, &config, &factories);
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

        let result = acquire_resource(Workflow::Screenshot, &config, &factories)
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

        let result = acquire_resource(Workflow::Screenshot, &config, &factories).unwrap();
        assert!(result.is_none(), "expected Ok(None) for cancellation");
    }

    #[test]
    fn region_mode_creates_one_shot_not_driver() {
        let config = test_config();
        let factories = ResourceFactories {
            streaming: Box::new(|_c, _p| Err("unused".to_string())),
            one_shot: Box::new(|_| Ok(fake_one_shot_capture())),
        };

        let result = acquire_resource(Workflow::Screenshot, &config, &factories)
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

    fn clear_region_globals() {
        *CAPTURE_WORKFLOW.lock().unwrap() = None;
        *ONE_SHOT_SLOT.lock().unwrap() = None;
        *RESULT_SLOT.lock().unwrap() = None;
    }

    #[test]
    fn region_surface_validation_accepts_exact_single_output() {
        let _guard = TEST_MUTEX.lock().unwrap();
        *CAPTURE_WORKFLOW.lock().unwrap() = Some(Workflow::Screenshot);
        *RESULT_SLOT.lock().unwrap() = None;
        // 1x portal image: physical == logical, overlay surface matches exactly.
        *ONE_SHOT_SLOT.lock().unwrap() = Some(one_shot_capture((200, 100), (200, 100)));

        let state = Overlay {
            window_size: Some(iced::Size::new(200.0, 100.0)),
            ..Overlay::default()
        };

        let exit = validate_region_surface_or_exit(&state);
        assert!(exit.is_none(), "exact single output must pass");
        assert!(RESULT_SLOT.lock().unwrap().is_none());
        clear_region_globals();
    }

    #[test]
    fn region_surface_validation_rejects_multi_output_composite() {
        let _guard = TEST_MUTEX.lock().unwrap();
        *CAPTURE_WORKFLOW.lock().unwrap() = Some(Workflow::Screenshot);
        *RESULT_SLOT.lock().unwrap() = None;
        // Portal returned a two-output composite (400x100), but the layer surface
        // opened on a single 200x100 output.
        *ONE_SHOT_SLOT.lock().unwrap() = Some(one_shot_capture((400, 100), (400, 100)));

        let state = Overlay {
            window_size: Some(iced::Size::new(200.0, 100.0)),
            ..Overlay::default()
        };

        let exit = validate_region_surface_or_exit(&state);
        assert!(exit.is_some(), "composite image must be rejected");
        assert!(
            RESULT_SLOT.lock().unwrap().as_ref().unwrap().is_err(),
            "composite rejection must record a mapping error"
        );
        clear_region_globals();
    }

    #[test]
    fn surface_validation_skipped_in_scrolling_mode() {
        let _guard = TEST_MUTEX.lock().unwrap();
        *CAPTURE_WORKFLOW.lock().unwrap() = Some(Workflow::Scrolling);
        *RESULT_SLOT.lock().unwrap() = None;
        *ONE_SHOT_SLOT.lock().unwrap() = None;

        let state = Overlay {
            window_size: Some(iced::Size::new(200.0, 100.0)),
            ..Overlay::default()
        };

        assert!(validate_region_surface_or_exit(&state).is_none());
        clear_region_globals();
    }

    use crate::workspace::{CropRect, WorkspacePhase, WorkspaceState};
    use iced::Rectangle;

    fn scrolling_workspace() -> Overlay {
        let mut ws = WorkspaceState::new(Workflow::Scrolling);
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
    fn linux_update_routes_cursor_drag_to_toolbar_without_translation() {
        use crate::workspace::{CropRect, ToolbarPosition};
        use iced::{mouse, Event, Point};

        // Selected phase uses a full-overlay input region, so the surface
        // receives cursor motion across its whole extent during a drag. Pin the
        // toolbar to a known rect so `toolbar_rect_for` is deterministic.
        let start = CropRect {
            x: 100.0,
            y: 100.0,
            width: crate::toolbar::TOOLBAR_WIDTH,
            height: crate::toolbar::TOOLBAR_HEIGHT,
        };
        let mut state = Overlay {
            window_size: Some(iced::Size::new(800.0, 600.0)),
            toolbar_position: ToolbarPosition::Manual(start),
            cursor_position: Some(Point::new(110.0, 110.0)),
            ..Overlay::default()
        };

        // Grab the toolbar 10px in from its top-left, then move the cursor. The
        // Linux runner forwards the raw `CursorMoved` to `app::update` untouched,
        // so the toolbar must track the cursor (cursor - grab), not snap to 0,0.
        let _ = update(&mut state, Message::Overlay(app::OverlayMessage::DragStart));
        let _ = update(
            &mut state,
            Message::Overlay(app::OverlayMessage::IcedEvent(Event::Mouse(
                mouse::Event::CursorMoved {
                    position: Point::new(300.0, 250.0),
                },
            ))),
        );

        match state.toolbar_position {
            ToolbarPosition::Manual(rect) => assert_eq!((rect.x, rect.y), (290.0, 240.0)),
            other => panic!("expected manual toolbar position, got {other:?}"),
        }
    }

    #[test]
    fn region_mode_does_not_consume_preview_receiver() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let config = test_config();
        let factories = ResourceFactories {
            streaming: Box::new(|_c, _p| Err("unused".to_string())),
            one_shot: Box::new(|_| Ok(fake_one_shot_capture())),
        };

        *PREVIEW_RX.lock().unwrap() = None;

        let _result = acquire_resource(Workflow::Screenshot, &config, &factories).unwrap();

        assert!(
            PREVIEW_RX.lock().unwrap().is_none(),
            "region mode should not set up preview channel"
        );
    }

    // ── Task 6: Scrolling resource with frozen background tests ──

    fn fake_one_shot_capture_for(output_name: &str) -> rollshot_capture::OneShotCapture {
        let img = RgbaImage::new(1920, 1080);
        rollshot_capture::OneShotCapture::new(
            img,
            DisplayTarget {
                output_name: Some(output_name.to_string()),
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

    fn auto_config() -> OverlayConfig {
        OverlayConfig {
            backend: "auto".to_string(),
            fps: 5,
            show_cursor: false,
            request: CaptureRequest::scrolling_region(),
            target_output_name: None,
        }
    }

    fn kwin_config() -> OverlayConfig {
        OverlayConfig {
            backend: "linux-kwin".to_string(),
            target_output_name: None,
            fps: 5,
            show_cursor: false,
            request: CaptureRequest::scrolling_region(),
        }
    }

    fn factories_with_successful_kwin_one_shot_and_driver() -> ResourceFactories {
        ResourceFactories {
            streaming: Box::new(|_cfg, _preview_tx| Err("fake streaming driver".to_string())),
            one_shot: Box::new(|_| Ok(fake_one_shot_capture_for("DP-2"))),
        }
    }

    fn factories_with_failed_kwin_one_shot_and_portal_driver() -> ResourceFactories {
        ResourceFactories {
            streaming: Box::new(
                |_cfg, _preview_tx| Err("fake portal streaming driver".to_string()),
            ),
            one_shot: Box::new(|_| {
                Err(CaptureError::Unsupported {
                    message: "KWin one-shot failed".to_string(),
                })
            }),
        }
    }

    #[test]
    fn kwin_scrolling_resource_targets_frozen_output() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let result = acquire_scrolling_resource(
            &auto_config(),
            &factories_with_successful_kwin_one_shot_and_driver(),
        );
        // Streaming factory returns Err, but the one-shot succeeded first.
        // The error is from the streaming factory, not the one-shot.
        match result {
            Err(OverlayError::Capture(msg)) => {
                assert!(msg.contains("fake streaming driver"), "msg: {msg}");
            }
            other => panic!("expected Capture error from streaming factory, got {other:?}"),
        }
    }

    #[test]
    fn portal_scrolling_resource_uses_active_start_mode_without_frozen_image() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let config = OverlayConfig {
            backend: "linux-portal".to_string(),
            fps: 5,
            show_cursor: false,
            request: CaptureRequest::scrolling_region(),
            target_output_name: None,
        };
        let factories = ResourceFactories {
            streaming: Box::new(
                |_cfg, _preview_tx| Err("fake portal streaming driver".to_string()),
            ),
            one_shot: Box::new(|_| {
                Err(CaptureError::Unsupported {
                    message: "not used for portal".to_string(),
                })
            }),
        };
        let result = acquire_scrolling_resource(&config, &factories);
        // Portal backend skips one-shot and goes straight to streaming.
        // Streaming factory returns Err, so we get a capture error.
        match result {
            Err(OverlayError::Capture(msg)) => {
                assert!(msg.contains("fake portal streaming driver"), "msg: {msg}");
            }
            other => panic!("expected Capture error from streaming factory, got {other:?}"),
        }
    }

    #[test]
    fn auto_kwin_one_shot_failure_uses_portal_stream_without_frozen_image() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let seen_backend = Arc::new(Mutex::new(None));
        let seen_backend_for_factory = Arc::clone(&seen_backend);
        let factories = ResourceFactories {
            streaming: Box::new(move |cfg, _preview_tx| {
                *seen_backend_for_factory.lock().unwrap() = Some(cfg.backend.clone());
                Err("fake portal streaming driver".to_string())
            }),
            one_shot: Box::new(|_| {
                Err(CaptureError::Unsupported {
                    message: "KWin one-shot failed".to_string(),
                })
            }),
        };
        let result = acquire_scrolling_resource(&auto_config(), &factories);
        // The streaming factory also fails, so we get an error from the
        // portal fallback path. The key behavior: no error from the one-shot.
        match result {
            Err(OverlayError::Capture(msg)) => {
                assert!(
                    msg.contains("fake portal streaming driver"),
                    "expected portal fallback error, got: {msg}"
                );
            }
            other => panic!("expected Capture error from portal fallback, got {other:?}"),
        }
        assert_eq!(
            seen_backend.lock().unwrap().as_deref(),
            Some("linux-portal")
        );
    }

    #[test]
    fn explicit_kwin_one_shot_failure_returns_error_without_portal() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let result = acquire_scrolling_resource(
            &kwin_config(),
            &factories_with_failed_kwin_one_shot_and_portal_driver(),
        );
        assert!(result.is_err());
        match result {
            Err(OverlayError::Capture(msg)) => {
                assert!(
                    msg.contains("KWin one-shot failed"),
                    "expected KWin error, got: {msg}"
                );
            }
            other => panic!("expected Capture error, got {other:?}"),
        }
    }

    #[test]
    fn frozen_handle_exists_only_for_kwin_streaming_resources() {
        let _guard = TEST_MUTEX.lock().unwrap();
        // KWin one-shot succeeds but streaming factory fails — test that the
        // one-shot was attempted and would have been used as frozen.
        let factories = factories_with_successful_kwin_one_shot_and_driver();
        let one_shot_result = (factories.one_shot)(false);
        assert!(one_shot_result.is_ok());
        let capture = one_shot_result.unwrap();
        assert_eq!(
            capture.target_display().output_name.as_deref(),
            Some("DP-2")
        );

        // Frozen handle from one-shot image.
        let img = capture.image();
        let handle =
            iced::widget::image::Handle::from_rgba(img.width(), img.height(), img.as_raw().clone());
        match handle {
            iced::widget::image::Handle::Rgba { width, height, .. } => {
                assert_eq!(width, 1920);
                assert_eq!(height, 1080);
            }
            other => panic!("expected Rgba handle, got {other:?}"),
        }
    }

    #[test]
    fn portal_fallback_discards_kwin_frozen_capture() {
        assert!(
            frozen_for_stream_backend(Some(fake_one_shot_capture_for("DP-2")), "linux-portal")
                .is_none()
        );
    }

    #[test]
    fn native_stream_keeps_kwin_frozen_capture() {
        assert!(
            frozen_for_stream_backend(Some(fake_one_shot_capture_for("DP-2")), "linux-kwin")
                .is_some()
        );
    }

    #[test]
    fn fullscreen_routes_to_direct_capture_before_overlay_startup() {
        let mut config = test_config();
        config.request = CaptureRequest::screenshot_fullscreen();
        let direct_calls = std::cell::Cell::new(0);
        let overlay_calls = std::cell::Cell::new(0);

        let result = run_initial_path(
            config,
            |_| {
                direct_calls.set(direct_calls.get() + 1);
                Ok(Some(CaptureResult {
                    image: RgbaImage::new(2, 2),
                    stats: None,
                }))
            },
            |_| {
                overlay_calls.set(overlay_calls.get() + 1);
                Ok(None)
            },
        )
        .unwrap();

        assert!(result.is_some());
        assert_eq!(direct_calls.get(), 1);
        assert_eq!(overlay_calls.get(), 0);
    }

    #[test]
    fn unsupported_request_is_rejected_before_routing() {
        let mut config = test_config();
        config.request = CaptureRequest {
            workflow: Workflow::Scrolling,
            scope: CaptureScope::Fullscreen,
        };
        let result = run_initial_path(
            config,
            |_| panic!("direct should not be called"),
            |_| panic!("overlay should not be called"),
        );
        assert!(result.is_err());
        match result {
            Err(OverlayError::Capture(msg)) => {
                assert!(msg.contains("unsupported"), "msg: {msg}");
            }
            other => panic!("expected Capture error, got {other:?}"),
        }
    }
}
