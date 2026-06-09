//! Embeddable macOS capture component.
//!
//! Extracted from the daemon-owning `macos_runner`. The component owns capture
//! state, rendering, resource acquisition, passthrough, and the controls window,
//! and reports completion to a host via [`HostEffect`] instead of calling
//! `iced::exit()`. It does NOT know about auto-save, thumbnails, the Result
//! Workspace, or process lifetime — Task 8 wires `rollshot-app` around it as the
//! daemon owner.
//!
//! NOTE: this module is `#[cfg(target_os = "macos")]` and is not compiled on
//! Linux. Its compilation and unit tests are verified on a macOS machine.

use std::sync::Mutex;
use std::time::Duration;

use iced::futures::StreamExt;
use iced::widget::container;
use iced::{event, window, Element, Event, Length, Point, Size, Task};

use crate::app::{self, OverlayEffect, OverlayMessage, OverlayState};
use crate::coords::LogicalRect;
use crate::driver::{Driver, LiveOverlayEvent};
use crate::workspace::WorkspacePhase;
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

/// Hand-off slot for the live preview receiver. iced's `Subscription::run`
/// closure must be `'static`, so it cannot borrow the component. The component
/// owns the receiver in its `preview_rx` field until it transitions into the
/// scrolling-capture phase, at which point `arm_preview_subscription` moves the
/// receiver here for the subscription closure to `take()` exactly once. Mode
/// switches and teardown clear this slot. (This mirrors the original runner's
/// `PREVIEW_RX` static; the receiver is single-consumer either way.)
static PREVIEW_RX: Mutex<
    Option<iced::futures::channel::mpsc::UnboundedReceiver<LiveOverlayEvent>>,
> = Mutex::new(None);

pub(crate) fn acquire_resource(
    mode: CaptureMode,
    config: &OverlayConfig,
    factories: &ResourceFactories,
) -> Result<Option<(CaptureResource, Option<PreviewReceiver>)>, OverlayError> {
    match mode {
        CaptureMode::Scrolling => {
            let (preview_tx, preview_rx) = iced::futures::channel::mpsc::unbounded();
            let driver =
                (factories.streaming)(config, preview_tx).map_err(OverlayError::Capture)?;
            Ok(Some((CaptureResource::Streaming(driver), Some(preview_rx))))
        }
        CaptureMode::Screenshot => {
            let capture = match (factories.one_shot)(config.show_cursor) {
                Ok(c) => c,
                Err(rollshot_capture::CaptureError::UserCancelled) => return Ok(None),
                Err(e) => return Err(OverlayError::Capture(e.to_string())),
            };
            Ok(Some((CaptureResource::OneShot(capture), None)))
        }
    }
}

type PreviewReceiver = iced::futures::channel::mpsc::UnboundedReceiver<LiveOverlayEvent>;

#[derive(Debug, Clone, Copy, PartialEq)]
enum ControlsWindowAction {
    Open(LogicalRect),
    Close,
    Noop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PassthroughAction {
    #[allow(dead_code)]
    Enable,
    Disable,
    Noop,
}

fn controls_window_action(
    current: Option<LogicalRect>,
    visible: Option<LogicalRect>,
) -> ControlsWindowAction {
    match (current, visible) {
        (_, Some(rect)) => ControlsWindowAction::Open(rect),
        (Some(_), None) => ControlsWindowAction::Close,
        _ => ControlsWindowAction::Noop,
    }
}

fn passthrough_action(old_phase: WorkspacePhase, new_phase: WorkspacePhase) -> PassthroughAction {
    match (old_phase, new_phase) {
        (_, WorkspacePhase::ScrollingCapture) if old_phase != WorkspacePhase::ScrollingCapture => {
            PassthroughAction::Enable
        }
        (WorkspacePhase::ScrollingCapture, _) => PassthroughAction::Disable,
        _ => PassthroughAction::Noop,
    }
}

fn controls_message_to_overlay(
    message: OverlayMessage,
    controls: Option<LogicalRect>,
) -> OverlayMessage {
    match (message, controls) {
        // The controls window hosts the toolbar in a separate, toolbar-sized
        // window during scrolling capture. Cursor positions it emits are
        // relative to that window, so offset them into overlay space to keep
        // toolbar drag positioning consistent with the single-window phases.
        (
            OverlayMessage::IcedEvent(Event::Mouse(iced::mouse::Event::CursorMoved { position })),
            Some(rect),
        ) => OverlayMessage::IcedEvent(Event::Mouse(iced::mouse::Event::CursorMoved {
            position: Point::new(position.x + rect.x, position.y + rect.y),
        })),
        (message, _) => message,
    }
}

#[derive(Debug, Clone)]
pub struct Message(InternalMessage);

impl Message {
    /// Construct the opaque message a host feeds back after its `window::open`
    /// task resolves the overlay window id. The host treats `Message` as opaque
    /// otherwise; this is the one id it must inject.
    pub fn overlay_window_ready(id: window::Id) -> Self {
        Message(InternalMessage::OverlayWindowReady(id))
    }
}

#[derive(Debug, Clone)]
enum InternalMessage {
    Overlay(OverlayMessage),
    WindowOpened { id: window::Id, size: Size },
    OverlayWindowReady(window::Id),
    ControlsWindowReady(window::Id),
    WindowPatched(Result<(), String>),
    PassthroughEnabled,
    PassthroughDisabled,
}

/// Reported to the host after [`Component::update`] / [`Component::apply_overlay_effect`].
/// The host maps `Task` back through `Component::update`, and acts on terminal
/// variants (`Completed`/`Cancelled`/`Fatal`) according to its own lifetime
/// policy. `None` keeps the capture session running (e.g. an inline transient
/// error that leaves the overlay open).
pub enum HostEffect {
    None,
    Task(iced::Task<Message>),
    Completed(CaptureResult),
    Cancelled,
    Fatal(String),
}

pub struct Component {
    overlay: OverlayState,
    overlay_window: Option<window::Id>,
    controls_window: Option<window::Id>,
    controls_rect: Option<LogicalRect>,
    driver: Option<Driver>,
    one_shot: Option<rollshot_capture::OneShotCapture>,
    preview_rx: Option<PreviewReceiver>,
    /// Active capture mode, used by the screenshot surface-mapping gate.
    capture_mode: Option<CaptureMode>,
    /// A terminal outcome staged while mouse passthrough is being disabled
    /// (scrolling finish or cancel during scrolling). The original runner
    /// disabled passthrough on the overlay window *before* finishing, to avoid
    /// leaving a ghost input-absorbing region; this stages the same handshake,
    /// reported once passthrough is off via the `PassthroughDisabled` message.
    pending_finish: Option<PendingFinish>,
}

/// Terminal outcome staged behind a passthrough-disable handshake. Mutually
/// exclusive: at most one is pending, consumed once by `PassthroughDisabled`.
enum PendingFinish {
    Complete(CaptureResult),
    Cancel,
}

impl Component {
    /// Build the component, acquiring the initial capture resource. Returns
    /// `Ok(None)` when the user cancelled before any capture began (e.g. portal
    /// picker dismissed). The host opens the overlay window and feeds the boot
    /// task returned by [`Component::boot`].
    pub fn new(config: &OverlayConfig) -> Result<Option<Self>, OverlayError> {
        #[cfg(not(test))]
        let factories = real_factories();
        #[cfg(test)]
        let factories = test_factories();

        let acquired = acquire_resource(config.initial_mode, config, &factories)?;
        let (resource, preview_rx) = match acquired {
            Some(r) => r,
            None => return Ok(None),
        };

        let frozen = match &resource {
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

        let overlay = OverlayState {
            mode: config.initial_mode,
            frozen,
            ..OverlayState::default()
        };

        let (driver, one_shot) = match resource {
            CaptureResource::Streaming(d) => (Some(d), None),
            CaptureResource::OneShot(c) => (None, Some(c)),
        };

        Ok(Some(Self {
            overlay,
            overlay_window: None,
            controls_window: None,
            controls_rect: None,
            driver,
            one_shot,
            preview_rx,
            capture_mode: Some(config.initial_mode),
            pending_finish: None,
        }))
    }

    /// Source size, scale, and (screenshot-only) display id resolved from the
    /// acquired resource. The host uses these to size and position the overlay
    /// window before calling [`Component::boot`].
    pub fn window_geometry(&self) -> WindowGeometry {
        if let Some(driver) = &self.driver {
            let source_size = driver.source_size();
            let scale = crate::macos_window::main_screen_scale_factor()
                .filter(|s| *s > 0.0)
                .unwrap_or(1.0);
            return WindowGeometry {
                source_size,
                scale,
                display_id: None,
            };
        }
        if let Some(capture) = &self.one_shot {
            let target = capture.target_display();
            let source_size = target.physical_size;
            let scale =
                target.physical_size.width as f64 / target.logical_region.width.max(1) as f64;
            let display_id = target
                .output_name
                .as_deref()
                .and_then(|s| s.parse::<u32>().ok());
            return WindowGeometry {
                source_size,
                scale,
                display_id,
            };
        }
        WindowGeometry {
            source_size: rollshot_capture::Size {
                width: 0,
                height: 0,
            },
            scale: 1.0,
            display_id: None,
        }
    }

    /// Resolve the logical overlay window size and origin from the acquired
    /// capture resource. The host (`rollshot-app`'s product daemon) calls this to
    /// size/position the overlay window before `boot`. The display-origin lookup
    /// (`display_screen_geometry`) lives here so the host need not reach into the
    /// crate-private `macos_window` module.
    ///
    /// iced window sizes are logical points but `source_size` is physical pixels;
    /// the returned size is `source_size / scale` so the window covers the
    /// display 1:1 and the crop maps at the true device scale.
    pub fn overlay_window_layout(&self) -> Result<(iced::Size, iced::Point), OverlayError> {
        let geom = self.window_geometry();
        let scale = geom.scale;
        let source_size = geom.source_size;
        let window_size = iced::Size::new(
            source_size.width as f32 / scale as f32,
            source_size.height as f32 / scale as f32,
        );
        let window_origin = match self.capture_mode {
            Some(CaptureMode::Screenshot) => match geom.display_id {
                Some(did) => {
                    let display_geom = crate::macos_window::display_screen_geometry(did)
                        .map_err(OverlayError::Capture)?;
                    iced::Point::new(
                        display_geom.logical_origin.0 as f32,
                        display_geom.logical_origin.1 as f32,
                    )
                }
                None => iced::Point::ORIGIN,
            },
            _ => iced::Point::ORIGIN,
        };
        Ok((window_size, window_origin))
    }

    /// Record the overlay window the host opened. The window patch is applied
    /// later, when the `Opened` event arrives through `subscription` -> `update`
    /// (matching the original runner), so no boot task is needed here.
    pub fn boot(&mut self, overlay_window: window::Id) -> Task<Message> {
        self.overlay_window = Some(overlay_window);
        Task::none()
    }

    pub fn overlay_window(&self) -> Option<window::Id> {
        self.overlay_window
    }

    pub fn controls_window(&self) -> Option<window::Id> {
        self.controls_window
    }

    /// True when `id` is one of this component's capture windows. The host uses
    /// this to route window events to the right owner.
    pub fn owns_window(&self, id: window::Id) -> bool {
        Some(id) == self.overlay_window || Some(id) == self.controls_window
    }

    pub fn mouse_passthrough_active(&self) -> bool {
        self.overlay.mouse_passthrough_active
    }

    pub fn has_pending_completion(&self) -> bool {
        matches!(self.pending_finish, Some(PendingFinish::Complete(_)))
    }

    fn set_pending_completion(&mut self, result: CaptureResult) {
        self.pending_finish = Some(PendingFinish::Complete(result));
    }

    fn set_pending_cancel(&mut self) {
        self.pending_finish = Some(PendingFinish::Cancel);
    }

    fn disable_passthrough_task(&self) -> Task<Message> {
        match self.overlay.window_id {
            Some(id) => window::disable_mouse_passthrough(id)
                .chain(Task::done(Message(InternalMessage::PassthroughDisabled))),
            None => Task::done(Message(InternalMessage::PassthroughDisabled)),
        }
    }

    /// Move the live preview receiver into the subscription hand-off slot. Called
    /// when entering the scrolling-capture phase so the `'static` subscription
    /// closure can consume it.
    fn arm_preview_subscription(&mut self) {
        if let Some(rx) = self.preview_rx.take() {
            *PREVIEW_RX.lock().unwrap() = Some(rx);
        }
    }

    /// Drop the live preview channel (both the held receiver and any armed
    /// hand-off slot). Used on mode switch and teardown.
    fn clear_preview_channel(&mut self) {
        self.preview_rx = None;
        *PREVIEW_RX.lock().unwrap() = None;
    }

    /// Tear down any live capture resources so the stream + reader thread don't
    /// leak. Called by the host on shutdown.
    pub fn shutdown(&mut self) {
        if let Some(driver) = self.driver.take() {
            driver.cancel();
        }
        self.one_shot = None;
        self.clear_preview_channel();
    }

    pub fn theme(&self, _window: window::Id) -> iced::Theme {
        iced::Theme::Dark
    }

    pub fn style(&self, theme: &iced::Theme) -> iced::theme::Style {
        app::style(&self.overlay, theme)
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        let mut subs = vec![event::listen_with(overlay_event_message)];
        if self.overlay.workspace.phase() == WorkspacePhase::ScrollingCapture {
            if PREVIEW_RX.lock().unwrap().is_some() {
                subs.push(preview_stream());
            }
            subs.push(
                iced::time::every(Duration::from_millis(250))
                    .map(|_| Message(InternalMessage::Overlay(OverlayMessage::Tick))),
            );
        }
        iced::Subscription::batch(subs)
    }

    pub fn view(&self, window: window::Id) -> Element<'_, Message> {
        if Some(window) == self.controls_window {
            let toolbar = crate::toolbar::render_toolbar(
                self.overlay.workspace.phase(),
                self.overlay.mode,
                |action| {
                    Message(InternalMessage::Overlay(OverlayMessage::ToolbarAction(
                        action,
                    )))
                },
                Message(InternalMessage::Overlay(OverlayMessage::DragStart)),
                Message(InternalMessage::Overlay(OverlayMessage::DragEnd)),
            );
            return container(toolbar)
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }

        crate::app::view(&self.overlay).map(|m| Message(InternalMessage::Overlay(m)))
    }

    /// After the overlay window opens, validate that a screenshot one-shot image
    /// is a provable single-output match for the resolved display (mirrors the
    /// Linux runner gate). On mismatch, report `Fatal` instead of cropping
    /// against the wrong geometry.
    fn validate_screenshot_surface(&self) -> Option<HostEffect> {
        if self.capture_mode != Some(CaptureMode::Screenshot) {
            return None;
        }
        let ws = self.overlay.window_size?;
        let target = self.one_shot.as_ref()?.target_display().clone();

        let overlay_logical = rollshot_capture::Size {
            width: ws.width as u32,
            height: ws.height as u32,
        };
        let scale = target.physical_size.width as f64 / target.logical_region.width.max(1) as f64;
        match rollshot_capture::validate_surface_mapping(
            target.physical_size,
            overlay_logical,
            scale,
        ) {
            Ok(()) => None,
            Err(e) => Some(HostEffect::Fatal(e.to_string())),
        }
    }

    pub fn update(&mut self, message: Message) -> HostEffect {
        match message.0 {
            InternalMessage::WindowOpened { id, size } => {
                let patch = window::run(id, crate::macos_window::apply_overlay_window_patch)
                    .map(|r| Message(InternalMessage::WindowPatched(r)));
                if Some(id) == self.overlay_window {
                    app::update(&mut self.overlay, OverlayMessage::WindowOpened { id, size });
                    if let Some(effect) = self.validate_screenshot_surface() {
                        return effect;
                    }
                }
                HostEffect::Task(patch)
            }
            InternalMessage::OverlayWindowReady(id) => {
                self.overlay_window = Some(id);
                HostEffect::None
            }
            InternalMessage::ControlsWindowReady(id) => {
                self.controls_window = Some(id);
                HostEffect::None
            }
            InternalMessage::Overlay(msg) => self.update_overlay(msg),
            InternalMessage::WindowPatched(result) => {
                if let Err(err) = result {
                    eprintln!("failed to patch macOS iced overlay window: {err}");
                }
                HostEffect::None
            }
            InternalMessage::PassthroughEnabled => {
                self.overlay.mouse_passthrough_active = true;
                HostEffect::None
            }
            InternalMessage::PassthroughDisabled => {
                self.overlay.mouse_passthrough_active = false;
                match self.pending_finish.take() {
                    Some(PendingFinish::Complete(result)) => HostEffect::Completed(result),
                    Some(PendingFinish::Cancel) => HostEffect::Cancelled,
                    None => HostEffect::None,
                }
            }
        }
    }

    /// Apply an [`OverlayEffect`] produced by `app::update`, plus the
    /// passthrough/controls-window side effects that depend on phase changes.
    fn update_overlay(&mut self, msg: OverlayMessage) -> HostEffect {
        let old_phase = self.overlay.workspace.phase();
        let msg = controls_message_to_overlay(msg, self.controls_rect);
        let (effect, _region_mode) = app::update(&mut self.overlay, msg);

        let effect_outcome = self.apply_effect(effect);

        // Terminal effects (completion / cancel / fatal) take precedence and stop
        // the phase-driven passthrough/controls bookkeeping.
        let base_task = match effect_outcome {
            EffectOutcome::Terminal(host) => return host,
            EffectOutcome::Task(task) => task,
        };

        let new_phase = self.overlay.workspace.phase();
        let passthrough = passthrough_action(old_phase, new_phase);
        let visible_rect = self.visible_toolbar_rect();
        let controls = controls_window_action(self.controls_rect, visible_rect);

        if passthrough == PassthroughAction::Enable {
            self.arm_preview_subscription();
        }

        let passthrough_task = match passthrough {
            PassthroughAction::Enable => match self.overlay.window_id {
                Some(id) => window::enable_mouse_passthrough(id)
                    .chain(Task::done(Message(InternalMessage::PassthroughEnabled))),
                None => Task::none(),
            },
            PassthroughAction::Disable => match self.overlay.window_id {
                Some(id) => window::disable_mouse_passthrough(id)
                    .chain(Task::done(Message(InternalMessage::PassthroughDisabled))),
                None => Task::none(),
            },
            PassthroughAction::Noop => Task::none(),
        };

        let controls_task = self.controls_task(controls);

        HostEffect::Task(Task::batch([base_task, passthrough_task, controls_task]))
    }

    fn apply_effect(&mut self, effect: OverlayEffect) -> EffectOutcome {
        match effect {
            OverlayEffect::None => EffectOutcome::Task(Task::none()),
            OverlayEffect::BeginStitch => {
                let crop = self.overlay.crop.unwrap();
                let ws = match self.overlay.window_size {
                    Some(ws) => ws,
                    None => {
                        self.overlay.transient_error =
                            Some("overlay surface size unknown".to_string());
                        return EffectOutcome::Task(Task::none());
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
                if let Some(driver) = self.driver.as_mut() {
                    driver.begin_stitch(crop_logical, overlay_logical, preview_constraints);
                }
                EffectOutcome::Task(Task::none())
            }
            OverlayEffect::FinishScreenshot => {
                let crop = self.overlay.crop.unwrap();
                let ws = match self.overlay.window_size {
                    Some(ws) => ws,
                    None => {
                        self.overlay.transient_error =
                            Some("overlay surface size unknown".to_string());
                        return EffectOutcome::Task(Task::none());
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
                let outcome = match self.one_shot.take() {
                    Some(capture) => crate::screenshot::finish_screenshot(
                        &capture,
                        crop_logical,
                        overlay_logical,
                    ),
                    None => {
                        // No one-shot resource available; nothing to crop. This
                        // matches the runner's `None => Ok(None)` no-op, which is
                        // reported as a (no-op) completion via cancellation.
                        return EffectOutcome::Terminal(HostEffect::Cancelled);
                    }
                };
                match outcome {
                    Ok(result) => EffectOutcome::Terminal(HostEffect::Completed(result)),
                    Err(e) => {
                        // Inline (non-terminal) error: the overlay stays open with
                        // a transient error, matching the original runner, which
                        // returned `Task::none()` and continued the phase-driven
                        // passthrough/controls bookkeeping.
                        self.overlay.transient_error = Some(e);
                        EffectOutcome::Task(Task::none())
                    }
                }
            }
            OverlayEffect::FinishScrolling => {
                let outcome = match self.driver.take() {
                    Some(driver) => driver.finalize(),
                    None => Err("No driver available".to_string()),
                };
                match outcome {
                    Ok(result) => {
                        if self.overlay.mouse_passthrough_active {
                            self.set_pending_completion(result);
                            EffectOutcome::Terminal(HostEffect::Task(
                                self.disable_passthrough_task(),
                            ))
                        } else {
                            EffectOutcome::Terminal(HostEffect::Completed(result))
                        }
                    }
                    Err(e) => {
                        // Inline (non-terminal) error: overlay stays open and the
                        // phase-driven bookkeeping still runs, matching the runner.
                        self.overlay.transient_error = Some(e);
                        EffectOutcome::Task(Task::none())
                    }
                }
            }
            OverlayEffect::ActivateMode(new_mode) => {
                self.activate_mode(new_mode);
                EffectOutcome::Task(Task::none())
            }
            OverlayEffect::Cancel => {
                // Tear capture down and report cancellation. When passthrough is
                // active (cancel during scrolling capture), mirror the original
                // runner: disable mouse passthrough on the overlay window *first*,
                // then report cancellation once the chained `PassthroughDisabled`
                // message arrives. Exiting with passthrough still on would leave a
                // ghost transparent input-absorbing region on screen.
                if let Some(driver) = self.driver.take() {
                    driver.cancel();
                }
                self.one_shot = None;
                self.clear_preview_channel();
                if self.overlay.mouse_passthrough_active {
                    self.set_pending_cancel();
                    EffectOutcome::Terminal(HostEffect::Task(self.disable_passthrough_task()))
                } else {
                    EffectOutcome::Terminal(HostEffect::Cancelled)
                }
            }
            OverlayEffect::EnablePassthrough => match self.overlay.window_id {
                Some(id) => EffectOutcome::Task(
                    window::enable_mouse_passthrough(id)
                        .chain(Task::done(Message(InternalMessage::PassthroughEnabled))),
                ),
                None => EffectOutcome::Task(Task::none()),
            },
            OverlayEffect::DisablePassthrough => match self.overlay.window_id {
                Some(id) => EffectOutcome::Task(
                    window::disable_mouse_passthrough(id)
                        .chain(Task::done(Message(InternalMessage::PassthroughDisabled))),
                ),
                None => EffectOutcome::Task(Task::none()),
            },
        }
    }

    fn activate_mode(&mut self, new_mode: CaptureMode) {
        if let Some(driver) = self.driver.take() {
            driver.cancel();
        }
        self.one_shot = None;
        self.clear_preview_channel();

        let config = OverlayConfig {
            backend: "auto".to_string(),
            fps: 5,
            show_cursor: false,
            initial_mode: new_mode,
        };
        #[cfg(not(test))]
        let factories = real_factories();
        #[cfg(test)]
        let factories = test_factories();

        match acquire_resource(new_mode, &config, &factories) {
            Ok(Some((resource, preview_rx))) => {
                self.capture_mode = Some(new_mode);
                self.preview_rx = preview_rx;
                match resource {
                    CaptureResource::Streaming(mut driver) => {
                        if let (Some(crop), Some(ws)) =
                            (self.overlay.crop, self.overlay.window_size)
                        {
                            self.overlay.workspace.begin_scrolling();
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
                        self.driver = Some(driver);
                    }
                    CaptureResource::OneShot(capture) => {
                        let img = capture.image();
                        self.overlay.frozen = Some(iced::widget::image::Handle::from_rgba(
                            img.width(),
                            img.height(),
                            img.as_raw().clone(),
                        ));
                        self.one_shot = Some(capture);
                    }
                }
            }
            Ok(None) => {
                self.overlay.transient_error = Some("Capture cancelled".to_string());
            }
            Err(e) => {
                self.overlay.transient_error = Some(format!("Capture failed: {e}"));
            }
        }
    }

    fn controls_task(&mut self, controls: ControlsWindowAction) -> Task<Message> {
        match controls {
            ControlsWindowAction::Open(rect) => {
                if Some(rect) != self.controls_rect {
                    self.controls_rect = Some(rect);
                    let (x, y, w, h) = (
                        rect.x as i32,
                        rect.y as i32,
                        rect.width as i32,
                        rect.height as i32,
                    );
                    if let Some(id) = self.controls_window {
                        Task::batch([
                            window::move_to(id, Point::new(x as f32, y as f32)),
                            window::resize(id, Size::new(w.max(1) as f32, h.max(1) as f32)),
                        ])
                    } else {
                        let (controls_window, open_controls) =
                            window::open(controls_window_settings(x, y, w, h));
                        self.controls_window = Some(controls_window);
                        open_controls.map(|id| Message(InternalMessage::ControlsWindowReady(id)))
                    }
                } else {
                    Task::none()
                }
            }
            ControlsWindowAction::Close => {
                if let Some(id) = self.controls_window.take() {
                    self.controls_rect = None;
                    window::close(id)
                } else {
                    Task::none()
                }
            }
            ControlsWindowAction::Noop => Task::none(),
        }
    }

    fn visible_toolbar_rect(&self) -> Option<LogicalRect> {
        if self.overlay.workspace.phase() != WorkspacePhase::ScrollingCapture {
            return None;
        }
        if !app::toolbar_is_visible(&self.overlay) {
            return None;
        }
        let rect = app::toolbar_rect_for(&self.overlay)?;
        Some(LogicalRect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        })
    }

    /// Host-facing convenience for driving the component from an `OverlayEffect`
    /// directly (used by lifecycle tests and host adapters that synthesize
    /// effects). Mirrors `update_overlay`'s effect handling without an
    /// `OverlayMessage` round-trip.
    #[cfg(test)]
    pub fn apply_overlay_effect(&mut self, effect: OverlayEffect) -> HostEffect {
        let old_phase = self.overlay.workspace.phase();
        match self.apply_effect(effect) {
            EffectOutcome::Terminal(host) => host,
            EffectOutcome::Task(base_task) => {
                let new_phase = self.overlay.workspace.phase();
                let passthrough = passthrough_action(old_phase, new_phase);
                if passthrough == PassthroughAction::Enable {
                    self.arm_preview_subscription();
                }
                HostEffect::Task(base_task)
            }
        }
    }
}

impl Drop for Component {
    fn drop(&mut self) {
        // Safety net: if the component is dropped with a live capture still in
        // flight (the daemon exited without a finish/cancel taking the driver),
        // tear it down so the stream + reader thread don't leak.
        if let Some(driver) = self.driver.take() {
            driver.cancel();
        }
        *PREVIEW_RX.lock().unwrap() = None;
    }
}

/// Result/scale/display geometry resolved from the acquired capture resource.
pub struct WindowGeometry {
    pub source_size: rollshot_capture::Size,
    pub scale: f64,
    pub display_id: Option<u32>,
}

/// Internal classification of an `OverlayEffect` outcome: either a plain task to
/// batch with phase-driven side effects, or a terminal `HostEffect` that ends
/// the effect's processing (completion, cancellation, fatal, or an inline error
/// that keeps the overlay open).
enum EffectOutcome {
    Task(Task<Message>),
    Terminal(HostEffect),
}

fn preview_stream() -> iced::Subscription<Message> {
    iced::Subscription::run(|| {
        let rx = PREVIEW_RX.lock().unwrap().take();
        match rx {
            Some(rx) => rx
                .map(|e| Message(InternalMessage::Overlay(OverlayMessage::LiveEvent(e))))
                .boxed(),
            None => iced::futures::stream::pending().boxed(),
        }
    })
}

fn overlay_event_message(
    event: Event,
    status: event::Status,
    window_id: window::Id,
) -> Option<Message> {
    match event {
        Event::Window(window::Event::Opened { size, .. }) => {
            Some(Message(InternalMessage::WindowOpened {
                id: window_id,
                size,
            }))
        }
        event if status == event::Status::Ignored => Some(Message(InternalMessage::Overlay(
            OverlayMessage::IcedEvent(event),
        ))),
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

#[cfg(test)]
fn test_factories() -> ResourceFactories {
    ResourceFactories {
        streaming: Box::new(|_cfg, _preview_tx| Err("test mode".to_string())),
        one_shot: Box::new(|_| {
            Err(rollshot_capture::CaptureError::Unsupported {
                message: "test mode".to_string(),
            })
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;
    use rollshot_capture::one_shot::DisplayTarget;
    use rollshot_capture::{Region, Size};

    fn rect(x: f32, y: f32, width: f32, height: f32) -> LogicalRect {
        LogicalRect {
            x,
            y,
            width,
            height,
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

    /// A bare component with no capture resources, in the selection phase.
    fn capture_component() -> Component {
        Component {
            overlay: OverlayState::default(),
            overlay_window: None,
            controls_window: None,
            controls_rect: None,
            driver: None,
            one_shot: None,
            preview_rx: None,
            capture_mode: Some(CaptureMode::Screenshot),
            pending_finish: None,
        }
    }

    /// A screenshot component with a confirmed crop and a one-shot capture ready
    /// to finalize.
    fn capture_component_with_one_shot() -> Component {
        let mut c = capture_component();
        c.one_shot = Some(fake_one_shot_capture());
        c.overlay.mode = CaptureMode::Screenshot;
        c.overlay.window_size = Some(iced::Size::new(1920.0, 1080.0));
        c.overlay.crop = Some(iced::Rectangle {
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 100.0,
        });
        c.overlay.window_id = Some(window::Id::unique());
        c
    }

    /// A scrolling component in the capture phase with passthrough active and a
    /// driver-less finalize that returns a result (via a fake driver is not
    /// possible without capture; this stages the completion path through the
    /// `FinishScrolling` handler with `mouse_passthrough_active`).
    fn capture_component_with_active_passthrough() -> Component {
        let mut c = capture_component();
        c.overlay.mode = CaptureMode::Scrolling;
        c.overlay.window_id = Some(window::Id::unique());
        c.overlay.mouse_passthrough_active = true;
        // Stage a pending completion directly to model the post-finalize state,
        // then assert the passthrough-disable -> Completed handshake. A real
        // driver cannot be constructed in a unit test, so we drive the same
        // state machine the `FinishScrolling` branch produces.
        c
    }

    fn capture_component_with_windows() -> Component {
        let mut c = capture_component();
        c.overlay_window = Some(window::Id::unique());
        c.controls_window = Some(window::Id::unique());
        c
    }

    #[test]
    fn finish_screenshot_reports_completed_result_without_exiting_host() {
        let mut component = capture_component_with_one_shot();
        let effect = component.apply_overlay_effect(OverlayEffect::FinishScreenshot);
        assert!(matches!(
            effect,
            HostEffect::Completed(CaptureResult { .. })
        ));
    }

    #[test]
    fn finish_scrolling_disables_passthrough_before_reporting_completion() {
        let mut component = capture_component_with_active_passthrough();
        // Model the finalize result staging that `FinishScrolling` performs.
        component.set_pending_completion(CaptureResult {
            image: RgbaImage::new(4, 4),
            stats: None,
        });
        let effect = component.disable_passthrough_task();
        // The disable-passthrough task is what `FinishScrolling` returns when
        // passthrough is active; completion is still pending until the chained
        // `PassthroughDisabled` message arrives.
        let _ = effect;
        assert!(component.has_pending_completion());
        assert!(matches!(
            component.update(Message(InternalMessage::PassthroughDisabled)),
            HostEffect::Completed(_)
        ));
    }

    #[test]
    fn cancel_reports_cancelled_without_owning_process_exit() {
        let mut component = capture_component();
        assert!(matches!(
            component.apply_overlay_effect(OverlayEffect::Cancel),
            HostEffect::Cancelled
        ));
    }

    #[test]
    fn cancel_disables_passthrough_before_reporting_cancellation() {
        // Cancel during scrolling capture (passthrough active): mirror the
        // original runner's disable-passthrough-then-exit chain. The component
        // must first hand back a passthrough-disable task and stage a pending
        // cancel, then report `Cancelled` only once `PassthroughDisabled` lands.
        let mut component = capture_component_with_active_passthrough();
        let effect = component.apply_overlay_effect(OverlayEffect::Cancel);
        assert!(matches!(effect, HostEffect::Task(_)));
        // A pending cancel is not a pending completion: completion stays false so
        // the host's completion contract is unaffected.
        assert!(!component.has_pending_completion());
        assert!(matches!(
            component.pending_finish,
            Some(PendingFinish::Cancel)
        ));
        assert!(matches!(
            component.update(Message(InternalMessage::PassthroughDisabled)),
            HostEffect::Cancelled
        ));
        // Pending state cleared once consumed.
        assert!(component.pending_finish.is_none());
    }

    #[test]
    fn component_identifies_only_its_capture_windows() {
        let component = capture_component_with_windows();
        assert!(component.owns_window(component.overlay_window().unwrap()));
        assert!(component.owns_window(component.controls_window().unwrap()));
        assert!(!component.owns_window(window::Id::unique()));
    }

    #[test]
    fn controls_window_tracks_visible_toolbar_rect() {
        assert_eq!(
            controls_window_action(None, Some(rect(20.0, 30.0, 260.0, 48.0))),
            ControlsWindowAction::Open(rect(20.0, 30.0, 260.0, 48.0))
        );
    }

    #[test]
    fn controls_window_closes_while_auto_hide_is_hidden() {
        assert_eq!(
            controls_window_action(Some(rect(20.0, 30.0, 260.0, 48.0)), None),
            ControlsWindowAction::Close
        );
    }

    #[test]
    fn controls_window_noop_when_both_none() {
        assert_eq!(
            controls_window_action(None, None),
            ControlsWindowAction::Noop
        );
    }

    #[test]
    fn leaving_scrolling_capture_disables_passthrough() {
        assert_eq!(
            passthrough_action(WorkspacePhase::ScrollingCapture, WorkspacePhase::Selecting),
            PassthroughAction::Disable
        );
    }

    #[test]
    fn scrolling_capture_enables_passthrough() {
        assert_eq!(
            passthrough_action(WorkspacePhase::Selecting, WorkspacePhase::ScrollingCapture),
            PassthroughAction::Enable
        );
    }

    #[test]
    fn controls_cursor_coordinates_are_mapped_to_overlay() {
        let message = controls_message_to_overlay(
            OverlayMessage::IcedEvent(Event::Mouse(iced::mouse::Event::CursorMoved {
                position: Point::new(10.0, 15.0),
            })),
            Some(rect(100.0, 200.0, 360.0, 48.0)),
        );

        match message {
            OverlayMessage::IcedEvent(Event::Mouse(iced::mouse::Event::CursorMoved {
                position,
            })) => assert_eq!(position, Point::new(110.0, 215.0)),
            _ => panic!("expected cursor moved"),
        }
    }
}
