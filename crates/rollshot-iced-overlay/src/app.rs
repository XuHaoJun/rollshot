use iced::futures::StreamExt;
use iced::widget::{button, canvas, column, container, image, row, text, Space};
use iced::{
    keyboard, mouse, window, Color, ContentFit, Element, Event, Length, Point, Rectangle, Size,
};
use rollshot_capture::CaptureMode;
use rollshot_overlay_core::preview::PREVIEW_WIDTH;
use rollshot_overlay_core::tokens;
use std::sync::Mutex;

static SHARED_PREVIEW_RX: Mutex<
    Option<iced::futures::channel::mpsc::UnboundedReceiver<crate::driver::LiveOverlayEvent>>,
> = Mutex::new(None);

const SENTINEL_MAGENTA: Color = Color::from_rgba(1.0, 0.0, 1.0, 1.0);
#[allow(dead_code)]
const TOOLBAR_W: f32 = 360.0;
const TOOLBAR_H: f32 = 50.0;
const CHROME_SPACING: f32 = 8.0;
/// Smallest band (px) around the crop that is worth placing chrome in (R3).
const MIN_CHROME_BAND: f32 = 64.0;

const CAPTURE_STATUS_TEXT: &str = "Capturing - scroll the target";
#[allow(dead_code)]
const FOCUS_PAUSED_TEXT: &str = "Shortcuts paused - click Rollshot controls to restore Esc";
const FINISH_LABEL: &str = "Finish";
const CANCEL_LABEL: &str = "Cancel";

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
    PrepareScreenshot,
    FinalizeScrolling,
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

/// Lay out `chrome` in the chosen band so it never overlaps the crop interior
/// (which stays transparent + scroll-through during capture, spec P3.4); `None`
/// if no band has room (caller hides the chrome).
pub(crate) fn place_outside_crop<'a>(
    crop: Rectangle,
    window: iced::Size,
    chrome: Element<'a, OverlayMessage>,
) -> Option<Element<'a, OverlayMessage>> {
    let band = choose_chrome_band(crop, window)?;
    // Anchor the chrome to the crop's near edge so it hugs the crop like a
    // connected popover, on whichever side `choose_chrome_band` found room.
    let crop_x = crop.x.max(0.0);
    let crop_y = crop.y.max(0.0);

    let placed: Element<'a, OverlayMessage> = match band {
        // Directly below the crop, left edge aligned to the crop; grows down.
        Band::Bottom => column![
            Space::new()
                .width(Length::Fill)
                .height(Length::Fixed(crop.y + crop.height)),
            row![
                Space::new()
                    .width(Length::Fixed(crop_x))
                    .height(Length::Shrink),
                chrome,
            ],
        ]
        .into(),
        // Directly above the crop, bottom-anchored to the crop's top; grows up.
        Band::Top => column![
            container(row![
                Space::new()
                    .width(Length::Fixed(crop_x))
                    .height(Length::Shrink),
                chrome,
            ])
            .width(Length::Fill)
            .height(Length::Fixed(crop_y))
            .align_y(iced::Alignment::End),
            Space::new().width(Length::Fill).height(Length::Fill),
        ]
        .into(),
        // Left of the crop, right edge aligned to the crop's left; top aligned.
        Band::Left => row![
            container(column![
                Space::new()
                    .width(Length::Shrink)
                    .height(Length::Fixed(crop_y)),
                chrome,
            ])
            .width(Length::Fixed(crop_x))
            .height(Length::Fill)
            .align_x(iced::Alignment::End),
            Space::new().width(Length::Fill).height(Length::Fill),
        ]
        .into(),
        // Right of the crop, left edge aligned to the crop's right; top aligned.
        Band::Right => row![
            Space::new()
                .width(Length::Fixed(crop.x + crop.width))
                .height(Length::Fill),
            column![
                Space::new()
                    .width(Length::Shrink)
                    .height(Length::Fixed(crop_y)),
                chrome,
            ],
        ]
        .into(),
    };
    Some(placed)
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

/// The full outside-crop band containing the capture chrome. Linux keeps this
/// band interactive because the live preview changes the chrome's final layout,
/// while the selected crop remains pointer-pass-through.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(crate) fn capture_chrome_input_rect(
    crop: Rectangle,
    window: iced::Size,
) -> Option<(i32, i32, i32, i32)> {
    if window.width <= 0.0 || window.height <= 0.0 {
        return None;
    }

    let (x, y, w, h) = match choose_chrome_band(crop, window)? {
        Band::Top => (0.0, 0.0, window.width, crop.y.clamp(0.0, window.height)),
        Band::Bottom => {
            let y = (crop.y + crop.height).clamp(0.0, window.height);
            (0.0, y, window.width, window.height - y)
        }
        Band::Left => (0.0, 0.0, crop.x.clamp(0.0, window.width), window.height),
        Band::Right => {
            let x = (crop.x + crop.width).clamp(0.0, window.width);
            (x, 0.0, window.width - x, window.height)
        }
    };

    (w > 0.0 && h > 0.0).then_some((x as i32, y as i32, w as i32, h as i32))
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

pub(crate) fn magenta_toolbar<'a>(
    content: Element<'a, OverlayMessage>,
) -> Element<'a, OverlayMessage> {
    container(content)
        .padding(8)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(SENTINEL_MAGENTA)),
            ..Default::default()
        })
        .into()
}

pub(crate) fn capture_control_strip<'a>() -> Element<'a, OverlayMessage> {
    magenta_toolbar(
        row![
            text(CAPTURE_STATUS_TEXT).size(16),
            button(FINISH_LABEL).on_press(OverlayMessage::FinishCapture),
            button(CANCEL_LABEL).on_press(OverlayMessage::Cancel),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center)
        .into(),
    )
}

pub(crate) fn view(state: &OverlayState) -> Element<'_, OverlayMessage> {
    let canvas_widget = canvas(CropCanvas::from_state(state))
        .width(Length::Fill)
        .height(Length::Fill);

    if state.workspace.phase() != crate::workspace::WorkspacePhase::Selecting {
        // Capture phase: the base layer (canvas) draws nothing, keeping the
        // crop interior transparent. Chrome goes strictly outside the crop.
        let toolbar = capture_control_strip();
        let crop = state.crop.unwrap_or(Rectangle {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        });
        let window = state.window_size.unwrap_or(iced::Size::new(0.0, 0.0));

        // R5: toolbar is always first so toolbar_input_rect contract holds.
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

        let chrome: Element<'_, OverlayMessage> = {
            let mut col = column![toolbar];
            col = col.spacing(CHROME_SPACING);
            if let Some(w) = warning {
                col = col.push(w);
            }
            if let Some(handle) = &state.preview {
                col = col.push(image(handle.clone()));
            }
            col.into()
        };

        return match place_outside_crop(crop, window, chrome) {
            Some(placed) => iced::widget::stack![canvas_widget, placed].into(),
            None => canvas_widget.into(),
        };
    }

    // Selection phase: drag to pick a crop; toolbar with Cancel.
    let status = match state.crop {
        Some(r) => format!("Crop: {}x{}", r.width as u32, r.height as u32),
        None => "Drag to select crop area".to_string(),
    };
    let toolbar = magenta_toolbar(
        row![
            button("Cancel").on_press(OverlayMessage::Cancel),
            text(status).size(16),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center)
        .into(),
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
pub(crate) fn update(state: &mut OverlayState, message: OverlayMessage) -> OverlayEffect {
    use crate::workspace::WorkspacePhase;

    match message {
        OverlayMessage::WindowOpened { id, size } => {
            state.window_id = Some(id);
            state.window_size = Some(size);
            OverlayEffect::None
        }
        OverlayMessage::IcedEvent(Event::Window(window::Event::Opened { size, .. })) => {
            state.window_size = Some(size);
            OverlayEffect::None
        }
        OverlayMessage::IcedEvent(Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        ))) if state.workspace.phase() == WorkspacePhase::Selecting => {
            state.drag_start = Some(Point::ORIGIN);
            state.crop = None;
            OverlayEffect::None
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
            OverlayEffect::None
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
                match state.mode {
                    CaptureMode::Screenshot => OverlayEffect::PrepareScreenshot,
                    CaptureMode::Scrolling => OverlayEffect::BeginStitch,
                }
            } else {
                OverlayEffect::None
            }
        }
        OverlayMessage::IcedEvent(Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        })) => {
            state.workspace.cancel();
            OverlayEffect::Cancel
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
            match state.mode {
                CaptureMode::Screenshot => OverlayEffect::PrepareScreenshot,
                CaptureMode::Scrolling => OverlayEffect::BeginStitch,
            }
        }
        OverlayMessage::FinishCapture => {
            if state.workspace.phase() == WorkspacePhase::ScrollingCapture {
                state.workspace.finish_scrolling(None);
                OverlayEffect::FinalizeScrolling
            } else {
                OverlayEffect::None
            }
        }
        OverlayMessage::Finish => {
            match state.workspace.phase() {
                WorkspacePhase::ScrollingCapture => {
                    state.workspace.finish_scrolling(None);
                    return OverlayEffect::FinalizeScrolling;
                }
                WorkspacePhase::Selected => {
                    if state.mode == CaptureMode::Screenshot {
                        state.workspace.prepare_screenshot(None);
                        return OverlayEffect::PrepareScreenshot;
                    }
                    // Scrolling in Selected: the runner calls begin_scrolling.
                    return OverlayEffect::None;
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
                        return OverlayEffect::None;
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
                    match state.mode {
                        CaptureMode::Screenshot => {
                            state.workspace.prepare_screenshot(None);
                            OverlayEffect::PrepareScreenshot
                        }
                        CaptureMode::Scrolling => OverlayEffect::BeginStitch,
                    }
                }
                WorkspacePhase::ResultReview => OverlayEffect::None,
            }
        }
        OverlayMessage::ActivateMode(mode) => {
            state.mode = mode;
            state.workspace.activate_mode(mode);
            state.crop = None;
            state.drag_start = None;
            OverlayEffect::ActivateMode(mode)
        }
        OverlayMessage::Cancel => OverlayEffect::Cancel,
        OverlayMessage::LiveEvent(crate::driver::LiveOverlayEvent::Preview(handle)) => {
            state.preview = Some(handle);
            OverlayEffect::None
        }
        OverlayMessage::LiveEvent(crate::driver::LiveOverlayEvent::CaptureMiss(miss)) => {
            if miss.warn {
                state.capture_miss_warn = true;
                state.capture_miss_message_expires_at =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
            }
            OverlayEffect::None
        }
        OverlayMessage::Tick => {
            if state
                .capture_miss_message_expires_at
                .is_some_and(|deadline| std::time::Instant::now() >= deadline)
            {
                state.capture_miss_warn = false;
                state.capture_miss_message_expires_at = None;
            }
            OverlayEffect::None
        }
        _ => OverlayEffect::None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        capture_chrome_input_rect, choose_chrome_band, crop_mask_bands, preview_constraints,
        token_color, toolbar_input_rect, Band, OverlayMessage, OverlayState,
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
    fn capture_chrome_input_rect_covers_top_band_without_crop() {
        let crop = Rectangle {
            x: 40.0,
            y: 300.0,
            width: 720.0,
            height: 260.0,
        };
        let window = Size::new(800.0, 600.0);

        assert_eq!(
            capture_chrome_input_rect(crop, window),
            Some((0, 0, 800, 300))
        );
    }

    #[test]
    fn capture_chrome_input_rect_covers_bottom_band_without_crop() {
        let crop = Rectangle {
            x: 40.0,
            y: 100.0,
            width: 720.0,
            height: 200.0,
        };
        let window = Size::new(800.0, 600.0);

        assert_eq!(
            capture_chrome_input_rect(crop, window),
            Some((0, 300, 800, 300))
        );
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
        let effect = super::update(&mut state, OverlayMessage::Finish);
        assert_eq!(effect, super::OverlayEffect::None);
        assert!(state.warning().is_some());
    }

    #[test]
    fn window_opened_records_window_id_and_size() {
        let mut state = OverlayState::default();
        let id = iced::window::Id::unique();
        let size = Size::new(1440.0, 900.0);

        let effect = super::update(&mut state, OverlayMessage::WindowOpened { id, size });

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

        let effect = super::update(&mut state, OverlayMessage::FinishCapture);

        assert_eq!(effect, super::OverlayEffect::FinalizeScrolling);
    }

    #[test]
    fn selection_finish_still_validates_empty_crop() {
        let mut state = OverlayState::default();

        let effect = super::update(&mut state, OverlayMessage::Finish);

        assert_eq!(effect, super::OverlayEffect::None);
        assert!(state.warning().is_some());
    }

    #[test]
    fn screenshot_release_finishes_immediately_without_confirming() {
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

        let effect = super::update(
            &mut state,
            OverlayMessage::IcedEvent(Event::Mouse(mouse::Event::ButtonReleased(
                mouse::Button::Left,
            ))),
        );

        assert_eq!(effect, super::OverlayEffect::PrepareScreenshot);
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

        let effect = super::update(
            &mut state,
            OverlayMessage::IcedEvent(Event::Mouse(mouse::Event::ButtonReleased(
                mouse::Button::Left,
            ))),
        );

        assert_eq!(effect, super::OverlayEffect::BeginStitch);
        assert_eq!(state.workspace.phase(), WorkspacePhase::Selected);
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

        let effect = super::update(
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

        let effect = super::update(&mut state, OverlayMessage::FinishCapture);

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
