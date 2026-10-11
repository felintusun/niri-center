//! The app's pages: what each is called, the sidebar's groups,
//! and the part of the config each one edits.

use std::path::Path;

use crate::settings::REGISTRY;
use crate::store::Store;

/// One page of the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Keybinds,
    Rules,
    Layout,
    Displays,
    Workspaces,
    Overview,
    Animations,
    Input,
    Gestures,
    Startup,
    Environment,
    Cursor,
    Screenshots,
    History,
    Files,
    Windows,
    Events,
    /// The config file on its own: the source column popped out.
    Source,
}

impl std::fmt::Display for Page {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

impl Page {
    pub const ALL: [Self; 18] = [
        Self::Keybinds,
        Self::Rules,
        Self::Layout,
        Self::Displays,
        Self::Workspaces,
        Self::Overview,
        Self::Animations,
        Self::Input,
        Self::Gestures,
        Self::Startup,
        Self::Environment,
        Self::Cursor,
        Self::Screenshots,
        Self::History,
        Self::Files,
        Self::Windows,
        Self::Events,
        Self::Source,
    ];

    /// The sidebar: groups under plain headings, in order. Windows,
    /// Events and the config page are reached through search and `--pane`.
    pub const SIDEBAR: [(&'static str, &'static [Self]); 5] = [
        (
            "Keyboard and mouse",
            &[Self::Keybinds, Self::Input, Self::Cursor, Self::Gestures],
        ),
        ("Screens", &[Self::Displays, Self::Screenshots]),
        (
            "Windows",
            &[
                Self::Layout,
                Self::Rules,
                Self::Workspaces,
                Self::Overview,
                Self::Animations,
            ],
        ),
        ("Session", &[Self::Startup, Self::Environment]),
        ("Your config", &[Self::History, Self::Files]),
    ];

    /// Every page in the sidebar, in its order.
    pub fn in_sidebar() -> impl Iterator<Item = Self> {
        Self::SIDEBAR
            .into_iter()
            .flat_map(|(_, pages)| pages.iter().copied())
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Keybinds => "Keybinds",
            Self::Rules => "Window rules",
            Self::Layout => "Layout",
            Self::Displays => "Displays",
            Self::Workspaces => "Workspaces",
            Self::Overview => "Overview",
            Self::Animations => "Animations",
            Self::Input => "Input devices",
            Self::Gestures => "Gestures",
            Self::Startup => "Startup",
            Self::Environment => "Environment",
            Self::Cursor => "Cursor",
            Self::Screenshots => "Screenshots",
            Self::History => "History",
            Self::Files => "Files",
            Self::Windows => "Windows",
            Self::Events => "Events",
            Self::Source => "Config file",
        }
    }

    /// Stable name for `--pane <slug>` and the window's app-id.
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Keybinds => "keybinds",
            Self::Rules => "rules",
            Self::Layout => "layout",
            Self::Displays => "displays",
            Self::Workspaces => "workspaces",
            Self::Overview => "overview",
            Self::Animations => "animations",
            Self::Input => "input",
            Self::Gestures => "gestures",
            Self::Startup => "startup",
            Self::Environment => "environment",
            Self::Cursor => "cursor",
            Self::Screenshots => "screenshots",
            Self::History => "history",
            Self::Files => "files",
            Self::Windows => "windows",
            Self::Events => "events",
            Self::Source => "source",
        }
    }

    /// `--pane` names, including the ones older launchers use.
    pub fn from_slug(slug: &str) -> Option<Self> {
        let renamed = match slug {
            "monitors" => "displays",
            "appearance" => "layout",
            "curves" => "animations",
            "behavior" | "general" => "overview",
            "window-rules" => "rules",
            "autostart" => "startup",
            "toolbox" => "windows",
            "live" => "events",
            "config" => "files",
            other => other,
        };
        Self::ALL.into_iter().find(|p| p.slug() == renamed)
    }

    /// Live views that are useful next to other windows.
    pub const fn pops_out(self) -> bool {
        matches!(self, Self::Displays | Self::Windows | Self::Events)
    }

    /// Pages that are rows from the settings table and nothing else.
    pub const fn only_settings(self) -> bool {
        matches!(
            self,
            Self::Layout
                | Self::Workspaces
                | Self::Overview
                | Self::Input
                | Self::Gestures
                | Self::Environment
                | Self::Cursor
                | Self::Screenshots
        )
    }
}

impl Page {
    /// The page a link asks for: `?page=<slug>` in the browser demo's
    /// address. The desktop app is given its page with `--pane` instead.
    pub fn from_address() -> Option<Self> {
        #[cfg(target_arch = "wasm32")]
        {
            let search = web_sys::window()?.location().search().ok()?;
            let slug = search
                .trim_start_matches('?')
                .split('&')
                .find_map(|pair| pair.strip_prefix("page="))?;
            Self::from_slug(slug)
        }
        #[cfg(not(target_arch = "wasm32"))]
        None
    }

    /// The app-id of the window showing the page alone (`--pane`), so niri
    /// window rules can tell the windows apart: `niri-center.displays`.
    pub fn app_id(self) -> String {
        format!("niri-center.{}", self.slug())
    }

    /// Pages whose part of the config can show beside them.
    pub const fn has_source(self) -> bool {
        self.only_settings() || matches!(self, Self::Rules | Self::Startup | Self::Animations)
    }

    /// The top-level nodes the page edits besides its settings.
    const fn blocks(self) -> &'static [&'static str] {
        match self {
            Self::Keybinds => &["binds"],
            Self::Rules => &["window-rule"],
            Self::Startup => &["spawn-at-startup", "spawn-sh-at-startup"],
            Self::Environment => &["environment"],
            Self::Animations => &["animations"],
            Self::Layout
            | Self::Displays
            | Self::Workspaces
            | Self::Overview
            | Self::Input
            | Self::Gestures
            | Self::Cursor
            | Self::Screenshots
            | Self::History
            | Self::Files
            | Self::Windows
            | Self::Events
            | Self::Source => &[],
        }
    }

    /// The top-level nodes holding a setting of the page that the config
    /// sets: a block that holds none of them is not this page's business.
    fn settings_blocks(self, store: &Store) -> Vec<&'static str> {
        let mut names = Vec::new();
        for spec in REGISTRY.iter().filter(|spec| spec.page == self) {
            let name = spec.chain.first().map_or(spec.child, |block| block.name);
            if !names.contains(&name) && store.setting_line(spec.chain, spec.child).is_some() {
                names.push(name);
            }
        }
        names
    }

    /// The stretch of the config the page edits: the first file holding
    /// any of it, and its first and last lines there, 1-based.
    pub fn excerpt(self, store: &Store) -> Option<Excerpt<'_>> {
        let mut names = self.blocks().to_vec();
        names.extend(self.settings_blocks(store));
        let found = store.block_lines(&names);
        let (path, ..) = *found.first()?;
        let in_file = found.iter().filter(|(file, ..)| *file == path);
        let first = in_file.clone().map(|(_, first, _)| *first).min()?;
        let last = in_file.map(|(_, _, last)| *last).max()?;
        Some(Excerpt { path, first, last })
    }
}

/// A stretch of one config file, by 1-based lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Excerpt<'a> {
    pub path: &'a Path,
    pub first: usize,
    pub last: usize,
}
