//! What the app listens to besides clicks: the desktop's theme and contrast,
//! its own keys and the window's size. niri's events come from the niri
//! actor.

use iced::futures::channel::mpsc::Sender;
use iced::{Subscription, stream, window};
use niri_center::page::Page;

use crate::app::App;
use crate::message::Message;
use crate::{keyboard, shortcuts};

/// The app's own keys and the window's size, from the raw events.
fn app_events(
    event: iced::Event,
    status: iced::event::Status,
    _window: window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Keyboard(key) => {
            shortcuts::from_event(&key, status == iced::event::Status::Captured)
                .map(Message::Shortcut)
        }
        iced::Event::Window(window::Event::Opened { size, .. } | window::Event::Resized(size)) => {
            Some(Message::WindowResized(size))
        }
        iced::Event::Mouse(_)
        | iced::Event::Window(_)
        | iced::Event::Touch(_)
        | iced::Event::InputMethod(_) => None,
    }
}

/// Hand `message` on; `false` once nobody listens any more.
fn send(sender: &mut Sender<Message>, message: Message) -> bool {
    sender.try_send(message).is_ok()
}

/// Every later change of the desktop's contrast preference.
fn contrast_changes() -> Subscription<Message> {
    Subscription::run_with("desktop-contrast", |_| {
        stream::channel(4, |mut sender: Sender<Message>| async move {
            niri_center::portal::watch_contrast(|high| {
                send(&mut sender, Message::ContrastLoaded(high))
            })
            .await;
        })
    })
}

/// A key pressed while the Keybinds page finds keys: the key on the
/// picture. Escape stops finding instead.
fn found_keys(
    event: iced::Event,
    _status: iced::event::Status,
    _window: window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) => {
            keyboard::token_for(&key)
                .filter(|token| token != "Escape")
                .map(move |value| {
                    Message::Keybinds(crate::pages::keybinds::Message::KeyFound(value))
                })
        }
        iced::Event::Keyboard(_)
        | iced::Event::Mouse(_)
        | iced::Event::Window(_)
        | iced::Event::Touch(_)
        | iced::Event::InputMethod(_) => None,
    }
}

/// Theme changes, shortcuts and close requests always; a 2-second refresh
/// on Windows; a second's tick while a toast or a display trial counts
/// down.
pub fn subscription(state: &App) -> Subscription<Message> {
    let mut subscriptions = vec![
        iced::system::theme_changes().map(Message::ModeLoaded),
        // while a key is being found, a key press is that and not a search
        if state.keybinds.finding_keys {
            iced::event::listen_with(|event, status, window| {
                app_events(event, status, window).filter(|message| {
                    !matches!(message, Message::Shortcut(shortcuts::Shortcut::Type(_)))
                })
            })
        } else {
            iced::event::listen_with(app_events)
        },
        window::close_requests().map(|_| Message::CloseRequested),
        contrast_changes(),
    ];
    match state.page {
        Page::Windows => subscriptions.push(
            iced::time::every(std::time::Duration::from_secs(2)).map(|_| Message::RunningTick),
        ),
        Page::Keybinds if state.keybinds.finding_keys => {
            subscriptions.push(iced::event::listen_with(found_keys));
        }
        Page::Keybinds
        | Page::Events
        | Page::Rules
        | Page::Layout
        | Page::Displays
        | Page::Workspaces
        | Page::Overview
        | Page::Animations
        | Page::Input
        | Page::Gestures
        | Page::Startup
        | Page::Environment
        | Page::Cursor
        | Page::Screenshots
        | Page::History
        | Page::Files
        | Page::Source => {}
    }
    if state.status.as_ref().is_some_and(|status| !status.error) {
        subscriptions
            .push(iced::time::every(std::time::Duration::from_secs(1)).map(|_| Message::ToastTick));
    }
    if state.session.trial.is_some() {
        subscriptions
            .push(iced::time::every(std::time::Duration::from_secs(1)).map(|_| Message::TrialTick));
    }
    Subscription::batch(subscriptions)
}
