use iced::widget::{button, canvas, container, row, text};
use iced::{Color, Element, Event, Length, Point, Rectangle, Size, Task, event, keyboard, mouse};
use iced_layershell::Settings;
use iced_layershell::build_pattern::application;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, StartMode};
use iced_layershell::to_layer_message;

const SENTINEL_MAGENTA: Color = Color::from_rgba(1.0, 0.0, 1.0, 1.0);

#[derive(Default)]
pub struct Overlay {
    drag_start: Option<Point>,
    crop: Option<Rectangle>,
}

#[to_layer_message]
#[derive(Debug, Clone)]
pub enum Message {
    IcedEvent(Event),
    Finish,
    Cancel,
}

fn namespace() -> String {
    "rollshot-spike-overlay".to_string()
}

fn subscription(_: &Overlay) -> iced::Subscription<Message> {
    event::listen().map(Message::IcedEvent)
}

fn update(state: &mut Overlay, message: Message) -> Task<Message> {
    match message {
        Message::IcedEvent(Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))) => {
            // Note: without cursor position from the event we store None;
            // the CursorMoved event below will set drag_start on first move.
            // This is a known limitation — the crop drag starts on first move after click.
            state.drag_start = Some(Point::ORIGIN);
            state.crop = None;
            Task::none()
        }
        Message::IcedEvent(Event::Mouse(mouse::Event::CursorMoved { position })) => {
            if let Some(start) = state.drag_start {
                // Update drag_start on first move if it was set to ORIGIN
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
            Task::none()
        }
        Message::Cancel => {
            std::process::exit(0);
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
    let status = match state.crop {
        Some(r) => format!("Crop: {}x{}", r.width as u32, r.height as u32),
        None => "Drag to select crop area".to_string(),
    };

    let canvas_widget = canvas(CropCanvas { crop: state.crop })
        .width(Length::Fill)
        .height(Length::Fill);

    let toolbar = container(
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
    });

    iced::widget::stack![
        canvas_widget,
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
pub fn run(start_mode: StartMode) -> Result<(), iced_layershell::Error> {
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
