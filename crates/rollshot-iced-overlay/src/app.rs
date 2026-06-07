use iced::futures::StreamExt;
use iced::widget::{canvas, container, image, text};
use iced::{
    keyboard, mouse, window, Color, ContentFit, Element, Event, Length, Point, Rectangle, Size,
};
use rollshot_capture::CaptureMode;
use rollshot_overlay_core::chrome_placement::{self, ChromeRequirements, Rect};
use rollshot_overlay_core::preview::PREVIEW_WIDTH;
use rollshot_overlay_core::tokens;
use std::sync::Mutex;

static SHARED_PREVIEW_RX: Mutex<
    Option<iced::futures::channel::mpsc::UnboundedReceiver<crate::driver::LiveOverlayEvent>>,
> = Mutex::new(None);

#[allow(dead_code)]
const SENTINEL_MAGENTA: Color = Color::from_rgba(1.0, 0.0, 1.0, 1.0);
#[allow(dead_code)]
const TOOLBAR_W: f32 = 360.0;
const TOOLBAR_H: f32 = 50.0;
const CHROME_SPACING: f32 = 8.0;
/// Smallest band (px) around the crop that is worth placing chrome in (R3).
const MIN_CHROME_BAND: f32 = 64.0;

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
    Finish,
    Cancel,
    EnablePassthrough,
    DisablePassthrough,
    ActivateMode(CaptureMode),
    PrepareScreenshot(Option<crate::workspace::OutputAction>),
    FinalizeScrolling(Option<crate::workspace::OutputAction>),
    PerformOutput(crate::workspace::OutputAction),
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
    ActivateMode(CaptureMode),
    ToolbarAction(crate::toolbar::ToolbarAction),
    DragStart(Point),
    DragMove(Point),
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
    /// Active workflow. Screenshot mode finishes immediately on a valid release;
    /// scrolling mode confirms the crop and begins streaming/stitching.
    pub(crate) mode: CaptureMode,
    /// Frozen one-shot background, present only in screenshot mode. Built once by
    /// the platform runner; `view()` clones the cheap handle, never the pixels.
    pub(crate) frozen: Option<image::Handle>,
    /// Toolbar drag state
    pub(crate) toolbar_drag_start: Option<Point>,
    pub(crate) toolbar_position: crate::workspace::ToolbarPosition,
    /// Full-resolution result image for Result Review. The Handle is built once
    /// when entering Result Review and reused per redraw.
    pub(crate) result_handle: Option<image::Handle>,
    pub(crate) result_size: Option<Size>,
    pub(crate) transient_error: Option<String>,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self {
            drag_start: None,
            crop: None,
            workspace: crate::workspace::WorkspaceState::new(CaptureMode::Scrolling),
            preview: None,
            window_id: None,
            mouse_passthrough_active: false,
            window_size: None,
            capture_miss_warn: false,
            capture_miss_message_expires_at: None,
            mode: CaptureMode::Scrolling,
            frozen: None,
            toolbar_drag_start: None,
            toolbar_position: crate::workspace::ToolbarPosition::Automatic,
            result_handle: None,
            result_size: None,
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

pub(crate) fn token_color(c: tokens::Rgba) -> Color {
    Color::from_rgba(
        c.r as f32 / 255.0,
        c.g as f32 / 255.0,
        c.b as f32 / 255.0,
        c.a,
    )
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
}

impl CropCanvas {
    fn from_state(state: &OverlayState) -> Self {
        Self {
            crop: state.crop,
            confirmed: state.workspace.phase() != crate::workspace::WorkspacePhase::Selecting,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Band {
    Top,
    Bottom,
    Left,
    Right,
}

/// R3: during capture, any chrome drawn inside the crop region is self-captured
/// (the portal grabs the whole monitor, this overlay surface included). Pick the
/// largest band of screen *outside* the crop rectangle big enough to host chrome
/// (spec P3.4); `None` if the crop leaves no usable room.
pub(crate) fn choose_chrome_band(crop: Rectangle, window: iced::Size) -> Option<Band> {
    let top = crop.y.max(0.0);
    let bottom = (window.height - (crop.y + crop.height)).max(0.0);
    let left = crop.x.max(0.0);
    let right = (window.width - (crop.x + crop.width)).max(0.0);

    let preferred_side = [(Band::Right, right), (Band::Left, left)]
        .into_iter()
        .filter(|&(_, width)| width >= TOOLBAR_W)
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(band, _)| band);
    if preferred_side.is_some() {
        return preferred_side;
    }

    [
        (Band::Bottom, bottom, window.width * bottom),
        (Band::Top, top, window.width * top),
        (Band::Right, right, right * window.height),
        (Band::Left, left, left * window.height),
    ]
    .into_iter()
    .filter(|&(_, edge, _)| edge >= MIN_CHROME_BAND)
    .max_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
    .map(|(band, _, _)| band)
}

/// The toolbar's interactive rect within the chosen chrome band, in surface-
/// logical px. Plan T6 S3: only the toolbar stays interactive during capture;
/// the crop interior + everything else passes through so the user can scroll the
/// target. Clamped to the band, so it never enters the crop (spec P3.4).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn toolbar_input_rect(
    crop: Rectangle,
    window: iced::Size,
) -> Option<(i32, i32, i32, i32)> {
    if window.width <= 0.0 || window.height <= 0.0 {
        return None;
    }

    let band = choose_chrome_band(crop, window)?;
    let (x, y, w, h) = match band {
        Band::Top => {
            let available_h = crop.y.max(0.0).min(window.height);
            let h = TOOLBAR_H.min(available_h);
            let x = crop.x.max(0.0).min(window.width);
            let y = (available_h - h).max(0.0);
            (x, y, TOOLBAR_W.min((window.width - x).max(0.0)), h)
        }
        Band::Bottom => {
            let by = (crop.y + crop.height).clamp(0.0, window.height);
            let x = crop.x.max(0.0).min(window.width);
            (
                x,
                by,
                TOOLBAR_W.min((window.width - x).max(0.0)),
                TOOLBAR_H.min((window.height - by).max(0.0)),
            )
        }
        Band::Left => {
            let available_w = crop.x.max(0.0).min(window.width);
            let w = TOOLBAR_W.min(available_w);
            let y = crop.y.max(0.0).min(window.height);
            (
                (available_w - w).max(0.0),
                y,
                w,
                TOOLBAR_H.min((window.height - y).max(0.0)),
            )
        }
        Band::Right => {
            let bx = (crop.x + crop.width).clamp(0.0, window.width);
            let y = crop.y.max(0.0).min(window.height);
            (
                bx,
                y,
                TOOLBAR_W.min((window.width - bx).max(0.0)),
                TOOLBAR_H.min((window.height - y).max(0.0)),
            )
        }
    };
    if w <= 0.0 || h <= 0.0 {
        return None;
    }

    Some((x as i32, y as i32, w as i32, h as i32))
}

pub(crate) fn preview_constraints(crop: Rectangle, window: iced::Size) -> PreviewConstraints {
    let band = choose_chrome_band(crop, window);
    let (available_width, available_height) = match band {
        Some(Band::Top) => ((window.width - crop.x.max(0.0)).max(0.0), crop.y.max(0.0)),
        Some(Band::Bottom) => (
            (window.width - crop.x.max(0.0)).max(0.0),
            (window.height - (crop.y + crop.height)).max(0.0),
        ),
        Some(Band::Left) => (crop.x.max(0.0), (window.height - crop.y.max(0.0)).max(0.0)),
        Some(Band::Right) => (
            (window.width - (crop.x + crop.width)).max(0.0),
            (window.height - crop.y.max(0.0)).max(0.0),
        ),
        None => (PREVIEW_WIDTH as f32, 1.0),
    };
    let max_width = (PREVIEW_WIDTH as f32).min(available_width).max(1.0);
    let band_height = (available_height - TOOLBAR_H - CHROME_SPACING).max(1.0) as u32;
    let crop_h = crop.height.max(1.0);
    let max_height = (band_height as f32).min(crop_h);

    PreviewConstraints {
        fixed_width: max_width.floor().max(1.0) as u32,
        max_height: max_height.floor().max(1.0) as u32,
    }
}

fn chrome_placement_for(
    state: &OverlayState,
) -> Option<rollshot_overlay_core::chrome_placement::ChromePlacement> {
    let crop = state.crop?;
    let window = state.window_size?;
    let preview = (state.workspace.phase() == crate::workspace::WorkspacePhase::ScrollingCapture)
        .then(|| {
            state
                .preview
                .as_ref()
                .map(|_| chrome_placement::Size::new(crate::toolbar::TOOLBAR_WIDTH, 100.0))
        })
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

pub(crate) fn view(state: &OverlayState) -> Element<'_, OverlayMessage> {
    let canvas_widget = canvas(CropCanvas::from_state(state))
        .width(Length::Fill)
        .height(Length::Fill);

    if state.workspace.phase() == crate::workspace::WorkspacePhase::ResultReview {
        let toolbar = crate::toolbar::render_toolbar(
            state.workspace.phase(),
            state.mode,
            OverlayMessage::ToolbarAction,
            OverlayMessage::DragStart(Point::ORIGIN),
            OverlayMessage::DragMove,
            OverlayMessage::DragEnd,
        );

        if let Some(handle) = &state.result_handle {
            let result_size = state.result_size.unwrap_or(iced::Size::new(1.0, 1.0));
            let crop = state.crop.unwrap_or(Rectangle {
                x: 0.0,
                y: 0.0,
                width: state.window_size.map_or(800.0, |size| size.width),
                height: state.window_size.map_or(600.0, |size| size.height),
            });
            let layout = crate::result_review::review_layout(
                result_size,
                Size::new(crop.width, crop.height),
            );
            let result_view = crate::result_review::view_result_review(handle, &layout);

            let error: Option<Element<'_, OverlayMessage>> =
                state.transient_error.as_ref().map(|msg| {
                    container(text(msg).size(14))
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

            let mut stack = iced::widget::stack![container(
                container(result_view)
                    .width(Length::Fixed(crop.width))
                    .height(Length::Fixed(crop.height)),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(iced::Padding {
                left: crop.x,
                top: crop.y,
                right: 0.0,
                bottom: 0.0,
            })];

            if let Some(toolbar_rect) = toolbar_rect_for(state) {
                stack = stack.push(
                    container(
                        container(toolbar)
                            .width(Length::Fixed(toolbar_rect.width))
                            .height(Length::Fixed(toolbar_rect.height)),
                    )
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .padding(iced::Padding {
                        left: toolbar_rect.x,
                        top: toolbar_rect.y,
                        right: 0.0,
                        bottom: 0.0,
                    }),
                );
            } else {
                stack = stack.push(
                    container(toolbar)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .align_x(iced::Alignment::Center)
                        .align_y(iced::Alignment::End)
                        .padding(16),
                );
            }

            if let Some(err) = error {
                stack = stack.push(
                    container(err)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .align_x(iced::Alignment::Center)
                        .align_y(iced::Alignment::Start)
                        .padding(16),
                );
            }

            return stack.into();
        }

        return container(toolbar)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::Alignment::Center)
            .align_y(iced::Alignment::Center)
            .into();
    }

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
            state.mode,
            OverlayMessage::ToolbarAction,
            OverlayMessage::DragStart(Point::ORIGIN),
            OverlayMessage::DragMove,
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

        let toolbar_rect = toolbar_rect_for(state).unwrap_or_else(|| placement.toolbar_rect());
        let chrome_visible = toolbar_is_visible(state);

        let toolbar_layer = container(toolbar)
            .width(Length::Fixed(toolbar_rect.width))
            .height(Length::Fixed(toolbar_rect.height))
            .align_x(iced::Alignment::Center)
            .align_y(iced::Alignment::Center);

        let mut chrome_stack = match (&state.frozen, state.mode) {
            (Some(handle), CaptureMode::Screenshot) => iced::widget::stack![
                image(handle.clone())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .content_fit(ContentFit::Fill),
                canvas_widget
            ],
            _ => iced::widget::stack![canvas_widget],
        };

        if chrome_visible {
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

            if state.workspace.phase() == crate::workspace::WorkspacePhase::ScrollingCapture {
                if let Some(handle) = &state.preview {
                    if let Some(preview_rect) = placement.preview_rect() {
                        chrome_stack = chrome_stack.push(
                            container(image(handle.clone()))
                                .width(Length::Fixed(preview_rect.width))
                                .height(Length::Fixed(preview_rect.height))
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
        state.mode,
        OverlayMessage::ToolbarAction,
        OverlayMessage::DragStart(Point::ORIGIN),
        OverlayMessage::DragMove,
        OverlayMessage::DragEnd,
    );

    let toolbar_layer = container(toolbar)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::Alignment::Start)
        .align_y(iced::Alignment::Start)
        .padding(16);

    // Screenshot mode draws the frozen capture as the background, with the dim
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
                let effect = match state.mode {
                    CaptureMode::Screenshot => OverlayEffect::PrepareScreenshot(None),
                    CaptureMode::Scrolling => {
                        state.workspace.begin_scrolling();
                        OverlayEffect::BeginStitch
                    }
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
            state.workspace.cancel();
            (OverlayEffect::Cancel, InputRegionMode::None)
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
            let effect = match state.mode {
                CaptureMode::Screenshot => OverlayEffect::PrepareScreenshot(None),
                CaptureMode::Scrolling => {
                    state.workspace.begin_scrolling();
                    OverlayEffect::BeginStitch
                }
            };
            (effect, InputRegionMode::None)
        }
        OverlayMessage::FinishCapture => {
            if state.workspace.phase() == WorkspacePhase::ScrollingCapture {
                state.workspace.finish_scrolling(None);
                (
                    OverlayEffect::FinalizeScrolling(None),
                    InputRegionMode::None,
                )
            } else {
                (OverlayEffect::None, InputRegionMode::None)
            }
        }
        OverlayMessage::Finish => {
            match state.workspace.phase() {
                WorkspacePhase::ScrollingCapture => {
                    state.workspace.finish_scrolling(None);
                    (
                        OverlayEffect::FinalizeScrolling(None),
                        InputRegionMode::None,
                    )
                }
                WorkspacePhase::Selected => {
                    if state.mode == CaptureMode::Screenshot {
                        state.workspace.prepare_screenshot(None);
                        return (
                            OverlayEffect::PrepareScreenshot(None),
                            InputRegionMode::None,
                        );
                    }
                    // Scrolling in Selected: the runner calls begin_scrolling.
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
                    let effect = match state.mode {
                        CaptureMode::Screenshot => {
                            state.workspace.prepare_screenshot(None);
                            OverlayEffect::PrepareScreenshot(None)
                        }
                        CaptureMode::Scrolling => OverlayEffect::BeginStitch,
                    };
                    (effect, InputRegionMode::None)
                }
                WorkspacePhase::ResultReview => (OverlayEffect::None, InputRegionMode::None),
            }
        }
        OverlayMessage::ActivateMode(mode) => {
            state.mode = mode;
            state.workspace.activate_mode(mode);
            state.drag_start = None;
            let region = match mode {
                CaptureMode::Scrolling => InputRegionMode::ToolbarOnly,
                CaptureMode::Screenshot => InputRegionMode::None,
            };
            (OverlayEffect::ActivateMode(mode), region)
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
            crate::toolbar::ToolbarAction::ScreenshotMode => {
                state.mode = CaptureMode::Screenshot;
                state.workspace.activate_mode(CaptureMode::Screenshot);
                (
                    OverlayEffect::ActivateMode(CaptureMode::Screenshot),
                    InputRegionMode::None,
                )
            }
            crate::toolbar::ToolbarAction::ScrollingMode => {
                state.mode = CaptureMode::Scrolling;
                state.workspace.activate_mode(CaptureMode::Scrolling);
                (
                    OverlayEffect::ActivateMode(CaptureMode::Scrolling),
                    InputRegionMode::None,
                )
            }
            crate::toolbar::ToolbarAction::Finish => match state.workspace.phase() {
                WorkspacePhase::ScrollingCapture => {
                    state.workspace.finish_scrolling(None);
                    (
                        OverlayEffect::FinalizeScrolling(None),
                        InputRegionMode::None,
                    )
                }
                _ => (OverlayEffect::None, InputRegionMode::None),
            },
            crate::toolbar::ToolbarAction::Save => {
                if state.workspace.phase() == WorkspacePhase::ResultReview {
                    (
                        OverlayEffect::PerformOutput(crate::workspace::OutputAction::Save),
                        InputRegionMode::None,
                    )
                } else if state.workspace.phase() == WorkspacePhase::ScrollingCapture {
                    state
                        .workspace
                        .finish_scrolling(Some(crate::workspace::OutputAction::Save));
                    (
                        OverlayEffect::FinalizeScrolling(Some(
                            crate::workspace::OutputAction::Save,
                        )),
                        InputRegionMode::None,
                    )
                } else if state.workspace.phase() == WorkspacePhase::Selected {
                    state
                        .workspace
                        .prepare_screenshot(Some(crate::workspace::OutputAction::Save));
                    (
                        OverlayEffect::PrepareScreenshot(Some(
                            crate::workspace::OutputAction::Save,
                        )),
                        InputRegionMode::None,
                    )
                } else {
                    (OverlayEffect::None, InputRegionMode::None)
                }
            }
            crate::toolbar::ToolbarAction::Copy => {
                if state.workspace.phase() == WorkspacePhase::ResultReview {
                    (
                        OverlayEffect::PerformOutput(crate::workspace::OutputAction::Copy),
                        InputRegionMode::None,
                    )
                } else if state.workspace.phase() == WorkspacePhase::ScrollingCapture {
                    state
                        .workspace
                        .finish_scrolling(Some(crate::workspace::OutputAction::Copy));
                    (
                        OverlayEffect::FinalizeScrolling(Some(
                            crate::workspace::OutputAction::Copy,
                        )),
                        InputRegionMode::None,
                    )
                } else if state.workspace.phase() == WorkspacePhase::Selected {
                    state
                        .workspace
                        .prepare_screenshot(Some(crate::workspace::OutputAction::Copy));
                    (
                        OverlayEffect::PrepareScreenshot(Some(
                            crate::workspace::OutputAction::Copy,
                        )),
                        InputRegionMode::None,
                    )
                } else {
                    (OverlayEffect::None, InputRegionMode::None)
                }
            }
            crate::toolbar::ToolbarAction::Cancel => {
                state.workspace.cancel();
                (OverlayEffect::Cancel, InputRegionMode::None)
            }
            crate::toolbar::ToolbarAction::Close => {
                state.workspace.cancel();
                (OverlayEffect::Cancel, InputRegionMode::None)
            }
        },
        OverlayMessage::DragStart(point) => {
            state.toolbar_drag_start = Some(point);
            state.workspace.auto_hide_mut().set_interacting(true);
            (OverlayEffect::None, InputRegionMode::None)
        }
        OverlayMessage::DragMove(point) => {
            if state.toolbar_drag_start.is_some() {
                let window = state.window_size.unwrap_or(iced::Size::new(0.0, 0.0));
                let viewport = Rect::new(0.0, 0.0, window.width, window.height);
                let toolbar_rect = Rect::new(
                    point.x - crate::toolbar::TOOLBAR_WIDTH / 2.0,
                    point.y - crate::toolbar::TOOLBAR_HEIGHT / 2.0,
                    crate::toolbar::TOOLBAR_WIDTH,
                    crate::toolbar::TOOLBAR_HEIGHT,
                );
                let clamped = crate::toolbar::finish_drag(toolbar_rect, viewport);
                state.toolbar_position = crate::workspace::ToolbarPosition::Manual(clamped);
            }
            (OverlayEffect::None, InputRegionMode::None)
        }
        OverlayMessage::DragEnd => {
            state.toolbar_drag_start = None;
            state.workspace.auto_hide_mut().set_interacting(false);
            (OverlayEffect::None, InputRegionMode::None)
        }
        _ => (OverlayEffect::None, InputRegionMode::None),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        choose_chrome_band, crop_mask_bands, preview_constraints, token_color, toolbar_input_rect,
        Band, OverlayMessage, OverlayState,
    };
    use iced::{Point, Rectangle, Size};
    use rollshot_overlay_core::preview::PREVIEW_WIDTH;

    #[test]
    fn preview_constraints_use_fixed_width_and_bottom_band_height() {
        let crop = Rectangle {
            x: 100.0,
            y: 100.0,
            width: 2400.0,
            height: 900.0,
        };
        let window = Size::new(2560.0, 1440.0);

        let constraints = preview_constraints(crop, window);

        assert_eq!(constraints.fixed_width, PREVIEW_WIDTH);
        assert_eq!(constraints.max_height, 382);
    }

    #[test]
    fn preview_constraints_clamp_width_to_side_band() {
        let crop = Rectangle {
            x: 200.0,
            y: 10.0,
            width: 2300.0,
            height: 1420.0,
        };
        let window = Size::new(2560.0, 1440.0);

        let constraints = preview_constraints(crop, window);

        assert_eq!(constraints.fixed_width, 200);
        assert_eq!(constraints.max_height, 1372);
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
    fn choose_chrome_band_prefers_side_that_fits_controls() {
        let crop = Rectangle {
            x: 100.0,
            y: 100.0,
            width: 700.0,
            height: 200.0,
        };
        let window = Size::new(1200.0, 1000.0);

        assert_eq!(choose_chrome_band(crop, window), Some(Band::Right));
    }

    #[test]
    fn choose_chrome_band_uses_larger_side_when_both_fit_controls() {
        let crop = Rectangle {
            x: 400.0,
            y: 100.0,
            width: 300.0,
            height: 200.0,
        };
        let window = Size::new(1200.0, 1000.0);

        assert_eq!(choose_chrome_band(crop, window), Some(Band::Right));
    }

    #[test]
    fn choose_chrome_band_falls_back_to_largest_band_when_sides_are_too_narrow() {
        let crop = Rectangle {
            x: 100.0,
            y: 200.0,
            width: 500.0,
            height: 100.0,
        };
        let window = Size::new(800.0, 600.0);

        assert_eq!(choose_chrome_band(crop, window), Some(Band::Bottom));
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
    fn toolbar_input_rect_rejects_zero_window_size() {
        let crop = Rectangle {
            x: 10.0,
            y: 80.0,
            width: 100.0,
            height: 100.0,
        };

        assert_eq!(toolbar_input_rect(crop, Size::new(0.0, 0.0)), None);
    }

    #[test]
    fn toolbar_input_rect_uses_control_strip_width() {
        let crop = Rectangle {
            x: 100.0,
            y: 100.0,
            width: 200.0,
            height: 200.0,
        };
        let window = Size::new(800.0, 600.0);

        let rect = toolbar_input_rect(crop, window).expect("toolbar input rect");

        assert_eq!(rect.2, 360);
        assert!(rect.3 > 0);
    }

    #[test]
    fn toolbar_input_rect_aligns_with_bottom_band_toolbar() {
        let crop = Rectangle {
            x: 40.0,
            y: 100.0,
            width: 720.0,
            height: 200.0,
        };
        let window = Size::new(800.0, 600.0);

        let rect = toolbar_input_rect(crop, window).expect("toolbar input rect");

        assert_eq!(rect, (40, 300, 360, 50));
    }

    #[test]
    fn toolbar_input_rect_aligns_with_top_band_toolbar() {
        let crop = Rectangle {
            x: 40.0,
            y: 300.0,
            width: 720.0,
            height: 260.0,
        };
        let window = Size::new(800.0, 600.0);

        let rect = toolbar_input_rect(crop, window).expect("toolbar input rect");

        assert_eq!(rect, (40, 250, 360, 50));
    }

    #[test]
    fn toolbar_input_rect_aligns_with_left_band_toolbar() {
        let crop = Rectangle {
            x: 400.0,
            y: 250.0,
            width: 360.0,
            height: 250.0,
        };
        let window = Size::new(800.0, 600.0);

        let rect = toolbar_input_rect(crop, window).expect("toolbar input rect");

        assert_eq!(rect, (40, 250, 360, 50));
    }

    #[test]
    fn toolbar_input_rect_aligns_with_right_band_toolbar() {
        let crop = Rectangle {
            x: 40.0,
            y: 250.0,
            width: 200.0,
            height: 250.0,
        };
        let window = Size::new(800.0, 600.0);

        let rect = toolbar_input_rect(crop, window).expect("toolbar input rect");

        assert_eq!(rect, (240, 250, 360, 50));
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

        assert_eq!(effect, super::OverlayEffect::FinalizeScrolling(None));
    }

    #[test]
    fn selection_finish_still_validates_empty_crop() {
        let mut state = OverlayState::default();

        let (effect, _region) = super::update(&mut state, OverlayMessage::Finish);

        assert_eq!(effect, super::OverlayEffect::None);
        assert!(state.warning().is_some());
    }

    #[test]
    fn screenshot_release_prepares_result_review_without_exiting() {
        use crate::workspace::WorkspacePhase;
        use iced::{mouse, Event};
        let mut state = OverlayState {
            mode: rollshot_capture::CaptureMode::Screenshot,
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

        assert_eq!(effect, super::OverlayEffect::PrepareScreenshot(None));
        assert_eq!(
            state.workspace.phase(),
            WorkspacePhase::Selected,
            "screenshot release must enter Selected, not ScrollingCapture"
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
            mode: rollshot_capture::CaptureMode::Screenshot,
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
            super::OverlayEffect::ActivateMode(rollshot_capture::CaptureMode::Scrolling)
        );
        assert_eq!(state.crop, Some(crop));
        assert_eq!(state.workspace.phase(), WorkspacePhase::Selected);
    }

    #[test]
    fn direct_output_actions_are_carried_through_finalization() {
        use crate::workspace::OutputAction;
        let mut state = OverlayState::default();
        state.workspace.begin_scrolling();

        let (effect, _) = super::update(
            &mut state,
            OverlayMessage::ToolbarAction(crate::toolbar::ToolbarAction::Copy),
        );

        assert_eq!(
            effect,
            super::OverlayEffect::FinalizeScrolling(Some(OutputAction::Copy))
        );
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
    fn screenshot_empty_release_stays_in_selection() {
        use crate::workspace::WorkspacePhase;
        use iced::{mouse, Event};
        let mut state = OverlayState {
            mode: rollshot_capture::CaptureMode::Screenshot,
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
}
