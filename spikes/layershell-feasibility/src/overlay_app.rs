use iced::widget::{button, canvas, container, image, row, text};
use iced::{Color, Element, Event, Length, Point, Rectangle, Size, Task, event, keyboard, mouse};
use iced_layershell::Settings;
use iced_layershell::actions::ActionCallback;
use iced_layershell::build_pattern::application;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, StartMode};
use iced_layershell::to_layer_message;

use std::sync::Mutex;

const SENTINEL_MAGENTA: Color = Color::from_rgba(1.0, 0.0, 1.0, 1.0);

static PREVIEW_RX: Mutex<Option<std::sync::mpsc::Receiver<image::Handle>>> = Mutex::new(None);

#[derive(Default)]
pub struct Overlay {
    drag_start: Option<Point>,
    crop: Option<Rectangle>,
    crop_confirmed: bool,
    preview: Option<image::Handle>,
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
    "rollshot-spike-overlay".to_string()
}

fn preview_stream() -> iced::Subscription<Message> {
    iced::Subscription::run(|| {
        let rx = PREVIEW_RX
            .lock()
            .unwrap()
            .take()
            .expect("preview channel already consumed");

        iced::futures::stream::unfold(rx, |rx| async move {
            let handle = rx.recv().ok()?;
            Some((Message::NewPreview(handle), rx))
        })
    })
}

fn subscription(_: &Overlay) -> iced::Subscription<Message> {
    iced::Subscription::batch([
        event::listen().map(Message::IcedEvent),
        preview_stream(),
    ])
}

fn update(state: &mut Overlay, message: Message) -> Task<Message> {
    match message {
        Message::IcedEvent(Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))) => {
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
                    state.crop = Some(Rectangle { x, y, width: w, height: h });
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
            std::process::exit(0);
        }
        Message::Finish => {
            eprintln!("crop confirmed: {:?}", state.crop);
            state.crop_confirmed = true;
            Task::done(Message::SetInputRegion(ActionCallback::new(|region| {
                region.add(16, 16, 300, 50);
            })))
        }
        Message::Cancel => {
            std::process::exit(0);
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

    let canvas_widget = canvas(CropCanvas { crop: state.crop })
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
            .align_x(iced::Alignment::End)
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

/// `start_mode` lets callers target a specific output by name (Task 8).
/// `rx` receives preview image handles from an external producer thread.
pub fn run(
    start_mode: StartMode,
    rx: std::sync::mpsc::Receiver<image::Handle>,
) -> Result<(), iced_layershell::Error> {
    *PREVIEW_RX.lock().unwrap() = Some(rx);

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
                start_mode,
                events_transparent: false,
            },
            ..Default::default()
        })
        .run()
}
