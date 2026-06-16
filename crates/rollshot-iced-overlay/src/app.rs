use iced::futures::StreamExt;
use iced::widget::canvas::Path;
use iced::widget::{canvas, container, image, text};
use iced::{
    keyboard, mouse, window, Color, ContentFit, Element, Event, Length, Point, Rectangle, Size,
};
use rollshot_capture::Workflow;
use rollshot_overlay_core::chrome_placement::{self, ChromeRequirements, Rect};
use rollshot_overlay_core::preview::PREVIEW_WIDTH;
use rollshot_overlay_core::tokens;
use std::sync::Mutex;

static SHARED_PREVIEW_RX: Mutex<
    Option<iced::futures::channel::mpsc::UnboundedReceiver<crate::driver::LiveOverlayEvent>>,
> = Mutex::new(None);

#[allow(dead_code)]
const SENTINEL_MAGENTA: Color = Color::from_rgba(1.0, 0.0, 1.0, 1.0);
const CHROME_SPACING: f32 = 8.0;

#[allow(dead_code)]
const CAPTURE_STATUS_TEXT: &str = "Capturing - scroll the target";
#[allow(dead_code)]
const FOCUS_PAUSED_TEXT: &str = "Shortcuts paused - click Rollshot controls to restore Esc";
#[allow(dead_code)]
const FINISH_LABEL: &str = "Finish";
#[allow(dead_code)]
const CANCEL_LABEL: &str = "Cancel";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputRegionMode {
    None,
    #[allow(dead_code)]
    FullOverlay,
    ToolbarOnly,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub(crate) enum OverlayEffect {
    None,
    BeginStitch,
    FinishScrolling,
    FinishRegion,
    Cancel,
    EnablePassthrough,
    DisablePassthrough,
    ActivateWorkflow(Workflow),
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum OverlayMessage {
    IcedEvent(iced::Event),
    WindowOpened { id: window::Id, size: Size },
    Finish,
    FinishCapture,
    Cancel,
    LiveEvent(crate::driver::LiveOverlayEvent),
    Tick,
    ActivateWorkflow(Workflow),
    ToolbarAction(crate::toolbar::ToolbarAction),
    DragStart,
    DragEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(crate) struct PreviewConstraints {
    pub(crate) fixed_width: u32,
    pub(crate) max_height: u32,
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(crate) struct OverlayState {
    pub(crate) drag_start: Option<Point>,
    pub(crate) crop: Option<Rectangle>,
    pub(crate) workspace: crate::workspace::WorkspaceState,
    pub(crate) preview: Option<image::Handle>,
    pub(crate) window_id: Option<window::Id>,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) mouse_passthrough_active: bool,
    pub(crate) window_size: Option<Size>,
    pub(crate) capture_miss_warn: bool,
    pub(crate) capture_miss_message_expires_at: Option<std::time::Instant>,
    pub(crate) capture_miss_active: bool,
    pub(crate) capture_miss_edge: rollshot_overlay_core::capture_miss::CapturedEdge,
    /// Active workflow. Region mode finishes immediately on a valid release;
    /// scrolling mode confirms the crop and begins streaming/stitching.
    pub(crate) workflow: Workflow,
    /// Frozen one-shot background, present only in region mode. Built once by
    /// the platform runner; `view()` clones the cheap handle, never the pixels.
    pub(crate) frozen: Option<image::Handle>,
    /// Last known window-relative cursor position, tracked from `CursorMoved`.
    pub(crate) cursor_position: Option<Point>,
    /// Toolbar drag state. `Some(offset)` while a drag is active, where `offset`
    /// is the cursor position relative to the toolbar's top-left at grab time, so
    /// the grabbed point stays under the cursor as the toolbar moves.
    pub(crate) toolbar_drag_grab: Option<iced::Vector>,
    pub(crate) toolbar_position: crate::workspace::ToolbarPosition,
    pub(crate) transient_error: Option<String>,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self {
            drag_start: None,
            crop: None,
            workspace: crate::workspace::WorkspaceState::new(Workflow::Scrolling),
            preview: None,
            window_id: None,
            mouse_passthrough_active: false,
            window_size: None,
            capture_miss_warn: false,
            capture_miss_message_expires_at: None,
            capture_miss_active: false,
            capture_miss_edge: rollshot_overlay_core::capture_miss::CapturedEdge::Unknown,
            workflow: Workflow::Scrolling,
            frozen: None,
            cursor_position: None,
            toolbar_drag_grab: None,
            toolbar_position: crate::workspace::ToolbarPosition::Automatic,
            transient_error: None,
        }
    }
}

impl OverlayState {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn warning(&self) -> Option<&str> {
        self.capture_miss_warn
            .then_some(rollshot_overlay_core::capture_miss::CAPTURE_MISS_WARNING)
    }
}

fn clear_capture_miss_ui(state: &mut OverlayState) {
    state.capture_miss_active = false;
    state.capture_miss_edge = rollshot_overlay_core::capture_miss::CapturedEdge::Unknown;
    state.capture_miss_warn = false;
    state.capture_miss_message_expires_at = None;
}

pub(crate) fn token_color(c: tokens::Rgba) -> Color {
    Color::from_rgba(
        c.r as f32 / 255.0,
        c.g as f32 / 255.0,
        c.b as f32 / 255.0,
        c.a,
    )
}

pub(crate) fn recovery_edge_line(
    crop: Rectangle,
    edge: rollshot_overlay_core::capture_miss::CapturedEdge,
) -> Option<(Point, Point)> {
    use rollshot_overlay_core::capture_miss::CapturedEdge;
    match edge {
        CapturedEdge::Top => Some((
            Point::new(crop.x, crop.y),
            Point::new(crop.x + crop.width, crop.y),
        )),
        CapturedEdge::Bottom => Some((
            Point::new(crop.x, crop.y + crop.height),
            Point::new(crop.x + crop.width, crop.y + crop.height),
        )),
        CapturedEdge::Left => Some((
            Point::new(crop.x, crop.y),
            Point::new(crop.x, crop.y + crop.height),
        )),
        CapturedEdge::Right => Some((
            Point::new(crop.x + crop.width, crop.y),
            Point::new(crop.x + crop.width, crop.y + crop.height),
        )),
        CapturedEdge::Unknown => None,
    }
}

pub(crate) fn crop_mask_bands(crop: Rectangle, bounds: Rectangle) -> [(Point, Size); 4] {
    let cx = crop.x.clamp(0.0, bounds.width);
    let cy = crop.y.clamp(0.0, bounds.height);
    let right = (crop.x + crop.width).clamp(0.0, bounds.width);
    let bottom = (crop.y + crop.height).clamp(0.0, bounds.height);
    let visible_h = (bottom - cy).max(0.0);

    [
        (Point::ORIGIN, Size::new(bounds.width, cy)),
        (
            Point::new(0.0, bottom),
            Size::new(bounds.width, (bounds.height - bottom).max(0.0)),
        ),
        (Point::new(0.0, cy), Size::new(cx, visible_h)),
        (
            Point::new(right, cy),
            Size::new((bounds.width - right).max(0.0), visible_h),
        ),
    ]
}

pub(crate) struct CropCanvas {
    crop: Option<Rectangle>,
    confirmed: bool,
    recovery_edge: Option<rollshot_overlay_core::capture_miss::CapturedEdge>,
}

impl CropCanvas {
    fn from_state(state: &OverlayState) -> Self {
        Self {
            crop: state.crop,
            confirmed: state.workspace.phase() != crate::workspace::WorkspacePhase::Selecting,
            recovery_edge: if state.capture_miss_active {
                Some(state.capture_miss_edge)
            } else {
                None
            },
        }
    }
}

impl canvas::Program<OverlayMessage> for CropCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        // R3: during capture (confirmed) draw the dark mask outside the crop so
        // the selected region stays visually highlighted.  Chrome lives outside
        // the crop and the region is cropped before stitching.
        match self.crop {
            Some(crop) => {
                // Dark mask over everything outside the crop (four bands),
                // matching the app's box-shadow dimming.
                let mask = token_color(tokens::CROP_MASK);
                for (origin, size) in crop_mask_bands(crop, bounds) {
                    if size.width > 0.0 && size.height > 0.0 {
                        frame.fill_rectangle(origin, size, mask);
                    }
                }

                if !self.confirmed {
                    // 1px white halo just outside the border.
                    let bw = tokens::CROP_BORDER_WIDTH;
                    let halo = canvas::Stroke::default()
                        .with_color(token_color(tokens::CROP_BORDER_HALO))
                        .with_width(1.0);
                    frame.stroke_rectangle(
                        Point::new(crop.x - bw, crop.y - bw),
                        Size::new(crop.width + bw * 2.0, crop.height + bw * 2.0),
                        halo,
                    );
                    // Sky-blue crop border.
                    let border = canvas::Stroke::default()
                        .with_color(token_color(tokens::CROP_BORDER))
                        .with_width(bw);
                    frame.stroke_rectangle(
                        Point::new(crop.x, crop.y),
                        Size::new(crop.width, crop.height),
                        border,
                    );
                }

                // Draw recovery edge guide when confirmed and paused.
                if self.confirmed {
                    if let Some(edge) = self.recovery_edge {
                        if let Some((p1, p2)) = recovery_edge_line(crop, edge) {
                            let stroke = canvas::Stroke::default()
                                .with_color(token_color(tokens::RECOVERY_EDGE))
                                .with_width(tokens::RECOVERY_EDGE_WIDTH);
                            let path = Path::line(p1, p2);
                            frame.stroke(&path, stroke);
                        }
                    }
                }
            }
            None => {
                if !self.confirmed {
                    // Dim the whole layer before a rect is drawn.
                    frame.fill_rectangle(
                        Point::ORIGIN,
                        bounds.size(),
                        token_color(tokens::CROP_DIM),
                    );
                }
            }
        }

        if !self.confirmed {
            // Cursor crosshair guides.
            if let Some(pos) = cursor.position_in(bounds) {
                let guide = token_color(tokens::CROP_GUIDE);
                let gw = tokens::CROP_GUIDE_WIDTH;
                frame.fill_rectangle(Point::new(0.0, pos.y), Size::new(bounds.width, gw), guide);
                frame.fill_rectangle(Point::new(pos.x, 0.0), Size::new(gw, bounds.height), guide);
            }
        }

        vec![frame.into_geometry()]
    }
}

pub(crate) fn preview_constraints(crop: Rectangle, _window: iced::Size) -> PreviewConstraints {
    PreviewConstraints {
        fixed_width: PREVIEW_WIDTH,
        max_height: crop.height.max(1.0).floor() as u32,
    }
}

fn preview_size(handle: &image::Handle) -> Option<chrome_placement::Size> {
    match handle {
        image::Handle::Rgba { width, height, .. } => {
            Some(chrome_placement::Size::new(*width as f32, *height as f32))
        }
        image::Handle::Path(..) | image::Handle::Bytes(..) => None,
    }
}

fn chrome_placement_for(
    state: &OverlayState,
) -> Option<rollshot_overlay_core::chrome_placement::ChromePlacement> {
    let crop = state.crop?;
    let window = state.window_size?;
    let preview = (state.workspace.phase() == crate::workspace::WorkspacePhase::ScrollingCapture)
        .then(|| state.preview.as_ref().and_then(preview_size))
        .flatten();
    Some(chrome_placement::place_chrome(
        Rect::new(0.0, 0.0, window.width, window.height),
        Rect::new(crop.x, crop.y, crop.width, crop.height),
        ChromeRequirements {
            toolbar: chrome_placement::Size::new(
                crate::toolbar::TOOLBAR_WIDTH,
                crate::toolbar::TOOLBAR_HEIGHT,
            ),
            preview,
            margin: 8.0,
            spacing: CHROME_SPACING,
        },
    ))
}

pub(crate) fn toolbar_rect_for(state: &OverlayState) -> Option<Rect> {
    match state.toolbar_position {
        crate::workspace::ToolbarPosition::Manual(rect) => Some(rect.into()),
        crate::workspace::ToolbarPosition::Automatic => {
            chrome_placement_for(state).map(|placement| placement.toolbar_rect())
        }
    }
}

pub(crate) fn toolbar_is_visible(state: &OverlayState) -> bool {
    match chrome_placement_for(state) {
        Some(chrome_placement::ChromePlacement::ActivityAutoHide { .. })
            if state.workspace.phase() == crate::workspace::WorkspacePhase::ScrollingCapture =>
        {
            state
                .workspace
                .auto_hide()
                .visible(std::time::Instant::now())
        }
        _ => true,
    }
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(crate) fn view(state: &OverlayState) -> Element<'_, OverlayMessage> {
    view_with_toolbar(state, true)
}

pub(crate) fn view_with_toolbar(
    state: &OverlayState,
    render_toolbar: bool,
) -> Element<'_, OverlayMessage> {
    let canvas_widget = canvas(CropCanvas::from_state(state))
        .width(Length::Fill)
        .height(Length::Fill);

    if state.workspace.phase() != crate::workspace::WorkspacePhase::Selecting {
        // Capture phase: the base layer (canvas) draws nothing, keeping the
        // crop interior transparent. Chrome goes strictly outside the crop.
        let crop = state.crop.unwrap_or(Rectangle {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        });
        let window = state.window_size.unwrap_or(iced::Size::new(0.0, 0.0));

        let toolbar = crate::toolbar::render_toolbar(
            state.workspace.phase(),
            state.workflow,
            OverlayMessage::ToolbarAction,
            OverlayMessage::DragStart,
            OverlayMessage::DragEnd,
        );

        let placement = chrome_placement_for(state).unwrap_or_else(|| {
            chrome_placement::place_chrome(
                Rect::new(0.0, 0.0, window.width, window.height),
                Rect::new(crop.x, crop.y, crop.width, crop.height),
                ChromeRequirements {
                    toolbar: chrome_placement::Size::new(
                        crate::toolbar::TOOLBAR_WIDTH,
                        crate::toolbar::TOOLBAR_HEIGHT,
                    ),
                    preview: None,
                    margin: 8.0,
                    spacing: CHROME_SPACING,
                },
            )
        });

        let warning: Option<Element<'_, OverlayMessage>> = state.capture_miss_warn.then(|| {
            container(text(rollshot_overlay_core::capture_miss::CAPTURE_MISS_WARNING).size(14))
                .padding(8)
                .style(|_theme| container::Style {
                    background: Some(iced::Background::Color(Color::from_rgba(
                        120.0 / 255.0,
                        53.0 / 255.0,
                        15.0 / 255.0,
                        0.94,
                    ))),
                    text_color: Some(Color::from_rgb(1.0, 251.0 / 255.0, 235.0 / 255.0)),
                    ..Default::default()
                })
                .into()
        });
        let transient_error: Option<Element<'_, OverlayMessage>> =
            state.transient_error.as_deref().map(|message| {
                container(text(message).size(14))
                    .padding(8)
                    .style(|_theme| container::Style {
                        background: Some(iced::Background::Color(Color::from_rgba(
                            127.0 / 255.0,
                            29.0 / 255.0,
                            29.0 / 255.0,
                            0.94,
                        ))),
                        text_color: Some(Color::WHITE),
                        ..Default::default()
                    })
                    .into()
            });

        let toolbar_rect = toolbar_rect_for(state).unwrap_or_else(|| placement.toolbar_rect());
        let chrome_visible = toolbar_is_visible(state);

        let toolbar_layer = container(toolbar)
            .width(Length::Fixed(toolbar_rect.width))
            .height(Length::Fixed(toolbar_rect.height))
            .align_x(iced::Alignment::Center)
            .align_y(iced::Alignment::Center);

        let mut chrome_stack = match (&state.frozen, state.workflow) {
            (Some(handle), Workflow::Screenshot) => iced::widget::stack![
                image(handle.clone())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .content_fit(ContentFit::Fill),
                canvas_widget
            ],
            _ => iced::widget::stack![canvas_widget],
        };

        if chrome_visible {
            if render_toolbar {
                chrome_stack = chrome_stack.push(
                    container(toolbar_layer)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .align_x(iced::Alignment::Start)
                        .align_y(iced::Alignment::Start)
                        .padding(iced::Padding {
                            left: toolbar_rect.x,
                            top: toolbar_rect.y,
                            right: 0.0,
                            bottom: 0.0,
                        }),
                );
            }

            if let Some(w) = warning {
                chrome_stack = chrome_stack.push(
                    container(w)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .align_x(iced::Alignment::Center)
                        .align_y(iced::Alignment::End)
                        .padding(16),
                );
            }
            if let Some(error) = transient_error {
                chrome_stack = chrome_stack.push(
                    container(error)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .align_x(iced::Alignment::Center)
                        .align_y(iced::Alignment::End)
                        .padding(16),
                );
            }

            if state.workspace.phase() == crate::workspace::WorkspacePhase::ScrollingCapture {
                if let Some(handle) = &state.preview {
                    if let Some(preview_rect) = placement.preview_rect() {
                        chrome_stack = chrome_stack.push(
                            container(
                                container(image(handle.clone()))
                                    .width(Length::Fixed(preview_rect.width))
                                    .height(Length::Fixed(preview_rect.height)),
                            )
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .align_x(iced::Alignment::Start)
                            .align_y(iced::Alignment::Start)
                            .padding(iced::Padding {
                                left: preview_rect.x,
                                top: preview_rect.y,
                                right: 0.0,
                                bottom: 0.0,
                            }),
                        );
                    }
                }
            }
        }

        return chrome_stack.into();
    }

    // Selection phase: drag to pick a crop; toolbar with Cancel.
    let toolbar = crate::toolbar::render_toolbar(
        state.workspace.phase(),
        state.workflow,
        OverlayMessage::ToolbarAction,
        OverlayMessage::DragStart,
        OverlayMessage::DragEnd,
    );

    let toolbar_layer = container(toolbar)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::Alignment::Start)
        .align_y(iced::Alignment::Start)
        .padding(16);

    // Region mode draws the frozen capture as the background, with the dim
    // mask, crosshair guides, and selection border (the canvas) composited above
    // it (spec steps 2-3). Scrolling mode has no frozen image; the canvas sits on
    // the transparent layer surface so the live target shows through.
    match &state.frozen {
        Some(handle) => iced::widget::stack![
            image(handle.clone())
                .width(Length::Fill)
                .height(Length::Fill)
                .content_fit(ContentFit::Fill),
            canvas_widget,
            toolbar_layer,
        ]
        .into(),
        None => iced::widget::stack![canvas_widget, toolbar_layer].into(),
    }
}

pub(crate) fn style(_state: &OverlayState, theme: &iced::Theme) -> iced::theme::Style {
    iced::theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: theme.palette().text,
    }
}

#[allow(dead_code)]
pub(crate) fn preview_stream(
    rx: iced::futures::channel::mpsc::UnboundedReceiver<crate::driver::LiveOverlayEvent>,
) -> iced::Subscription<OverlayMessage> {
    *SHARED_PREVIEW_RX.lock().unwrap() = Some(rx);
    iced::Subscription::run(|| {
        SHARED_PREVIEW_RX
            .lock()
            .unwrap()
            .take()
            .expect("preview channel already consumed")
            .map(OverlayMessage::LiveEvent)
    })
}

#[allow(dead_code)]
pub(crate) fn update(
    state: &mut OverlayState,
    message: OverlayMessage,
) -> (OverlayEffect, InputRegionMode) {
    use crate::workspace::WorkspacePhase;

    match message {
        OverlayMessage::WindowOpened { id, size } => {
            state.window_id = Some(id);
            state.window_size = Some(size);
            (OverlayEffect::None, InputRegionMode::None)
        }
        OverlayMessage::IcedEvent(Event::Window(window::Event::Opened { size, .. })) => {
            state.window_size = Some(size);
            (OverlayEffect::None, InputRegionMode::None)
        }
        OverlayMessage::IcedEvent(Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        ))) if state.workspace.phase() == WorkspacePhase::Selecting => {
            state.drag_start = Some(Point::ORIGIN);
            state.crop = None;
            state.toolbar_position = crate::workspace::ToolbarPosition::Automatic;
            (OverlayEffect::None, InputRegionMode::None)
        }
        OverlayMessage::IcedEvent(Event::Mouse(mouse::Event::CursorMoved { position })) => {
            state.cursor_position = Some(position);
            if let Some(grab) = state.toolbar_drag_grab {
                let window = state.window_size.unwrap_or(iced::Size::new(0.0, 0.0));
                let viewport = Rect::new(0.0, 0.0, window.width, window.height);
                let toolbar_rect = Rect::new(
                    position.x - grab.x,
                    position.y - grab.y,
                    crate::toolbar::TOOLBAR_WIDTH,
                    crate::toolbar::TOOLBAR_HEIGHT,
                );
                let clamped = crate::toolbar::finish_drag(toolbar_rect, viewport);
                state.toolbar_position = crate::workspace::ToolbarPosition::Manual(clamped);
            }
            if let Some(start) = state.drag_start {
                if start == Point::ORIGIN && state.crop.is_none() {
                    state.drag_start = Some(position);
                }
                if let Some(start) = state.drag_start {
                    let x = start.x.min(position.x);
                    let y = start.y.min(position.y);
                    let w = (position.x - start.x).abs();
                    let h = (position.y - start.y).abs();
                    state.crop = Some(Rectangle {
                        x,
                        y,
                        width: w,
                        height: h,
                    });
                }
            }
            (OverlayEffect::None, InputRegionMode::None)
        }
        OverlayMessage::IcedEvent(Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Left,
        ))) => {
            state.drag_start = None;
            if state.workspace.phase() == WorkspacePhase::Selecting
                && state.crop.is_some_and(|c| c.width > 0.0 && c.height > 0.0)
            {
                let crop = state.crop.unwrap();
                let crop_rect = crate::workspace::CropRect {
                    x: crop.x,
                    y: crop.y,
                    width: crop.width,
                    height: crop.height,
                };
                state.workspace.set_crop(Some(crop_rect));
                state.workspace.complete_selection();
                let effect = match state.workflow {
                    Workflow::Screenshot => {
                        state.workspace.finish_region();
                        OverlayEffect::FinishRegion
                    }
                    Workflow::Scrolling => {
                        state.workspace.begin_scrolling();
                        OverlayEffect::BeginStitch
                    }
                    Workflow::ActionGuide => { /* wired in Task 5 */ OverlayEffect::None }
                };
                (effect, InputRegionMode::None)
            } else {
                (OverlayEffect::None, InputRegionMode::None)
            }
        }
        OverlayMessage::IcedEvent(Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        })) => {
            if state.workspace.phase() == WorkspacePhase::ScrollingCapture {
                state.workspace.finish_scrolling();
                (OverlayEffect::FinishScrolling, InputRegionMode::None)
            } else {
                state.workspace.cancel();
                (OverlayEffect::Cancel, InputRegionMode::None)
            }
        }
        OverlayMessage::IcedEvent(Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Enter),
            ..
        })) if state.workspace.phase() == WorkspacePhase::Selecting
            && state.crop.is_some_and(|c| c.width > 0.0 && c.height > 0.0) =>
        {
            let crop = state.crop.unwrap();
            let crop_rect = crate::workspace::CropRect {
                x: crop.x,
                y: crop.y,
                width: crop.width,
                height: crop.height,
            };
            state.workspace.set_crop(Some(crop_rect));
            state.workspace.complete_selection();
            let effect = match state.workflow {
                Workflow::Screenshot => {
                    state.workspace.finish_region();
                    OverlayEffect::FinishRegion
                }
                Workflow::Scrolling => {
                    state.workspace.begin_scrolling();
                    OverlayEffect::BeginStitch
                }
                Workflow::ActionGuide => { /* wired in Task 5 */ OverlayEffect::None }
            };
            (effect, InputRegionMode::None)
        }
        OverlayMessage::FinishCapture => {
            if state.workspace.phase() == WorkspacePhase::ScrollingCapture {
                state.workspace.finish_scrolling();
                (OverlayEffect::FinishScrolling, InputRegionMode::None)
            } else {
                (OverlayEffect::None, InputRegionMode::None)
            }
        }
        OverlayMessage::Finish => {
            match state.workspace.phase() {
                WorkspacePhase::ScrollingCapture => {
                    state.workspace.finish_scrolling();
                    (OverlayEffect::FinishScrolling, InputRegionMode::None)
                }
                WorkspacePhase::Selected => {
                    if state.workflow == Workflow::Screenshot {
                        state.workspace.finish_region();
                        return (OverlayEffect::FinishRegion, InputRegionMode::None);
                    }
                    // Scrolling in Selected: the runner calls begin_scrolling.
                    (OverlayEffect::None, InputRegionMode::None)
                }
                WorkspacePhase::Recording => {
                    state.workspace.finish_recording();
                    // FinishRecording wired in Task 5
                    (OverlayEffect::None, InputRegionMode::None)
                }
                WorkspacePhase::Selecting => {
                    // Require a non-empty crop; otherwise keep selecting.
                    if !state
                        .crop
                        .is_some_and(|c| c.width >= 1.0 && c.height >= 1.0)
                    {
                        state.capture_miss_warn = true;
                        state.capture_miss_message_expires_at =
                            Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
                        return (OverlayEffect::None, InputRegionMode::None);
                    }
                    let crop = state.crop.unwrap();
                    let crop_rect = crate::workspace::CropRect {
                        x: crop.x,
                        y: crop.y,
                        width: crop.width,
                        height: crop.height,
                    };
                    state.workspace.set_crop(Some(crop_rect));
                    state.workspace.complete_selection();
                    let effect = match state.workflow {
                        Workflow::Screenshot => {
                            state.workspace.finish_region();
                            OverlayEffect::FinishRegion
                        }
                        Workflow::Scrolling => OverlayEffect::BeginStitch,
                        Workflow::ActionGuide => { /* wired in Task 5 */ OverlayEffect::None }
                    };
                    (effect, InputRegionMode::None)
                }
            }
        }
        OverlayMessage::ActivateWorkflow(workflow) => {
            state.workflow = workflow;
            state.workspace.activate_workflow(workflow);
            state.drag_start = None;
            clear_capture_miss_ui(state);
            let region = match workflow {
                Workflow::Scrolling => InputRegionMode::ToolbarOnly,
                Workflow::Screenshot => InputRegionMode::None,
                Workflow::ActionGuide => { /* wired in Task 5 */ InputRegionMode::None }
            };
            (OverlayEffect::ActivateWorkflow(workflow), region)
        }
        OverlayMessage::Cancel => (OverlayEffect::Cancel, InputRegionMode::None),
        OverlayMessage::LiveEvent(crate::driver::LiveOverlayEvent::AcceptedActivity(instant)) => {
            state.workspace.auto_hide_mut().accepted_frame(instant);
            (OverlayEffect::None, InputRegionMode::None)
        }
        OverlayMessage::LiveEvent(crate::driver::LiveOverlayEvent::Preview(handle)) => {
            state.preview = Some(handle);
            (OverlayEffect::None, InputRegionMode::None)
        }
        OverlayMessage::LiveEvent(crate::driver::LiveOverlayEvent::CaptureMiss(miss)) => {
            state.capture_miss_active = miss.active;
            state.capture_miss_edge = miss.edge;
            if miss.warn {
                state.capture_miss_warn = true;
                state.capture_miss_message_expires_at =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
            }
            (OverlayEffect::None, InputRegionMode::None)
        }
        OverlayMessage::Tick => {
            if state
                .capture_miss_message_expires_at
                .is_some_and(|deadline| std::time::Instant::now() >= deadline)
            {
                state.capture_miss_warn = false;
                state.capture_miss_message_expires_at = None;
            }
            (OverlayEffect::None, InputRegionMode::None)
        }
        OverlayMessage::ToolbarAction(action) => match action {
            crate::toolbar::ToolbarAction::RegionMode => {
                state.workflow = Workflow::Screenshot;
                state.workspace.activate_workflow(Workflow::Screenshot);
                clear_capture_miss_ui(state);
                (
                    OverlayEffect::ActivateWorkflow(Workflow::Screenshot),
                    InputRegionMode::None,
                )
            }
            crate::toolbar::ToolbarAction::ScrollingMode => {
                state.workflow = Workflow::Scrolling;
                state.workspace.activate_workflow(Workflow::Scrolling);
                clear_capture_miss_ui(state);
                (
                    OverlayEffect::ActivateWorkflow(Workflow::Scrolling),
                    InputRegionMode::None,
                )
            }
            crate::toolbar::ToolbarAction::Finish => match state.workspace.phase() {
                WorkspacePhase::ScrollingCapture => {
                    state.workspace.finish_scrolling();
                    (OverlayEffect::FinishScrolling, InputRegionMode::None)
                }
                WorkspacePhase::Selected if state.workflow == Workflow::Screenshot => {
                    state.workspace.finish_region();
                    (OverlayEffect::FinishRegion, InputRegionMode::None)
                }
                _ => (OverlayEffect::None, InputRegionMode::None),
            },
            crate::toolbar::ToolbarAction::Cancel => {
                state.workspace.cancel();
                (OverlayEffect::Cancel, InputRegionMode::None)
            }
        },
        OverlayMessage::DragStart => {
            // Anchor the drag to where the cursor grabbed the toolbar so the
            // grabbed point stays under the cursor; movement is applied in the
            // `CursorMoved` handler using the latest window-relative cursor.
            if let (Some(cursor), Some(toolbar)) = (state.cursor_position, toolbar_rect_for(state))
            {
                state.toolbar_drag_grab = Some(iced::Vector::new(
                    cursor.x - toolbar.x,
                    cursor.y - toolbar.y,
                ));
            } else {
                state.toolbar_drag_grab = Some(iced::Vector::new(0.0, 0.0));
            }
            state.workspace.auto_hide_mut().set_interacting(true);
            (OverlayEffect::None, InputRegionMode::None)
        }
        OverlayMessage::DragEnd => {
            state.toolbar_drag_grab = None;
            state.workspace.auto_hide_mut().set_interacting(false);
            (OverlayEffect::None, InputRegionMode::None)
        }
        _ => (OverlayEffect::None, InputRegionMode::None),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        chrome_placement_for, crop_mask_bands, preview_constraints, token_color, OverlayMessage,
        OverlayState,
    };
    use iced::{Point, Rectangle, Size};
    use rollshot_overlay_core::preview::PREVIEW_WIDTH;

    #[test]
    fn preview_constraints_use_fixed_width_and_crop_height() {
        let crop = Rectangle {
            x: 100.0,
            y: 100.0,
            width: 2400.0,
            height: 900.0,
        };
        let window = Size::new(2560.0, 1440.0);

        let constraints = preview_constraints(crop, window);

        assert_eq!(constraints.fixed_width, PREVIEW_WIDTH);
        assert_eq!(constraints.max_height, 900);
    }

    #[test]
    fn preview_constraints_ignore_narrow_outside_bands() {
        let crop = Rectangle {
            x: 200.0,
            y: 10.0,
            width: 2300.0,
            height: 1420.0,
        };
        let window = Size::new(2560.0, 1440.0);

        let constraints = preview_constraints(crop, window);

        assert_eq!(constraints.fixed_width, PREVIEW_WIDTH);
        assert_eq!(constraints.max_height, 1420);
    }

    #[test]
    fn preview_constraints_cap_height_at_crop_height() {
        let crop = Rectangle {
            x: 100.0,
            y: 100.0,
            width: 200.0,
            height: 600.0,
        };
        let window = Size::new(2560.0, 1440.0);

        let constraints = preview_constraints(crop, window);

        assert_eq!(constraints.fixed_width, PREVIEW_WIDTH);
        assert_eq!(constraints.max_height, 600);
    }

    #[test]
    fn preview_constraints_use_crop_for_activity_auto_hide_fallback() {
        let crop = Rectangle {
            x: 10.0,
            y: 10.0,
            width: 980.0,
            height: 780.0,
        };
        let window = Size::new(1000.0, 800.0);

        let constraints = preview_constraints(crop, window);

        assert_eq!(constraints.fixed_width, PREVIEW_WIDTH);
        assert_eq!(constraints.max_height, 780);
    }

    #[test]
    fn chrome_placement_uses_actual_growing_preview_size() {
        let mut state = OverlayState {
            crop: Some(Rectangle {
                x: 385.0,
                y: 189.0,
                width: 1037.0,
                height: 520.0,
            }),
            window_size: Some(Size::new(1470.0, 956.0)),
            preview: Some(iced::widget::image::Handle::from_rgba(
                PREVIEW_WIDTH,
                240,
                vec![0; (PREVIEW_WIDTH * 240 * 4) as usize],
            )),
            ..OverlayState::default()
        };
        state.workspace.begin_scrolling();

        let preview = chrome_placement_for(&state)
            .and_then(|placement| placement.preview_rect())
            .expect("preview placement");

        assert_eq!(preview.width, PREVIEW_WIDTH as f32);
        assert_eq!(preview.height, 240.0);
    }

    #[test]
    fn crop_mask_bands_clamp_crop_to_canvas_bounds() {
        let bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 80.0,
        };
        let crop = Rectangle {
            x: -10.0,
            y: 10.0,
            width: 70.0,
            height: 90.0,
        };

        let bands = crop_mask_bands(crop, bounds);

        assert_eq!(bands[0], (Point::ORIGIN, Size::new(100.0, 10.0)));
        assert_eq!(bands[1], (Point::new(0.0, 80.0), Size::new(100.0, 0.0)));
        assert_eq!(bands[2], (Point::new(0.0, 10.0), Size::new(0.0, 70.0)));
        assert_eq!(bands[3], (Point::new(60.0, 10.0), Size::new(40.0, 70.0)));
    }

    #[test]
    fn token_color_preserves_rgba_channels() {
        let color = token_color(rollshot_overlay_core::tokens::CROP_MASK);

        assert_eq!(color.r, 0.0);
        assert_eq!(color.g, 0.0);
        assert_eq!(color.b, 0.0);
        assert!((color.a - 0.24).abs() < f32::EPSILON);
    }

    #[test]
    fn finish_without_crop_requests_warning_not_effect() {
        let mut state = OverlayState::default();
        let (effect, _region) = super::update(&mut state, OverlayMessage::Finish);
        assert_eq!(effect, super::OverlayEffect::None);
        assert!(state.warning().is_some());
    }

    #[test]
    fn window_opened_records_window_id_and_size() {
        let mut state = OverlayState::default();
        let id = iced::window::Id::unique();
        let size = Size::new(1440.0, 900.0);

        let (effect, _region) =
            super::update(&mut state, OverlayMessage::WindowOpened { id, size });

        assert_eq!(effect, super::OverlayEffect::None);
        assert_eq!(state.window_id, Some(id));
        assert_eq!(state.window_size, Some(size));
    }

    #[test]
    fn finish_capture_control_finalizes_scrolling_capture() {
        let mut state = OverlayState {
            crop: Some(Rectangle {
                x: 10.0,
                y: 20.0,
                width: 120.0,
                height: 80.0,
            }),
            ..OverlayState::default()
        };
        state.workspace.begin_scrolling();

        let (effect, _region) = super::update(&mut state, OverlayMessage::FinishCapture);

        assert_eq!(effect, super::OverlayEffect::FinishScrolling);
    }

    #[test]
    fn selection_finish_still_validates_empty_crop() {
        let mut state = OverlayState::default();

        let (effect, _region) = super::update(&mut state, OverlayMessage::Finish);

        assert_eq!(effect, super::OverlayEffect::None);
        assert!(state.warning().is_some());
    }

    #[test]
    fn region_release_requests_immediate_finalization() {
        use crate::workspace::WorkspacePhase;
        use iced::{mouse, Event};
        let mut state = OverlayState {
            workflow: rollshot_capture::Workflow::Screenshot,
            crop: Some(Rectangle {
                x: 10.0,
                y: 10.0,
                width: 50.0,
                height: 40.0,
            }),
            ..OverlayState::default()
        };

        let (effect, _region) = super::update(
            &mut state,
            OverlayMessage::IcedEvent(Event::Mouse(mouse::Event::ButtonReleased(
                mouse::Button::Left,
            ))),
        );

        assert_eq!(effect, super::OverlayEffect::FinishRegion);
        assert_eq!(
            state.workspace.phase(),
            WorkspacePhase::Selected,
            "region release confirms the crop then finalizes immediately"
        );
    }

    #[test]
    fn scrolling_release_begins_stitch_and_confirms() {
        use crate::workspace::WorkspacePhase;
        use iced::{mouse, Event};
        let mut state = OverlayState {
            crop: Some(Rectangle {
                x: 10.0,
                y: 10.0,
                width: 50.0,
                height: 40.0,
            }),
            ..OverlayState::default()
        };

        let (effect, _region) = super::update(
            &mut state,
            OverlayMessage::IcedEvent(Event::Mouse(mouse::Event::ButtonReleased(
                mouse::Button::Left,
            ))),
        );

        assert_eq!(effect, super::OverlayEffect::BeginStitch);
        assert_eq!(state.workspace.phase(), WorkspacePhase::ScrollingCapture);
    }

    #[test]
    fn switching_modes_preserves_selected_crop() {
        use crate::workspace::WorkspacePhase;
        let crop = Rectangle {
            x: 10.0,
            y: 10.0,
            width: 50.0,
            height: 40.0,
        };
        let mut state = OverlayState {
            workflow: rollshot_capture::Workflow::Screenshot,
            crop: Some(crop),
            ..OverlayState::default()
        };
        state.workspace.set_crop(Some(crate::workspace::CropRect {
            x: crop.x,
            y: crop.y,
            width: crop.width,
            height: crop.height,
        }));
        state.workspace.complete_selection();

        let (effect, _) = super::update(
            &mut state,
            OverlayMessage::ToolbarAction(crate::toolbar::ToolbarAction::ScrollingMode),
        );

        assert_eq!(
            effect,
            super::OverlayEffect::ActivateWorkflow(rollshot_capture::Workflow::Scrolling)
        );
        assert_eq!(state.crop, Some(crop));
        assert_eq!(state.workspace.phase(), WorkspacePhase::Selected);
    }

    #[test]
    fn switching_modes_clears_capture_miss_ui_state() {
        use rollshot_overlay_core::capture_miss::CapturedEdge;

        let mut state = OverlayState {
            capture_miss_active: true,
            capture_miss_edge: CapturedEdge::Bottom,
            capture_miss_warn: true,
            capture_miss_message_expires_at: Some(
                std::time::Instant::now() + std::time::Duration::from_secs(3),
            ),
            ..OverlayState::default()
        };

        super::update(
            &mut state,
            OverlayMessage::ToolbarAction(crate::toolbar::ToolbarAction::RegionMode),
        );

        assert!(!state.capture_miss_active);
        assert_eq!(state.capture_miss_edge, CapturedEdge::Unknown);
        assert!(!state.capture_miss_warn);
        assert!(state.capture_miss_message_expires_at.is_none());
    }

    #[test]
    fn scrolling_finish_requests_finalization() {
        let mut state = OverlayState::default();
        state.workspace.begin_scrolling();

        let (effect, _) = super::update(
            &mut state,
            OverlayMessage::ToolbarAction(crate::toolbar::ToolbarAction::Finish),
        );

        assert_eq!(effect, super::OverlayEffect::FinishScrolling);
    }

    #[test]
    fn escape_finalizes_active_scrolling_capture() {
        use iced::{keyboard, Event};
        let mut state = OverlayState::default();
        state.workspace.begin_scrolling();

        let (effect, _) = super::update(
            &mut state,
            OverlayMessage::IcedEvent(Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                modified_key: keyboard::Key::Named(keyboard::key::Named::Escape),
                physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::default(),
                text: None,
                repeat: false,
            })),
        );

        assert_eq!(effect, super::OverlayEffect::FinishScrolling);
    }

    #[test]
    fn escape_cancels_while_selecting() {
        use iced::{keyboard, Event};
        let mut state = OverlayState::default();

        let (effect, _) = super::update(
            &mut state,
            OverlayMessage::IcedEvent(Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                modified_key: keyboard::Key::Named(keyboard::key::Named::Escape),
                physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::default(),
                text: None,
                repeat: false,
            })),
        );

        assert_eq!(effect, super::OverlayEffect::Cancel);
    }

    #[test]
    fn capture_result_constructs_from_image_and_stats_only() {
        // The capture-only result is just the stitched image plus optional
        // stats; there is no post-overlay request field after Task 1.
        let result = crate::CaptureResult {
            image: image::RgbaImage::new(4, 7),
            stats: None,
        };
        assert!(result.stats.is_none());
        assert_eq!(result.image.dimensions(), (4, 7));
    }

    #[test]
    fn manual_toolbar_position_overrides_automatic_placement() {
        let manual = crate::workspace::CropRect {
            x: 120.0,
            y: 140.0,
            width: crate::toolbar::TOOLBAR_WIDTH,
            height: crate::toolbar::TOOLBAR_HEIGHT,
        };
        let state = OverlayState {
            crop: Some(Rectangle {
                x: 10.0,
                y: 20.0,
                width: 300.0,
                height: 200.0,
            }),
            window_size: Some(Size::new(800.0, 600.0)),
            toolbar_position: crate::workspace::ToolbarPosition::Manual(manual),
            ..OverlayState::default()
        };

        assert_eq!(super::toolbar_rect_for(&state), Some(manual.into()));
    }

    #[test]
    fn toolbar_drag_follows_cursor_with_grab_offset() {
        use iced::{mouse, Event};
        let start = crate::workspace::CropRect {
            x: 100.0,
            y: 100.0,
            width: crate::toolbar::TOOLBAR_WIDTH,
            height: crate::toolbar::TOOLBAR_HEIGHT,
        };
        let mut state = OverlayState {
            window_size: Some(Size::new(800.0, 600.0)),
            toolbar_position: crate::workspace::ToolbarPosition::Manual(start),
            cursor_position: Some(Point::new(110.0, 110.0)),
            ..OverlayState::default()
        };

        // Grab the toolbar 10px in from its top-left, then move the cursor.
        super::update(&mut state, OverlayMessage::DragStart);
        super::update(
            &mut state,
            OverlayMessage::IcedEvent(Event::Mouse(mouse::Event::CursorMoved {
                position: Point::new(300.0, 250.0),
            })),
        );

        match state.toolbar_position {
            crate::workspace::ToolbarPosition::Manual(rect) => {
                assert_eq!(rect.x, 290.0);
                assert_eq!(rect.y, 240.0);
            }
            other => panic!("expected manual toolbar position, got {other:?}"),
        }
    }

    #[test]
    fn region_empty_release_stays_in_selection() {
        use crate::workspace::WorkspacePhase;
        use iced::{mouse, Event};
        let mut state = OverlayState {
            workflow: rollshot_capture::Workflow::Screenshot,
            crop: None,
            ..OverlayState::default()
        };

        let (effect, _region) = super::update(
            &mut state,
            OverlayMessage::IcedEvent(Event::Mouse(mouse::Event::ButtonReleased(
                mouse::Button::Left,
            ))),
        );

        assert_eq!(effect, super::OverlayEffect::None);
        assert_eq!(state.workspace.phase(), WorkspacePhase::Selecting);
    }

    #[test]
    fn finish_capture_without_confirmed_crop_returns_none() {
        let mut state = OverlayState::default();

        let (effect, _region) = super::update(&mut state, OverlayMessage::FinishCapture);

        assert_eq!(effect, super::OverlayEffect::None);
    }

    #[test]
    fn capture_control_copy_matches_spec() {
        assert_eq!(super::CAPTURE_STATUS_TEXT, "Capturing - scroll the target");
        assert_eq!(
            super::FOCUS_PAUSED_TEXT,
            "Shortcuts paused - click Rollshot controls to restore Esc"
        );
        assert_eq!(super::FINISH_LABEL, "Finish");
        assert_eq!(super::CANCEL_LABEL, "Cancel");
    }

    #[test]
    fn capture_miss_active_stores_active_recovery_edge() {
        use rollshot_overlay_core::capture_miss::CapturedEdge;

        let mut state = OverlayState::default();
        let miss = rollshot_overlay_core::capture_miss::CaptureMissState {
            active: true,
            warn: true,
            edge: CapturedEdge::Bottom,
            ..Default::default()
        };

        super::update(
            &mut state,
            OverlayMessage::LiveEvent(crate::driver::LiveOverlayEvent::CaptureMiss(miss)),
        );

        assert!(state.capture_miss_active);
        assert_eq!(state.capture_miss_edge, CapturedEdge::Bottom);
    }

    #[test]
    fn capture_miss_clearing_removes_active_edge() {
        use rollshot_overlay_core::capture_miss::CapturedEdge;

        let mut state = OverlayState {
            capture_miss_active: true,
            capture_miss_edge: CapturedEdge::Bottom,
            ..OverlayState::default()
        };
        let miss = rollshot_overlay_core::capture_miss::CaptureMissState {
            active: false,
            warn: false,
            edge: CapturedEdge::Unknown,
            ..Default::default()
        };

        super::update(
            &mut state,
            OverlayMessage::LiveEvent(crate::driver::LiveOverlayEvent::CaptureMiss(miss)),
        );

        assert!(!state.capture_miss_active);
    }

    #[test]
    fn warning_timeout_does_not_clear_active_edge() {
        use rollshot_overlay_core::capture_miss::CapturedEdge;

        let mut state = OverlayState {
            capture_miss_active: true,
            capture_miss_edge: CapturedEdge::Bottom,
            capture_miss_warn: true,
            capture_miss_message_expires_at: Some(
                std::time::Instant::now() - std::time::Duration::from_millis(100),
            ),
            ..OverlayState::default()
        };

        super::update(&mut state, OverlayMessage::Tick);

        assert!(!state.capture_miss_warn);
        assert!(state.capture_miss_active);
        assert_eq!(state.capture_miss_edge, CapturedEdge::Bottom);
    }

    #[test]
    fn recovery_edge_line_returns_correct_endpoints() {
        use rollshot_overlay_core::capture_miss::CapturedEdge;

        let crop = Rectangle {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 80.0,
        };

        // Top edge
        let (p1, p2) = super::recovery_edge_line(crop, CapturedEdge::Top).unwrap();
        assert_eq!(p1, Point::new(10.0, 20.0));
        assert_eq!(p2, Point::new(110.0, 20.0));

        // Bottom edge
        let (p1, p2) = super::recovery_edge_line(crop, CapturedEdge::Bottom).unwrap();
        assert_eq!(p1, Point::new(10.0, 100.0));
        assert_eq!(p2, Point::new(110.0, 100.0));

        // Left edge
        let (p1, p2) = super::recovery_edge_line(crop, CapturedEdge::Left).unwrap();
        assert_eq!(p1, Point::new(10.0, 20.0));
        assert_eq!(p2, Point::new(10.0, 100.0));

        // Right edge
        let (p1, p2) = super::recovery_edge_line(crop, CapturedEdge::Right).unwrap();
        assert_eq!(p1, Point::new(110.0, 20.0));
        assert_eq!(p2, Point::new(110.0, 100.0));

        // Unknown returns None
        assert!(super::recovery_edge_line(crop, CapturedEdge::Unknown).is_none());
    }
}
