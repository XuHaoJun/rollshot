use iced::widget::{button, canvas, column, container, image, row, text, Space};
use iced::{
    event, keyboard, mouse, window, Color, Element, Event, Length, Point, Rectangle, Size, Task,
};
use iced_layershell::actions::ActionCallback;
use iced_layershell::build_pattern::application;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, StartMode};
use iced_layershell::to_layer_message;
use iced_layershell::Settings;

use iced::futures::StreamExt;
use std::sync::Mutex;

use crate::coords::LogicalRect;
use crate::driver::Driver;
use crate::CaptureResult;
use crate::OverlayConfig;
use crate::OverlayError;
use rollshot_overlay_core::preview::PREVIEW_WIDTH;
use rollshot_overlay_core::tokens;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PreviewConstraints {
    pub(crate) fixed_width: u32,
    pub(crate) max_height: u32,
}

const SENTINEL_MAGENTA: Color = Color::from_rgba(1.0, 0.0, 1.0, 1.0);
const TOOLBAR_W: f32 = 300.0;
const TOOLBAR_H: f32 = 50.0;
const CHROME_SPACING: f32 = 8.0;
/// Smallest band (px) around the crop that is worth placing chrome in (R3).
const MIN_CHROME_BAND: f32 = 64.0;

static PREVIEW_RX: Mutex<
    Option<iced::futures::channel::mpsc::UnboundedReceiver<crate::driver::LiveOverlayEvent>>,
> = Mutex::new(None);
static RESULT_SLOT: Mutex<Option<Result<Option<CaptureResult>, String>>> = Mutex::new(None);

// Capture starts in `run()` before the overlay surface exists, so the portal
// screen-share picker dialog appears + dismisses on a clean desktop and never
// lands in a captured frame. The live Driver is stashed here for the update fn
// to drive: `begin_stitch` on Finish, `finalize`/`cancel` on Esc.
static DRIVER_SLOT: Mutex<Option<Driver>> = Mutex::new(None);

#[derive(Default)]
pub struct Overlay {
    drag_start: Option<Point>,
    crop: Option<Rectangle>,
    crop_confirmed: bool,
    preview: Option<image::Handle>,
    window_size: Option<iced::Size>,
    capture_miss_warn: bool,
    capture_miss_message_expires_at: Option<std::time::Instant>,
}

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
    "rollshot-overlay".to_string()
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
                    state.crop = Some(Rectangle {
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
                // Capturing: stop the threads and produce the finalized result.
                (true, Some(driver)) => driver.finalize().map(Some),
                // Esc before a crop was confirmed: cancel + tear down capture.
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
            // Ignore duplicate Finish (e.g. double-click / repeated Enter): the
            // crop is already confirmed and stitching has begun.
            if state.crop_confirmed {
                return Task::none();
            }
            // Require a non-empty crop; otherwise keep selecting.
            let crop = match state.crop {
                Some(c) if c.width >= 1.0 && c.height >= 1.0 => c,
                _ => return Task::none(),
            };
            // Require a known surface size — it is the denominator of the
            // crop->frame scale, so a missing one would silently mis-scale.
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

            // Capture is already running (started in `run()` before the overlay
            // appeared, so the picker dialog is long gone). Just map the crop to
            // frame pixels and start stitching live frames from here on.
            let preview_constraints = preview_constraints(crop, ws);
            if let Some(driver) = DRIVER_SLOT.lock().unwrap().as_mut() {
                driver.begin_stitch(crop_logical, overlay_logical, preview_constraints);
            }

            // Keep only the toolbar interactive (plan T6 S3); the crop interior
            // + everything else passes through so the user can scroll the
            // target. The toolbar sits in the chrome band outside the crop, so
            // this never overlaps the crop region (spec P3.4).
            let input_rect = toolbar_input_rect(crop, ws);
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

fn token_color(c: tokens::Rgba) -> Color {
    Color::from_rgba(
        c.r as f32 / 255.0,
        c.g as f32 / 255.0,
        c.b as f32 / 255.0,
        c.a,
    )
}

fn crop_mask_bands(crop: Rectangle, bounds: Rectangle) -> [(Point, Size); 4] {
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

struct CropCanvas {
    crop: Option<Rectangle>,
    confirmed: bool,
}

impl canvas::Program<Message> for CropCanvas {
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

enum Band {
    Top,
    Bottom,
    Left,
    Right,
}

/// R3: during capture, any chrome drawn inside the crop region is self-captured
/// (the portal grabs the whole monitor, this overlay surface included). Pick the
/// largest band of screen *outside* the crop rectangle big enough to host chrome
/// (spec P3.4); `None` if the crop leaves no usable room.
fn choose_chrome_band(crop: Rectangle, window: iced::Size) -> Option<Band> {
    let top = crop.y.max(0.0);
    let bottom = (window.height - (crop.y + crop.height)).max(0.0);
    let left = crop.x.max(0.0);
    let right = (window.width - (crop.x + crop.width)).max(0.0);

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
fn place_outside_crop<'a>(
    crop: Rectangle,
    window: iced::Size,
    chrome: Element<'a, Message>,
) -> Option<Element<'a, Message>> {
    let band = choose_chrome_band(crop, window)?;
    // Anchor the chrome to the crop's near edge so it hugs the crop like a
    // connected popover, on whichever side `choose_chrome_band` found room.
    let crop_x = crop.x.max(0.0);
    let crop_y = crop.y.max(0.0);

    let placed: Element<'a, Message> = match band {
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
fn toolbar_input_rect(crop: Rectangle, window: iced::Size) -> Option<(i32, i32, i32, i32)> {
    let band = choose_chrome_band(crop, window)?;
    let (x, y, w, h) = match band {
        Band::Top => (
            0.0,
            0.0,
            TOOLBAR_W.min(window.width),
            TOOLBAR_H.min(crop.y.max(0.0)),
        ),
        Band::Bottom => {
            let by = crop.y + crop.height;
            (
                0.0,
                by,
                TOOLBAR_W.min(window.width),
                TOOLBAR_H.min((window.height - by).max(0.0)),
            )
        }
        Band::Left => (
            0.0,
            0.0,
            TOOLBAR_W.min(crop.x.max(0.0)),
            TOOLBAR_H.min(window.height),
        ),
        Band::Right => {
            let bx = crop.x + crop.width;
            (
                bx,
                0.0,
                TOOLBAR_W.min((window.width - bx).max(0.0)),
                TOOLBAR_H.min(window.height),
            )
        }
    };
    Some((x as i32, y as i32, w as i32, h as i32))
}

fn preview_constraints(crop: Rectangle, window: iced::Size) -> PreviewConstraints {
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

fn magenta_toolbar<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    container(content)
        .padding(8)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(SENTINEL_MAGENTA)),
            ..Default::default()
        })
        .into()
}

fn view(state: &Overlay) -> Element<'_, Message> {
    let canvas_widget = canvas(CropCanvas {
        crop: state.crop,
        confirmed: state.crop_confirmed,
    })
    .width(Length::Fill)
    .height(Length::Fill);

    if state.crop_confirmed {
        // Capture phase: the base layer (canvas) draws nothing, keeping the
        // crop interior transparent. Chrome goes strictly outside the crop.
        let toolbar = magenta_toolbar(
            text("Capturing — scroll the target, Esc to finish")
                .size(16)
                .into(),
        );
        let crop = state.crop.unwrap_or(Rectangle {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        });
        let window = state.window_size.unwrap_or(iced::Size::new(0.0, 0.0));

        // R5: toolbar is always first so toolbar_input_rect contract holds.
        let warning: Option<Element<'_, Message>> = state.capture_miss_warn.then(|| {
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

        let chrome: Element<'_, Message> = {
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
            button("Cancel").on_press(Message::Cancel),
            text(status).size(16),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center)
        .into(),
    );

    iced::widget::stack![
        canvas_widget,
        container(toolbar)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::Alignment::Start)
            .align_y(iced::Alignment::Start)
            .padding(16),
    ]
    .into()
}

fn style(_state: &Overlay, theme: &iced::Theme) -> iced::theme::Style {
    iced::theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: theme.palette().text,
    }
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
        .style(style)
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

#[cfg(test)]
mod tests {
    use super::{crop_mask_bands, preview_constraints, token_color};
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
}
