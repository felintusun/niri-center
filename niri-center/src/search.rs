//! The header search, over pages and settings, and the list of the app's
//! own keys the help card shows.

use crate::page::Page;
use crate::settings::Setting;

/// One search result: a page, or a setting on a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub label: String,
    /// Shown beside the label: the page, or the setting's config path.
    pub detail: String,
    pub page: Page,
    /// The registry row to highlight on arrival.
    pub setting: Option<Setting>,
}

/// Search terms for pages beyond their names.
const PAGE_KEYWORDS: &[(Page, &str)] = &[
    (
        Page::Keybinds,
        "binds shortcuts hotkeys keys keyboard visualizer actions",
    ),
    (
        Page::Rules,
        "window rules match app-id float floating maximize opacity pick",
    ),
    (
        Page::Layout,
        "gaps focus ring border shadow colors gradient centering columns widths heights presets struts tabs insert hint blur",
    ),
    (
        Page::Displays,
        "monitors outputs screens arrange position mode scale refresh vrr rotate backdrop",
    ),
    (Page::Workspaces, "workspace empty back and forth"),
    (
        Page::Overview,
        "overview zoom backdrop alt tab switcher recent windows hotkey overlay",
    ),
    (
        Page::Animations,
        "curves easing bezier spring damping stiffness slowdown duration",
    ),
    (
        Page::Input,
        "keyboard touchpad mouse xkb layout variant options mod key tap scroll tablet touchscreen power",
    ),
    (Page::Gestures, "hot corners gestures"),
    (Page::Startup, "spawn-at-startup autostart launch programs"),
    (
        Page::Environment,
        "environment variables csd decorations title bars clipboard xwayland x11",
    ),
    (
        Page::Cursor,
        "cursor theme size hide pointer focus follows mouse warp",
    ),
    (Page::Screenshots, "screenshot path folder"),
    (Page::History, "history undo changes edits"),
    (
        Page::Files,
        "config includes backups snapshots restore sandbox",
    ),
    (Page::Windows, "open windows layers ipc toolbox"),
    (Page::Events, "live events stream focus workspaces"),
];

/// Every page and setting the search can find, each with its text in
/// lower case, so a keystroke only compares.
pub struct Index(Vec<(Entry, String)>);

impl Default for Index {
    fn default() -> Self {
        Self(entries())
    }
}

impl Index {
    /// Pages and settings whose text contains every query word.
    pub fn matches(&self, query: &str) -> impl Iterator<Item = &Entry> {
        let words = Words::of(query);
        self.0
            .iter()
            .filter(move |(_, haystack)| words.all_in(haystack))
            .map(|(entry, _)| entry)
    }
}

/// A query's words, lower case: what a search looks for.
pub struct Words(Vec<String>);

impl Words {
    pub fn of(query: &str) -> Self {
        Self(query.split_whitespace().map(str::to_lowercase).collect())
    }

    /// Whether the lower-case `text` has every word; never for no words.
    pub fn all_in(&self, text: &str) -> bool {
        !self.0.is_empty() && self.0.iter().all(|word| text.contains(word.as_str()))
    }
}

fn entries() -> Vec<(Entry, String)> {
    let pages = Page::ALL.iter().map(|&page| {
        let keywords = PAGE_KEYWORDS
            .iter()
            .find(|(p, _)| *p == page)
            .map_or("", |(_, k)| *k);
        (
            Entry {
                label: page.label().to_string(),
                detail: "Page".to_string(),
                page,
                setting: None,
            },
            keywords.to_string(),
        )
    });
    let settings = Setting::ALL.iter().map(|&setting| {
        let spec = setting.spec();
        (
            Entry {
                label: format!("{}: {}", spec.section, spec.label),
                detail: spec.path(),
                page: spec.page,
                setting: Some(setting),
            },
            format!("{} {}", spec.help, spec.page.label()),
        )
    });
    pages
        .chain(settings)
        .map(|(entry, extra)| {
            let haystack = format!("{} {} {extra}", entry.label, entry.detail).to_lowercase();
            (entry, haystack)
        })
        .collect()
}

/// A group of lines on the help card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpGroup {
    MovingAround,
    Changes,
    Windows,
}

impl HelpGroup {
    pub const ALL: [Self; 3] = [Self::MovingAround, Self::Changes, Self::Windows];

    pub const fn heading(self) -> &'static str {
        match self {
            Self::MovingAround => "Moving around",
            Self::Changes => "Changes",
            Self::Windows => "Windows",
        }
    }
}

/// One line on the help card.
pub struct HelpLine {
    pub group: HelpGroup,
    /// The keys, as people read them.
    pub keys: &'static str,
    pub what: &'static str,
    /// The same keys as niri writes them in a bind: niri takes a bound key
    /// before the app sees it.
    pub binds: &'static [&'static str],
}

/// The app's own keys, as the help card lists them.
pub const HELP: [HelpLine; 15] = [
    HelpLine {
        group: HelpGroup::MovingAround,
        keys: "Ctrl+PgUp, Ctrl+PgDn",
        what: "Previous or next section",
        binds: &["Ctrl+Page_Up", "Ctrl+Page_Down"],
    },
    HelpLine {
        group: HelpGroup::MovingAround,
        keys: "Ctrl+1 to Ctrl+0",
        what: "Go to the first ten sections",
        binds: &[
            "Ctrl+1", "Ctrl+2", "Ctrl+3", "Ctrl+4", "Ctrl+5", "Ctrl+6", "Ctrl+7", "Ctrl+8",
            "Ctrl+9", "Ctrl+0",
        ],
    },
    HelpLine {
        group: HelpGroup::MovingAround,
        keys: "Ctrl+F, Ctrl+K, or type",
        what: "Find a setting, key or action",
        binds: &["Ctrl+F", "Ctrl+K"],
    },
    HelpLine {
        group: HelpGroup::MovingAround,
        keys: "Up, Down",
        what: "Move in the search results",
        binds: &[],
    },
    HelpLine {
        group: HelpGroup::MovingAround,
        keys: "Esc",
        what: "Close search or this list, or go back",
        binds: &[],
    },
    HelpLine {
        group: HelpGroup::MovingAround,
        keys: "Alt+Left",
        what: "Back up one level in Window rules",
        binds: &["Alt+Left"],
    },
    HelpLine {
        group: HelpGroup::Changes,
        keys: "Ctrl+Z",
        what: "Undo the latest change",
        binds: &["Ctrl+Z"],
    },
    HelpLine {
        group: HelpGroup::Changes,
        keys: "Ctrl+S",
        what: "Save now instead of a moment later",
        binds: &["Ctrl+S"],
    },
    HelpLine {
        group: HelpGroup::Changes,
        keys: "Enter",
        what: "While trying a display change: keep it",
        binds: &[],
    },
    HelpLine {
        group: HelpGroup::Changes,
        keys: "Esc",
        what: "While trying a display change: go back",
        binds: &[],
    },
    HelpLine {
        group: HelpGroup::Windows,
        keys: "Ctrl+B",
        what: "Fold the sidebar to icons, or open it",
        binds: &["Ctrl+B"],
    },
    HelpLine {
        group: HelpGroup::Windows,
        keys: "Ctrl+.",
        what: "Show or hide the config beside the page",
        binds: &["Ctrl+Period"],
    },
    HelpLine {
        group: HelpGroup::Windows,
        keys: "Ctrl+Shift+N",
        what: "Open the config as a window of its own",
        binds: &["Ctrl+Shift+N"],
    },
    HelpLine {
        group: HelpGroup::Windows,
        keys: "Ctrl+W",
        what: "In that window: put it back",
        binds: &["Ctrl+W"],
    },
    HelpLine {
        group: HelpGroup::Windows,
        keys: "F1, Ctrl+?",
        what: "This list",
        binds: &["F1", "Ctrl+Question"],
    },
];
