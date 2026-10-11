//! The browser demo's world: niri's default config, a laptop with an
//! external display, a few open windows and what a typical system has
//! installed. Tests use it too.

use std::path::PathBuf;

use crate::disk::Disk;
use crate::niri::{EventKind, OpenWindow, Running};
use crate::outputs::{MonitorInfo, OutputInfo, RawMode, modes};
use crate::store::{Store, StoreError};
use crate::system::{InstalledApp, SystemData, XkbItem};

/// niri 26.04's default config, as a fresh install has it.
pub const CONFIG: &str = include_str!("../demo/config.kdl");

/// Where the demo's config is said to be.
pub fn path() -> PathBuf {
    PathBuf::from("/home/demo/.config/niri/config.kdl")
}

pub fn store() -> Result<Store, StoreError> {
    Store::from_text(path(), CONFIG)
}

/// The demo's disk: its config, in memory.
pub fn disk() -> Disk {
    Disk::memory([(path(), CONFIG.to_string())])
}

/// A 14 inch laptop at 150% with a 27 inch display to its right.
pub fn outputs() -> Vec<OutputInfo> {
    let mode = |width, height, millihertz, preferred| RawMode {
        width,
        height,
        millihertz,
        preferred,
    };
    vec![
        OutputInfo {
            name: "DP-1".into(),
            description: "Dell Inc. U2723QE".into(),
            modes: modes(&[
                mode(3840, 2160, 60_000, true),
                mode(3840, 2160, 30_000, false),
                mode(2560, 1440, 59_951, false),
                mode(1920, 1080, 60_000, false),
            ]),
            preferred: Some(0),
            current: Some(0),
            vrr_supported: false,
            vrr_enabled: false,
            rect: Some(MonitorInfo {
                x: 1920.0,
                y: 0.0,
                width: 2560.0,
                height: 1440.0,
            }),
            scale: 1.5,
        },
        OutputInfo {
            name: "eDP-1".into(),
            description: "BOE NE140QDM".into(),
            modes: modes(&[
                mode(2880, 1800, 120_000, true),
                mode(2880, 1800, 60_000, false),
            ]),
            preferred: Some(0),
            current: Some(0),
            vrr_supported: true,
            vrr_enabled: true,
            rect: Some(MonitorInfo {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1200.0,
            }),
            scale: 1.5,
        },
    ]
}

pub fn running() -> Running {
    let window = |app_id: &str, title: &str, floating, focused| OpenWindow {
        id: 0,
        app_id: app_id.into(),
        title: title.into(),
        floating,
        focused,
    };
    Running {
        outputs: outputs(),
        windows: vec![
            window("Alacritty", "~/code/niri-center", false, true),
            window("firefox", "niri wiki: Configuration", false, false),
            window("org.gnome.Calculator", "Calculator", true, false),
            window("firefox", "Picture-in-Picture", true, false),
        ],
        layers: vec![
            "waybar on DP-1 (Top)".into(),
            "waybar on eDP-1 (Top)".into(),
            "mako on DP-1 (Overlay)".into(),
        ],
    }
}

/// A few lines for the Events page, newest first.
pub fn events() -> Vec<(EventKind, String)> {
    [
        (
            EventKind::Windows,
            "Alacritty opened on workspace 1 of DP-1",
        ),
        (EventKind::Focus, "Focus moved to Alacritty"),
        (EventKind::Workspaces, "Workspace 2 on DP-1 became active"),
        (EventKind::Input, "Keyboard layout switched to English (US)"),
    ]
    .into_iter()
    .map(|(kind, text)| (kind, text.to_string()))
    .collect()
}

/// A slice of what xkeyboard-config, icon themes and launchers offer.
pub fn system() -> SystemData {
    let layout = |code: &str, description: &str| XkbItem {
        code: code.into(),
        description: description.into(),
        group: String::new(),
    };
    let option = |code: &str, description: &str, group: &str| XkbItem {
        code: code.into(),
        description: description.into(),
        group: group.into(),
    };
    let app = |name: &str, command: &str| InstalledApp {
        id: command.into(),
        name: name.into(),
        command: command.into(),
    };
    SystemData {
        layouts: vec![
            layout("cz", "Czech"),
            layout("dk", "Danish"),
            layout("nl", "Dutch"),
            layout("gb", "English (UK)"),
            layout("us", "English (US)"),
            layout("fi", "Finnish"),
            layout("fr", "French"),
            layout("de", "German"),
            layout("it", "Italian"),
            layout("jp", "Japanese"),
            layout("no", "Norwegian"),
            layout("pl", "Polish"),
            layout("pt", "Portuguese"),
            layout("ru", "Russian"),
            layout("es", "Spanish"),
            layout("se", "Swedish"),
            layout("ch", "Swiss German"),
            layout("ua", "Ukrainian"),
        ],
        options: vec![
            option(
                "grp:alt_shift_toggle",
                "Alt+Shift",
                "Switching to another layout",
            ),
            option(
                "grp:caps_toggle",
                "Caps Lock",
                "Switching to another layout",
            ),
            option(
                "grp:win_space_toggle",
                "Win+Space",
                "Switching to another layout",
            ),
            option("ctrl:nocaps", "Caps Lock as Ctrl", "Ctrl position"),
            option("ctrl:swapcaps", "Swap Ctrl and Caps Lock", "Ctrl position"),
            option(
                "caps:escape",
                "Make Caps Lock an additional Esc",
                "Caps Lock behavior",
            ),
            option("compose:ralt", "Right Alt", "Position of Compose key"),
        ],
        cursor_themes: vec![
            "Adwaita".into(),
            "Bibata-Modern-Classic".into(),
            "breeze_cursors".into(),
        ],
        apps: vec![
            app("Alacritty", "alacritty"),
            app("Firefox", "firefox"),
            app("mako", "mako"),
            app("swayidle", "swayidle"),
            app("Waybar", "waybar"),
        ],
        desktop_ids: ["alacritty", "firefox", "org.keepassxc.keepassxc", "steam"]
            .map(String::from)
            .into(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_demo_config_is_niris_default_and_valid() {
        let store = super::store().unwrap();
        assert!(store.binding_rows().len() > 50, "the default binds load");
        if crate::validate::niri_available() {
            let probes = tempfile::tempdir().unwrap();
            let disk = crate::disk::Disk::files(crate::disk::Folders::under(probes.path()));
            disk.check(&store.job()).unwrap();
        }
    }
}
