//! The rule library: ready-made window rules for things people often ask
//! niri for, most of them for one app, each naming where it comes from.
//! Marked rules are imported in one change, into a file of their own that
//! the config includes, or into the config itself.
//!
//! Every rule is checked against `niri validate` by the test suite.

use std::collections::HashSet;

use crate::store::{RuleMeaning, Store, StoreError, WindowRuleRow};
use crate::system::SystemData;

/// Where imported rules go when not into the config itself.
pub const FILE: &str = "rules-library.kdl";

/// An app the library has rules for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnownApp {
    NiriCenter,
    Firefox,
    Wezterm,
    Steam,
    Keepassxc,
    Secrets,
    Bitwarden,
    OnePassword,
    Telegram,
    Gimp,
    Pavucontrol,
    Pwvucontrol,
    GnomeCalculator,
    Kcalc,
    NetworkConnections,
    Blueman,
    QtSettings,
    Mpv,
    Obs,
    Fcitx,
}

impl KnownApp {
    pub const ALL: [Self; 20] = [
        Self::NiriCenter,
        Self::Firefox,
        Self::Wezterm,
        Self::Steam,
        Self::Keepassxc,
        Self::Secrets,
        Self::Bitwarden,
        Self::OnePassword,
        Self::Telegram,
        Self::Gimp,
        Self::Pavucontrol,
        Self::Pwvucontrol,
        Self::GnomeCalculator,
        Self::Kcalc,
        Self::NetworkConnections,
        Self::Blueman,
        Self::QtSettings,
        Self::Mpv,
        Self::Obs,
        Self::Fcitx,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::NiriCenter => "niri-center",
            Self::Firefox => "Firefox",
            Self::Wezterm => "WezTerm",
            Self::Steam => "Steam",
            Self::Keepassxc => "KeePassXC",
            Self::Secrets => "Secrets",
            Self::Bitwarden => "Bitwarden",
            Self::OnePassword => "1Password",
            Self::Telegram => "Telegram",
            Self::Gimp => "GIMP",
            Self::Pavucontrol => "Volume control (pavucontrol)",
            Self::Pwvucontrol => "Volume control (pwvucontrol)",
            Self::GnomeCalculator => "Calculator",
            Self::Kcalc => "KCalc",
            Self::NetworkConnections => "Network connections",
            Self::Blueman => "Bluetooth (Blueman)",
            Self::QtSettings => "Qt settings (qt5ct, qt6ct)",
            Self::Mpv => "mpv",
            Self::Obs => "OBS Studio",
            Self::Fcitx => "Fcitx5",
        }
    }

    /// The app IDs niri sees on its windows, across the ways it is
    /// packaged; `niri-center` also covers `niri-center.displays`.
    pub const fn ids(self) -> &'static [&'static str] {
        match self {
            Self::NiriCenter => &["niri-center"],
            Self::Firefox => &["firefox", "org.mozilla.firefox"],
            Self::Wezterm => &["org.wezfurlong.wezterm"],
            Self::Steam => &["steam"],
            Self::Keepassxc => &["org.keepassxc.KeePassXC"],
            Self::Secrets => &["org.gnome.World.Secrets"],
            Self::Bitwarden => &["Bitwarden"],
            Self::OnePassword => &["1Password"],
            Self::Telegram => &["org.telegram.desktop"],
            Self::Gimp => &["gimp"],
            Self::Pavucontrol => &["org.pulseaudio.pavucontrol", "pavucontrol"],
            Self::Pwvucontrol => &["com.saivert.pwvucontrol"],
            Self::GnomeCalculator => &["org.gnome.Calculator", "gnome-calculator"],
            Self::Kcalc => &["org.kde.kcalc"],
            Self::NetworkConnections => &["nm-connection-editor"],
            Self::Blueman => &["blueman-manager"],
            Self::QtSettings => &["qt5ct", "qt6ct"],
            Self::Mpv => &["mpv"],
            Self::Obs => &["com.obsproject.Studio"],
            Self::Fcitx => &["fcitx"],
        }
    }

    /// Desktop file names it installs under when they differ from its ids.
    const fn desktop(self) -> &'static [&'static str] {
        match self {
            Self::NiriCenter => &[],
            Self::Firefox => &[],
            Self::Wezterm => &[],
            Self::Steam => &["com.valvesoftware.Steam"],
            Self::Keepassxc => &[],
            Self::Secrets => &[],
            Self::Bitwarden => &["com.bitwarden.desktop"],
            Self::OnePassword => &[],
            Self::Telegram => &[],
            Self::Gimp => &["org.gimp.GIMP"],
            Self::Pavucontrol => &[],
            Self::Pwvucontrol => &[],
            Self::GnomeCalculator => &[],
            Self::Kcalc => &[],
            Self::NetworkConnections => &[],
            Self::Blueman => &[],
            Self::QtSettings => &[],
            Self::Mpv => &["io.mpv.Mpv"],
            Self::Obs => &[],
            Self::Fcitx => &["org.fcitx.Fcitx5", "fcitx5"],
        }
    }

    /// Whether a window with this app ID is this app's, ignoring case.
    pub fn owns(self, app_id: &str) -> bool {
        self.ids().iter().any(|id| is_app(app_id, id))
    }

    /// Whether it is installed here, by its desktop files.
    pub fn installed(self, system: &SystemData) -> bool {
        self.ids()
            .iter()
            .chain(self.desktop())
            .any(|id| system.has_app(id))
    }
}

/// Whether `app_id` is `id`, or one of its windows named `id.something`.
fn is_app(app_id: &str, id: &str) -> bool {
    app_id.len() >= id.len()
        && app_id.is_char_boundary(id.len())
        && app_id[..id.len()].eq_ignore_ascii_case(id)
        && matches!(app_id.as_bytes().get(id.len()), None | Some(b'.'))
}

/// One ready-made rule.
pub struct LibraryRule {
    /// The app it is for; `None`: windows of any app.
    pub app: Option<KnownApp>,
    pub name: &'static str,
    pub why: &'static str,
    pub kdl: &'static str,
    /// Where it comes from.
    pub source: &'static str,
    /// Windows it is for, as (app ID, title): the tests check that the
    /// rule reaches each, and that each of its app's IDs is among them.
    pub examples: &'static [(&'static str, &'static str)],
}

/// Shared by the small apps that open, get used and close again.
const SMALL_WINDOW: &str =
    "A window you open, use and close again; tiled, it would take a whole column.";

pub const LIBRARY: [LibraryRule; 30] = [
    // ---- every window ----
    LibraryRule {
        app: None,
        name: "Rounded corners on every window",
        why: "The focus ring and border follow the corners. Apps that draw their own shadows may look clipped.",
        kdl: r##"window-rule {
    geometry-corner-radius 12
    clip-to-geometry true
}"##,
        source: "niri's default config",
        examples: &[],
    },
    LibraryRule {
        app: None,
        name: "Blur behind see-through windows",
        why: "Shows only where a window is transparent, such as a terminal with a translucent background.",
        kdl: r##"window-rule {
    background-effect {
        blur true
    }
}"##,
        source: "niri-center",
        examples: &[],
    },
    LibraryRule {
        app: None,
        name: "Fade windows that do not have focus",
        why: "Makes the active window stand out without a border.",
        kdl: r##"window-rule {
    match is-active=false
    opacity 0.9
}"##,
        source: "niri-center",
        examples: &[],
    },
    LibraryRule {
        app: None,
        name: "Glow around windows that ask for attention",
        why: "In niri's urgent red, also with the focus ring and border off. Such a window stays fully opaque, even where another rule fades it.",
        kdl: r##"window-rule {
    match is-urgent=true
    opacity 1.0
    shadow {
        on
        softness 40
        spread 8
        draw-behind-window true
        color "#9b0000cc"
    }
}"##,
        source: "niri-center",
        examples: &[],
    },
    LibraryRule {
        app: None,
        name: "Red ring around a window being screencast",
        why: "When you share a single window, you can see which one, even with the focus ring off.",
        kdl: r##"window-rule {
    match is-window-cast-target=true
    focus-ring {
        on
        active-color "#f38ba8"
        inactive-color "#7d0d2d"
    }
}"##,
        source: "niri wiki",
        examples: &[],
    },
    LibraryRule {
        app: None,
        name: "Picture-in-Picture videos float",
        why: "Firefox and Chromium name the window Picture-in-Picture.",
        kdl: r##"window-rule {
    match title="^Picture-in-Picture$"
    open-floating true
}"##,
        source: "niri's default config",
        examples: &[
            ("firefox", "Picture-in-Picture"),
            ("org.mozilla.firefox", "Picture-in-Picture"),
        ],
    },
    LibraryRule {
        app: None,
        name: "File choosers float",
        why: "niri floats a dialog its app claims. Some apps open the file chooser without saying whose it is, and then it tiles.",
        kdl: r##"window-rule {
    match app-id=r#"^xdg-desktop-portal"#
    open-floating true
}"##,
        source: "niri issues 2831 and 3895",
        examples: &[
            ("xdg-desktop-portal-gtk", "Open File"),
            ("xdg-desktop-portal-gnome", "Save File"),
        ],
    },
    LibraryRule {
        app: None,
        name: "SSH passphrase prompts float",
        why: "The prompt ssh-askpass opens when a key needs its passphrase.",
        kdl: r##"window-rule {
    match title="^OpenSSH Authentication Passphrase request$"
    open-floating true
}"##,
        source: "shared niri configs",
        examples: &[("", "OpenSSH Authentication Passphrase request")],
    },
    // ---- apps ----
    LibraryRule {
        app: Some(KnownApp::NiriCenter),
        name: "Pages in their own window open floating",
        why: "A page opened on its own (Open in its own window, or --pane) floats; the main window keeps tiling.",
        kdl: crate::store::SUGGESTED_WINDOW_RULES,
        source: "niri-center",
        examples: &[("niri-center.displays", "Displays")],
    },
    LibraryRule {
        app: Some(KnownApp::Firefox),
        name: "Private windows stay out of screen recordings",
        why: "They show as a black box to anyone watching a screen share.",
        kdl: r##"window-rule {
    match app-id=r#"firefox$"# title="Private Browsing"
    block-out-from "screencast"
}"##,
        source: "niri-center",
        examples: &[
            ("firefox", "Mozilla Firefox Private Browsing"),
            ("org.mozilla.firefox", "Mozilla Firefox Private Browsing"),
        ],
    },
    LibraryRule {
        app: Some(KnownApp::Firefox),
        name: "The sharing indicator floats",
        why: "The small bar Firefox shows while your camera, microphone or screen is shared.",
        kdl: r##"window-rule {
    match app-id=r#"firefox$"# title="Sharing Indicator$"
    open-floating true
}"##,
        source: "shared niri configs",
        examples: &[
            ("firefox", "Firefox \u{2014} Sharing Indicator"),
            ("org.mozilla.firefox", "Firefox \u{2014} Sharing Indicator"),
        ],
    },
    LibraryRule {
        app: Some(KnownApp::Wezterm),
        name: "Opens at the size it asks for",
        why: "WezTerm waits for a size of its own before it shows, so without this its window may never appear.",
        kdl: r##"window-rule {
    match app-id=r#"^org\.wezfurlong\.wezterm$"#
    default-column-width {}
}"##,
        source: "niri's default config",
        examples: &[("org.wezfurlong.wezterm", "wezterm")],
    },
    LibraryRule {
        app: Some(KnownApp::Steam),
        name: "Notifications float in the bottom-right corner",
        why: "Steam opens its notifications as windows of their own, in the middle of the screen.",
        kdl: r##"window-rule {
    match app-id="steam" title=r#"^notificationtoasts_\d+_desktop$"#
    default-floating-position x=10 y=10 relative-to="bottom-right"
}"##,
        source: "niri wiki",
        examples: &[("steam", "notificationtoasts_12_desktop")],
    },
    LibraryRule {
        app: Some(KnownApp::Keepassxc),
        name: "Hidden from screen recordings",
        why: "The password list shows as a black box to anyone watching a screen share.",
        kdl: r##"window-rule {
    match app-id=r#"^org\.keepassxc\.KeePassXC$"#
    block-out-from "screencast"
}"##,
        source: "niri's default config",
        examples: &[("org.keepassxc.KeePassXC", "Passwords.kdbx - KeePassXC")],
    },
    LibraryRule {
        app: Some(KnownApp::Keepassxc),
        name: "The unlock dialog takes focus",
        why: "So you can type the password as soon as a browser asks KeePassXC for one.",
        kdl: r##"window-rule {
    match app-id=r#"^org\.keepassxc\.KeePassXC$"# title="^Unlock Database - KeePassXC$"
    open-focused true
}"##,
        source: "niri wiki",
        examples: &[("org.keepassxc.KeePassXC", "Unlock Database - KeePassXC")],
    },
    LibraryRule {
        app: Some(KnownApp::Secrets),
        name: "Hidden from screen recordings",
        why: "The password list shows as a black box to anyone watching a screen share.",
        kdl: r##"window-rule {
    match app-id=r#"^org\.gnome\.World\.Secrets$"#
    block-out-from "screencast"
}"##,
        source: "niri's default config",
        examples: &[("org.gnome.World.Secrets", "Secrets")],
    },
    LibraryRule {
        app: Some(KnownApp::Bitwarden),
        name: "Hidden from screen recordings",
        why: "The vault shows as a black box to anyone watching a screen share.",
        kdl: r##"window-rule {
    match app-id=r#"^Bitwarden$"#
    block-out-from "screencast"
}"##,
        source: "niri-center",
        examples: &[("Bitwarden", "Bitwarden")],
    },
    LibraryRule {
        app: Some(KnownApp::OnePassword),
        name: "Hidden from screen recordings",
        why: "The vault shows as a black box to anyone watching a screen share.",
        kdl: r##"window-rule {
    match app-id=r#"^1Password$"#
    block-out-from "screencast"
}"##,
        source: "niri-center",
        examples: &[("1Password", "1Password")],
    },
    LibraryRule {
        app: Some(KnownApp::Telegram),
        name: "The media viewer opens in a window",
        why: "Telegram shows photos and videos fullscreen; this keeps them in a window you can tile.",
        kdl: r##"window-rule {
    match app-id=r#"^org\.telegram\.desktop$"# title="^Media viewer$"
    open-fullscreen false
}"##,
        source: "niri wiki",
        examples: &[("org.telegram.desktop", "Media viewer")],
    },
    LibraryRule {
        app: Some(KnownApp::Gimp),
        name: "The startup splash does not take focus",
        why: "You can keep working in another window while GIMP starts.",
        kdl: r##"window-rule {
    match app-id="^gimp" title="^GIMP Startup$"
    open-focused false
}"##,
        source: "niri wiki",
        examples: &[("gimp", "GIMP Startup")],
    },
    LibraryRule {
        app: Some(KnownApp::Pavucontrol),
        name: "Opens floating",
        why: SMALL_WINDOW,
        kdl: r##"window-rule {
    match app-id=r#"^(org\.pulseaudio\.)?pavucontrol$"#
    open-floating true
}"##,
        source: "shared niri configs",
        examples: &[
            ("org.pulseaudio.pavucontrol", "Volume Control"),
            ("pavucontrol", "Volume Control"),
        ],
    },
    LibraryRule {
        app: Some(KnownApp::Pwvucontrol),
        name: "Opens floating",
        why: SMALL_WINDOW,
        kdl: r##"window-rule {
    match app-id=r#"^com\.saivert\.pwvucontrol$"#
    open-floating true
}"##,
        source: "shared niri configs",
        examples: &[("com.saivert.pwvucontrol", "Pipewire Volume Control")],
    },
    LibraryRule {
        app: Some(KnownApp::GnomeCalculator),
        name: "Opens floating",
        why: SMALL_WINDOW,
        kdl: r##"window-rule {
    match app-id=r#"^(org\.gnome\.Calculator|gnome-calculator)$"#
    open-floating true
}"##,
        source: "shared niri configs",
        examples: &[
            ("org.gnome.Calculator", "Calculator"),
            ("gnome-calculator", "Calculator"),
        ],
    },
    LibraryRule {
        app: Some(KnownApp::Kcalc),
        name: "Opens floating",
        why: SMALL_WINDOW,
        kdl: r##"window-rule {
    match app-id=r#"^org\.kde\.kcalc$"#
    open-floating true
}"##,
        source: "shared niri configs",
        examples: &[("org.kde.kcalc", "KCalc")],
    },
    LibraryRule {
        app: Some(KnownApp::NetworkConnections),
        name: "Opens floating",
        why: SMALL_WINDOW,
        kdl: r##"window-rule {
    match app-id="^nm-connection-editor$"
    open-floating true
}"##,
        source: "shared niri configs",
        examples: &[("nm-connection-editor", "Network Connections")],
    },
    LibraryRule {
        app: Some(KnownApp::Blueman),
        name: "Opens floating",
        why: SMALL_WINDOW,
        kdl: r##"window-rule {
    match app-id="^blueman-manager$"
    open-floating true
}"##,
        source: "shared niri configs",
        examples: &[("blueman-manager", "Bluetooth Devices")],
    },
    LibraryRule {
        app: Some(KnownApp::QtSettings),
        name: "Opens floating",
        why: SMALL_WINDOW,
        kdl: r##"window-rule {
    match app-id="^qt[56]ct$"
    open-floating true
}"##,
        source: "shared niri configs",
        examples: &[
            ("qt5ct", "Qt5 Configuration Tool"),
            ("qt6ct", "Qt6 Configuration Tool"),
        ],
    },
    LibraryRule {
        app: Some(KnownApp::Mpv),
        name: "Variable refresh rate while it shows",
        why: "On a display set to on-demand VRR, video plays at its own frame rate while mpv is on screen.",
        kdl: r##"window-rule {
    match app-id="^mpv$"
    variable-refresh-rate true
}"##,
        source: "niri wiki",
        examples: &[("mpv", "video.mkv - mpv")],
    },
    LibraryRule {
        app: Some(KnownApp::Obs),
        name: "Keeps a width its controls fit in",
        why: "Narrower than this, OBS lays its controls over each other.",
        kdl: r##"window-rule {
    match app-id=r#"^com\.obsproject\.Studio$"#
    min-width 876
}"##,
        source: "niri wiki",
        examples: &[("com.obsproject.Studio", "OBS 31.0.0 - Profile: Untitled")],
    },
    LibraryRule {
        app: Some(KnownApp::Fcitx),
        name: "The input window floats",
        why: "Otherwise niri tiles Fcitx5's candidate list as a column of its own.",
        kdl: r##"window-rule {
    match app-id="^fcitx$" title="^Fcitx5 Input Window$"
    open-floating true
}"##,
        source: "niri issue 3917",
        examples: &[("fcitx", "Fcitx5 Input Window")],
    },
];

/// The library rules for one app, by its app ID: `niri-center.displays`
/// counts as niri-center.
pub fn for_app(app_id: &str) -> Vec<usize> {
    (0..LIBRARY.len())
        .filter(|&index| LIBRARY[index].app.is_some_and(|app| app.owns(app_id)))
        .collect()
}

/// The library's rules for a window's app that the config lacks, given
/// which it has ([`in_config`]).
pub fn missing(have: &[bool], app_id: &str) -> Vec<usize> {
    for_app(app_id)
        .into_iter()
        .filter(|&index| !have.get(index).copied().unwrap_or(false))
        .collect()
}

/// How many of the library's rules for a window's app the config lacks.
pub fn missing_for(have: &[bool], app_id: &str) -> usize {
    missing(have, app_id).len()
}

/// The apps the library has rules for, in the page's order: every window
/// first, then apps by name. Only apps installed here, unless `all`.
pub fn apps(system: &SystemData, all: bool) -> Vec<Option<KnownApp>> {
    let mut apps: Vec<Option<KnownApp>> = vec![None];
    for rule in &LIBRARY {
        if let Some(app) = rule.app
            && !apps.contains(&Some(app))
            && (all || app.installed(system))
        {
            apps.push(Some(app));
        }
    }
    apps[1..].sort_by_cached_key(|app| app.map(|app| app.name().to_lowercase()));
    apps
}

/// What each library rule says, read as the config's rules are: two rules
/// that say the same are one however they are written.
pub struct Meanings(Vec<Option<RuleMeaning>>);

impl Default for Meanings {
    /// Every library rule, read.
    fn default() -> Self {
        Self(
            LIBRARY
                .iter()
                .map(|rule| {
                    let store = Store::from_text("library.kdl".into(), rule.kdl).ok()?;
                    store.window_rule_rows().first().map(WindowRuleRow::meaning)
                })
                .collect(),
        )
    }
}

impl Meanings {
    /// Which of the library's rules the config's `rules` have turned on,
    /// by index.
    pub fn in_config(&self, rules: &[WindowRuleRow]) -> Vec<bool> {
        let have: HashSet<RuleMeaning> = rules
            .iter()
            .filter(|rule| !rule.disabled)
            .map(WindowRuleRow::meaning)
            .collect();
        self.0
            .iter()
            .map(|meaning| {
                meaning
                    .as_ref()
                    .is_some_and(|meaning| have.contains(meaning))
            })
            .collect()
    }
}

/// Add the library's rules at `marked` to `store`, as one change: into the
/// config itself, or into [`FILE`] beside it, which the config includes.
pub fn import(store: &mut Store, marked: &[usize], into_config: bool) -> Result<(), StoreError> {
    for rule in marked.iter().filter_map(|&index| LIBRARY.get(index)) {
        if into_config {
            store.add_window_rule_text(rule.kdl)?;
        } else {
            store.add_to_include(FILE, rule.kdl)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::niri::OpenWindow;
    use crate::rules::{Reach, reach};

    #[test]
    fn every_library_rule_reaches_the_windows_it_is_for() {
        for rule in &LIBRARY {
            let rows = Store::from_text("config.kdl".into(), rule.kdl)
                .unwrap()
                .window_rule_rows();
            let [row] = rows.as_slice() else {
                panic!("{}: not one rule", rule.name);
            };
            for (app_id, title) in rule.examples {
                let window = OpenWindow {
                    id: 1,
                    app_id: (*app_id).to_string(),
                    title: (*title).to_string(),
                    floating: false,
                    focused: false,
                };
                assert_eq!(
                    reach(row, &window),
                    Reach::Reaches,
                    "{}: misses {app_id} {title:?}",
                    rule.name
                );
            }
            // every ID the app goes by has a window the rule is shown to reach
            if let Some(app) = rule.app {
                for id in app.ids() {
                    assert!(
                        rule.examples.iter().any(|(app_id, _)| app.owns(app_id)
                            && app_id.to_lowercase().starts_with(&id.to_lowercase())),
                        "{} ({}): no example for {id}",
                        rule.name,
                        app.name()
                    );
                }
            }
        }
    }

    #[test]
    fn a_window_is_matched_to_its_app_under_any_of_its_ids() {
        let floating = |app_id: &str| {
            for_app(app_id)
                .into_iter()
                .map(|index| LIBRARY[index].name)
                .collect::<Vec<_>>()
        };
        assert_eq!(floating("pavucontrol"), ["Opens floating"]);
        assert_eq!(floating("org.pulseaudio.pavucontrol"), ["Opens floating"]);
        assert_eq!(floating("ORG.MOZILLA.FIREFOX").len(), 2);
        assert!(floating("org.example.Unknown").is_empty());
    }
}
