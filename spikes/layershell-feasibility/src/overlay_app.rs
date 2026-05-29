use iced::widget::{container, text};
use iced::{Color, Element, Event, Length, Task, event, keyboard};
use iced_layershell::Settings;
use iced_layershell::build_pattern::application;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, StartMode};
use iced_layershell::to_layer_message;

#[derive(Default)]
pub struct Overlay;

#[to_layer_message]
#[derive(Debug, Clone)]
pub enum Message {
    IcedEvent(Event),
}

fn namespace() -> String {
    "rollshot-spike-overlay".to_string()
}

fn subscription(_: &Overlay) -> iced::Subscription<Message> {
    event::listen().map(Message::IcedEvent)
}

fn update(_state: &mut Overlay, message: Message) -> Task<Message> {
    match message {
        Message::IcedEvent(Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        })) => {
            std::process::exit(0);
        }
        _ => Task::none(),
    }
}

fn view(_state: &Overlay) -> Element<'_, Message> {
    container(text("rollshot overlay (Esc to quit)").size(24))
        .width(Length::Fill)
        .height(Length::Fill)
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
