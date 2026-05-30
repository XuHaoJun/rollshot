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

const SENTINEL_MAGENTA: Color = Color::from_rgba(1.0, 0.0, 1.0, 1.0);
// Fixed preview width (matches wayscrollshot's PREVIEW_MAX_WIDTH). Combined with
// PREVIEW_MAX_HEIGHT this keeps the per-frame preview texture small. iced's
// layer-shell/wgpu path renders ≤480px previews stably (the Phase 2 spike's
// 200×200 swatch and the old ≤480 downscale), but ~960×~1380 textures flicker /
// never composite — so the viewport is bounded to that proven-stable envelope.
const PREVIEW_WIDTH: u32 = 280;
const PREVIEW_MAX_HEIGHT: u32 = 480;
const TOOLBAR_W: f32 = 300.0;
const TOOLBAR_H: f32 = 50.0;
const CHROME_SPACING: f32 = 8.0;
/// Smallest band (px) around the crop that is worth placing chrome in (R3).
const MIN_CHROME_BAND: f32 = 64.0;

static PREVIEW_RX: Mutex<Option<iced::futures::channel::mpsc::UnboundedReceiver<image::Handle>>> =
    Mutex::new(None);
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
}

#[to_layer_message]
#[derive(Debug, Clone)]
pub enum Message {
    IcedEvent(Event),
    Finish,
    Cancel,
    NewPreview(image::Handle),
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

        rx.map(Message::NewPreview)
    })
}

fn subscription(_: &Overlay) -> iced::Subscription<Message> {
    iced::Subscription::batch([event::listen().map(Message::IcedEvent), preview_stream()])
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
            Task::none()
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
            let preview_size = preview_viewport_size(crop, ws);
            if let Some(driver) = DRIVER_SLOT.lock().unwrap().as_mut() {
                driver.begin_stitch(crop_logical, overlay_logical, preview_size);
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
        Message::NewPreview(handle) => {
            state.preview = Some(handle);
            Task::none()
        }
        _ => Task::none(),
    }
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
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        // R3: draw nothing inside the crop region during capture phase.
        if !self.confirmed {
            if let Some(crop) = self.crop {
                let stroke = canvas::Stroke::default()
                    .with_color(Color::WHITE)
                    .with_width(2.0);
                frame.stroke_rectangle(
                    Point::new(crop.x, crop.y),
                    Size::new(crop.width, crop.height),
                    stroke,
                );
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

fn preview_viewport_size(crop: Rectangle, window: iced::Size) -> rollshot_capture::Size {
    let band = choose_chrome_band(crop, window);
    // Space actually available from the crop's anchor edge to the screen edge,
    // matching where `place_outside_crop` pins the chrome (so the capped preview
    // never overflows past the screen).
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
    let width = (PREVIEW_WIDTH as f32).min(available_width).max(1.0) as u32;
    let band_height = (available_height - TOOLBAR_H - CHROME_SPACING).max(1.0) as u32;
    // Cap the height so the texture stays in the proven-stable envelope; a tall
    // side band would otherwise produce a ~280×1380 preview that flickers.
    let height = band_height.clamp(1, PREVIEW_MAX_HEIGHT);

    rollshot_capture::Size { width, height }
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

        let chrome: Element<'_, Message> = if let Some(handle) = &state.preview {
            // The driver builds the handle as a fixed-width, bottom-anchored
            // viewport that grows up to a cap (driver::preview_viewport_handle),
            // so rendering it at its natural size makes the preview grow with the
            // scroll and then follow the bottom once capped.
            column![toolbar, image(handle.clone())]
                .spacing(CHROME_SPACING)
                .into()
        } else {
            toolbar
        };

        return match place_outside_crop(crop, window, chrome) {
            Some(placed) => iced::widget::stack![canvas_widget, placed].into(),
            None => canvas_widget.into(),
        };
    }

    // Selection phase: drag to pick a crop; toolbar with Finish/Cancel.
    let status = match state.crop {
        Some(r) => format!("Crop: {}x{}", r.width as u32, r.height as u32),
        None => "Drag to select crop area".to_string(),
    };
    let toolbar = magenta_toolbar(
        row![
            button("Finish").on_press(Message::Finish),
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
                exclusive_zone: 0,
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
    use super::{
        preview_viewport_size, CHROME_SPACING, PREVIEW_MAX_HEIGHT, PREVIEW_WIDTH, TOOLBAR_H,
    };
    use iced::{Rectangle, Size};

    #[test]
    fn preview_viewport_uses_fixed_width_and_bottom_band_height() {
        let crop = Rectangle {
            x: 100.0,
            y: 100.0,
            width: 2400.0,
            height: 900.0,
        };
        let window = Size::new(2560.0, 1440.0);

        let viewport = preview_viewport_size(crop, window);

        assert_eq!(viewport.width, PREVIEW_WIDTH);
        assert_eq!(viewport.height, (440.0 - TOOLBAR_H - CHROME_SPACING) as u32);
    }

    #[test]
    fn preview_viewport_clamps_width_to_side_band() {
        let crop = Rectangle {
            x: 200.0,
            y: 10.0,
            width: 2300.0,
            height: 1420.0,
        };
        let window = Size::new(2560.0, 1440.0);

        let viewport = preview_viewport_size(crop, window);

        // A tall side band offers ~1382px of height, but the preview texture is
        // capped so the per-frame upload stays small enough to render without
        // flicker on the iced_layershell/wgpu path (the larger 960×~1380
        // textures flickered / never showed).
        assert_eq!(viewport.width, 200);
        assert_eq!(viewport.height, PREVIEW_MAX_HEIGHT);
    }
}
