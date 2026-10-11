//! niri-center's window, drawn with iced. Everything else (the config,
//! saving and its history, the settings table, what niri and the system
//! report) is the `niri_center` library.

mod app;
mod color;
mod controls;
mod curves;
mod highlight;
mod history;
mod icons;
mod keyboard;
mod library;
mod message;
mod monitors;
mod pages;
mod repair;
mod rows;
mod shortcuts;
mod source;
mod subscriptions;
mod tasks;
mod theme;
#[cfg(test)]
mod ui_tests;
mod update;
mod view;

use iced::{Element, Theme, window};
use niri_center::page::Page;

use app::App;
use message::Message;

type El<'a> = Element<'a, Message, Theme, iced::Renderer>;

/// Faces a browser lends the app none of: monospace for config paths and
/// diffs, and the semibold that headings use (iced brings Fira Sans Regular).
#[cfg(target_arch = "wasm32")]
const FONTS: [&[u8]; 2] = [
    include_bytes!("../demo/FiraMono-Regular.ttf"),
    include_bytes!("../demo/FiraSans-SemiBold.ttf"),
];

/// Labels, values in fields and lists, and buttons share one text size;
/// the headless tests draw with the same settings. No MSAA in the browser:
/// over WebGL it leaves a page's canvas drawings behind on the next
/// (iced-rs/iced#2400).
fn settings() -> iced::Settings {
    iced::Settings {
        default_text_size: iced::Pixels(14.0),
        antialiasing: !cfg!(target_arch = "wasm32"),
        ..iced::Settings::default()
    }
}

fn title(state: &App) -> String {
    state.pane.map_or_else(
        || "niri-center".to_string(),
        |page| format!("niri-center: {}", page.label()),
    )
}

/// Follows the desktop's dark or light preference.
fn app_theme(state: &App) -> Theme {
    theme::for_mode(state.mode, state.high_contrast)
}

/// The window: the full app, or one page alone (`--pane`).
pub fn run(pane: Option<Page>) -> iced::Result {
    // `niri-center` for the full app, `niri-center.<pane>` for a single
    // pane: distinct ids let niri window rules target them separately
    #[cfg(not(target_arch = "wasm32"))]
    let platform_specific = window::settings::PlatformSpecific {
        application_id: pane.map_or_else(|| "niri-center".to_string(), Page::app_id),
        ..Default::default()
    };
    #[cfg(target_arch = "wasm32")]
    let platform_specific = window::settings::PlatformSpecific::default();
    iced::application(move || App::boot(pane), update::step, view::view)
        .window(window::Settings {
            platform_specific,
            exit_on_close_request: false,
            ..Default::default()
        })
        .subscription(subscriptions::subscription)
        .title(title)
        .theme(app_theme)
        .settings(settings())
        .run()
}

/// The browser demo's entry, called once the wasm module is loaded.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() -> Result<(), wasm_bindgen::JsValue> {
    run(None).map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))
}
