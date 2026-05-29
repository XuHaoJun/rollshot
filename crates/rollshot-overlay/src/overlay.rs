use iced::widget::{button, canvas, container, image, row, text};
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

static PREVIEW_RX: Mutex<Option<iced::futures::channel::mpsc::UnboundedReceiver<image::Handle>>> =
    Mutex::new(None);
static RESULT_SLOT: Mutex<Option<Result<Option<CaptureResult>, String>>> = Mutex::new(None);

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
            // If the driver is running (crop was confirmed), finalize it.
            if let Some(driver) = state.driver.take() {
                match driver.finalize() {
                    Ok(result) => {
                        *RESULT_SLOT.lock().unwrap() = Some(Ok(Some(result)));
                    }
                    Err(e) => {
                        *RESULT_SLOT.lock().unwrap() = Some(Err(e));
                    }
                }
            } else {
                *RESULT_SLOT.lock().unwrap() = Some(Ok(None));
            }
            iced::exit()
        }
        Message::IcedEvent(Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Enter),
            ..
        })) if !state.crop_confirmed && state.crop.is_some() => Task::done(Message::Finish),
        Message::Finish => {
            state.crop_confirmed = true;

            // Start the real driver with the crop region.
            let crop = state.crop.unwrap_or(Rectangle {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            });
            let crop_logical = LogicalRect {
                x: crop.x,
                y: crop.y,
                width: crop.width,
                height: crop.height,
            };

            // Use the actual overlay surface size captured from the window
            // Opened event. The overlay is fullscreen (layer-shell anchors all
            // four edges), so this equals the screen size.
            let ws = state.window_size.unwrap_or(iced::Size {
                width: crop.x + crop.width,
                height: crop.y + crop.height,
            });
            let overlay_logical = rollshot_capture::Size {
                width: ws.width as u32,
                height: ws.height as u32,
            };

            let cfg: OverlayConfig = (*OVERLAY_CFG.lock().unwrap().as_ref().unwrap()).clone();
            let preview_tx = PREVIEW_TX.lock().unwrap().take().unwrap();

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
                    state.driver = Some(driver);
                }
                Err(e) => {
                    *RESULT_SLOT.lock().unwrap() = Some(Err(e));
                    return iced::exit();
                }
            }

            // Narrow input region to just the toolbar (top-left, ~300x50).
            Task::done(Message::SetInputRegion(ActionCallback::new(|region| {
                region.add(16, 16, 300, 50);
            })))
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

fn view(state: &Overlay) -> Element<'_, Message> {
    let status = if state.crop_confirmed {
        "Scroll passthrough active".to_string()
    } else {
        match state.crop {
            Some(r) => format!("Crop: {}x{}", r.width as u32, r.height as u32),
            None => "Drag to select crop area".to_string(),
        }
    };

    let canvas_widget = canvas(CropCanvas {
        crop: state.crop,
        confirmed: state.crop_confirmed,
    })
    .width(Length::Fill)
    .height(Length::Fill);

    let content: Element<'_, Message> = if let Some(handle) = &state.preview {
        iced::widget::stack![
            image(handle.clone())
                .width(Length::Fill)
                .height(Length::Fill),
            canvas_widget,
        ]
        .into()
    } else {
        canvas_widget.into()
    };

    let toolbar = if state.crop_confirmed {
        container(
            row![text(status).size(16)]
                .spacing(12)
                .align_y(iced::Alignment::Center),
        )
        .padding(8)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(SENTINEL_MAGENTA)),
            ..Default::default()
        })
    } else {
        container(
            row![
                button("Finish").on_press(Message::Finish),
                button("Cancel").on_press(Message::Cancel),
                text(status).size(16),
            ]
            .spacing(12)
            .align_y(iced::Alignment::Center),
        )
        .padding(8)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(SENTINEL_MAGENTA)),
            ..Default::default()
        })
    };

    iced::widget::stack![
        content,
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

// Holds the preview sender so the update function can hand it to Driver::start.
static PREVIEW_TX: Mutex<Option<iced::futures::channel::mpsc::UnboundedSender<image::Handle>>> =
    Mutex::new(None);

// Holds the overlay config so the update function can access it.
static OVERLAY_CFG: Mutex<Option<OverlayConfig>> = Mutex::new(None);

pub fn run(config: OverlayConfig) -> Result<Option<CaptureResult>, OverlayError> {
    let (preview_tx, preview_rx) = iced::futures::channel::mpsc::unbounded();

    *PREVIEW_RX.lock().unwrap() = Some(preview_rx);
    *PREVIEW_TX.lock().unwrap() = Some(preview_tx);
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
