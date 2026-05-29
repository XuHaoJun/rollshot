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
const PREVIEW_MAX_EDGE: u32 = 480;
/// Smallest band (px) around the crop that is worth placing chrome in (R3).
const MIN_CHROME_BAND: f32 = 64.0;

static PREVIEW_RX: Mutex<Option<iced::futures::channel::mpsc::UnboundedReceiver<image::Handle>>> =
    Mutex::new(None);
static RESULT_SLOT: Mutex<Option<Result<Option<CaptureResult>, String>>> = Mutex::new(None);

// Holds the preview sender so the update function can hand it to Driver::start.
static PREVIEW_TX: Mutex<Option<iced::futures::channel::mpsc::UnboundedSender<image::Handle>>> =
    Mutex::new(None);

// Holds the overlay config so the update function can access it.
static OVERLAY_CFG: Mutex<Option<OverlayConfig>> = Mutex::new(None);

// Driver start runs on a background thread (so portal negotiation + the
// first-frame wait never block the iced event loop). The started Driver is
// stashed here and a readiness signal is sent over the channel below.
static DRIVER_SLOT: Mutex<Option<Driver>> = Mutex::new(None);
static DRIVER_RX: Mutex<
    Option<iced::futures::channel::mpsc::UnboundedReceiver<Result<(), String>>>,
> = Mutex::new(None);
static DRIVER_TX: Mutex<Option<iced::futures::channel::mpsc::UnboundedSender<Result<(), String>>>> =
    Mutex::new(None);

#[derive(Default)]
pub struct Overlay {
    drag_start: Option<Point>,
    crop: Option<Rectangle>,
    crop_confirmed: bool,
    preview: Option<image::Handle>,
    driver: Option<Driver>,
    window_size: Option<iced::Size>,
}

#[to_layer_message]
#[derive(Debug, Clone)]
pub enum Message {
    IcedEvent(Event),
    Finish,
    Cancel,
    NewPreview(image::Handle),
    /// The background driver-start finished: `Ok` means the Driver is in
    /// `DRIVER_SLOT`; `Err` carries the failure message.
    DriverStarted(Result<(), String>),
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

fn driver_stream() -> iced::Subscription<Message> {
    iced::Subscription::run(|| {
        let rx = DRIVER_RX
            .lock()
            .unwrap()
            .take()
            .expect("driver channel already consumed");

        rx.map(Message::DriverStarted)
    })
}

fn subscription(_: &Overlay) -> iced::Subscription<Message> {
    iced::Subscription::batch([
        event::listen().map(Message::IcedEvent),
        preview_stream(),
        driver_stream(),
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
            Task::none()
        }
        Message::IcedEvent(Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        })) => {
            // Finalize the driver if one is running (or just finished starting,
            // racing the DriverStarted message); otherwise this is a cancel.
            let driver = state
                .driver
                .take()
                .or_else(|| DRIVER_SLOT.lock().unwrap().take());
            match driver {
                Some(driver) => match driver.finalize() {
                    Ok(result) => {
                        *RESULT_SLOT.lock().unwrap() = Some(Ok(Some(result)));
                    }
                    Err(e) => {
                        *RESULT_SLOT.lock().unwrap() = Some(Err(e));
                    }
                },
                None => {
                    *RESULT_SLOT.lock().unwrap() = Some(Ok(None));
                }
            }
            iced::exit()
        }
        Message::IcedEvent(Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Enter),
            ..
        })) if !state.crop_confirmed && state.crop.is_some() => Task::done(Message::Finish),
        Message::Finish => {
            // Ignore duplicate Finish (e.g. double-click / repeated Enter): a
            // second pass would re-take the preview sender and panic.
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

            let cfg: OverlayConfig = OVERLAY_CFG.lock().unwrap().as_ref().unwrap().clone();
            let preview_tx = PREVIEW_TX.lock().unwrap().take().unwrap();
            let driver_tx = DRIVER_TX.lock().unwrap().take().unwrap();

            // Start the driver off the UI thread: Driver::start does portal
            // negotiation and blocks until the first frame, which would
            // otherwise freeze the overlay (it holds an exclusive keyboard
            // grab). Report readiness back via DriverStarted.
            std::thread::spawn(move || {
                match Driver::start(
                    &cfg.backend,
                    cfg.fps,
                    cfg.show_cursor,
                    crop_logical,
                    overlay_logical,
                    preview_tx,
                    PREVIEW_MAX_EDGE,
                ) {
                    Ok(driver) => {
                        *DRIVER_SLOT.lock().unwrap() = Some(driver);
                        let _ = driver_tx.unbounded_send(Ok(()));
                    }
                    Err(e) => {
                        let _ = driver_tx.unbounded_send(Err(e));
                    }
                }
            });

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
        Message::DriverStarted(Ok(())) => {
            if let Some(driver) = DRIVER_SLOT.lock().unwrap().take() {
                state.driver = Some(driver);
            }
            Task::none()
        }
        Message::DriverStarted(Err(msg)) => {
            *RESULT_SLOT.lock().unwrap() = Some(Err(msg));
            iced::exit()
        }
        Message::Cancel => {
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
    let top = crop.y.max(0.0);
    let bottom = (window.height - (crop.y + crop.height)).max(0.0);
    let left = crop.x.max(0.0);
    let right = (window.width - (crop.x + crop.width)).max(0.0);

    let placed: Element<'a, Message> = match band {
        Band::Top => column![
            container(chrome)
                .width(Length::Fill)
                .height(Length::Fixed(top)),
            Space::new().width(Length::Fill).height(Length::Fill),
        ]
        .into(),
        Band::Bottom => column![
            Space::new()
                .width(Length::Fill)
                .height(Length::Fixed(crop.y + crop.height)),
            container(chrome)
                .width(Length::Fill)
                .height(Length::Fixed(bottom)),
        ]
        .into(),
        Band::Left => row![
            container(chrome)
                .width(Length::Fixed(left))
                .height(Length::Fill),
            Space::new().width(Length::Fill).height(Length::Fill),
        ]
        .into(),
        Band::Right => row![
            Space::new()
                .width(Length::Fixed(crop.x + crop.width))
                .height(Length::Fill),
            container(chrome)
                .width(Length::Fixed(right))
                .height(Length::Fill),
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
    const TOOLBAR_W: f32 = 300.0;
    const TOOLBAR_H: f32 = 50.0;
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
        let status = if state.driver.is_some() {
            "Capturing — scroll the target, Esc to finish"
        } else {
            "Starting capture…"
        };
        let toolbar = magenta_toolbar(text(status).size(16).into());
        let chrome: Element<'_, Message> = if let Some(handle) = &state.preview {
            column![
                toolbar,
                image(handle.clone())
                    .width(Length::Fill)
                    .height(Length::Fill),
            ]
            .spacing(8)
            .into()
        } else {
            toolbar
        };

        let crop = state.crop.unwrap_or(Rectangle {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        });
        let window = state.window_size.unwrap_or(iced::Size::new(0.0, 0.0));

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
    let (driver_tx, driver_rx) = iced::futures::channel::mpsc::unbounded();

    *PREVIEW_RX.lock().unwrap() = Some(preview_rx);
    *PREVIEW_TX.lock().unwrap() = Some(preview_tx);
    *DRIVER_RX.lock().unwrap() = Some(driver_rx);
    *DRIVER_TX.lock().unwrap() = Some(driver_tx);
    *DRIVER_SLOT.lock().unwrap() = None;
    *OVERLAY_CFG.lock().unwrap() = Some(config);
    *RESULT_SLOT.lock().unwrap() = None;

    application(Overlay::default, namespace, update, view)
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
        .run()
        .map_err(|e| OverlayError::Overlay(e.to_string()))?;

    // After the iced app exits, read the result slot.
    match RESULT_SLOT.lock().unwrap().take().unwrap_or(Ok(None)) {
        Ok(opt) => Ok(opt),
        Err(e) => Err(OverlayError::Capture(e)),
    }
}
