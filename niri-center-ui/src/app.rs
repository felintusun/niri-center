//! The app's state: the session with the config and its history, and what
//! each page has open, typed or picked.

use std::collections::VecDeque;

use iced::Task;
use niri_center::animations::{self, Editor};
use niri_center::color::GradientEnd;
use niri_center::disk::actor::Command;
use niri_center::forms::{BindingForm, RuleDraft, follow};
use niri_center::niri::{self, OpenWindow, Running};
use niri_center::outputs::OutputInfo;
use niri_center::page::Page;
use niri_center::session::{Platform, Session};
use niri_center::settings::{DraftKey, Setting};
use niri_center::store::{BindingRow, Store, StoreError, WindowRuleRow};
use niri_center::system::{SystemData, XkbItem};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::message::Message;
use crate::{color, pages, tasks, theme};

/// The line at the bottom of the window.
#[derive(Debug, Clone)]
pub struct StatusLine {
    pub text: String,
    pub error: bool,
    /// When it showed: a toast for something that worked goes after a
    /// moment; an error stays until dismissed.
    pub since: iced::time::Instant,
}

/// How long a toast for something that worked stays.
pub const TOAST_SECONDS: u64 = 4;

pub fn info(text: impl Into<String>) -> Option<StatusLine> {
    Some(StatusLine {
        text: text.into(),
        error: false,
        since: iced::time::Instant::now(),
    })
}

pub fn error(text: impl Into<String>) -> Option<StatusLine> {
    Some(StatusLine {
        text: text.into(),
        error: true,
        since: iced::time::Instant::now(),
    })
}

/// The status after a store operation: `done` on success, the error otherwise.
pub fn outcome(result: Result<(), StoreError>, done: impl Into<String>) -> Option<StatusLine> {
    match result {
        Ok(()) => info(done),
        Err(e) => error(e.to_string()),
    }
}

/// The status after an edit that says what it did, or what is wrong.
pub fn said(result: Result<String, String>) -> Option<StatusLine> {
    match result {
        Ok(done) => info(done),
        Err(e) => error(e),
    }
}

/// What turning an entry on or off did, for the status line.
pub fn turned(what: &str, on: bool) -> String {
    if on {
        format!("{what} turned on.")
    } else {
        format!("{what} turned off. It stays in the file behind /-.")
    }
}

/// A color field: a setting's, or one end of its gradient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorField {
    pub key: DraftKey,
    pub end: Option<GradientEnd>,
}

/// The app was asked to close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closing {
    /// It closes once what is being saved is written.
    AfterSave,
    /// Edits would be lost: the dialog asks.
    Asking,
}

/// The color picker that is open under one field.
#[derive(Debug, Clone)]
pub struct ColorEdit {
    pub field: ColorField,
    /// The message a picked color goes into, with its hole to fill.
    pub target: Box<Message>,
    pub hsva: color::Hsva,
}

#[derive(Default)]
pub struct App {
    /// The config, what is saved of it, and its history.
    pub session: Session,
    pub load_error: Option<String>,
    pub page: Page,
    /// `--pane <slug>`: show only this page, without the sidebar or search.
    pub pane: Option<Page>,
    /// The search in the header: (query, highlighted match) while it has
    /// text; the results take the place of the page.
    pub header_search: Option<(String, usize)>,
    pub help_open: bool,
    pub status: Option<StatusLine>,
    /// The registry row a search result pointed at.
    pub highlight: Option<Setting>,
    pub color: Option<ColorEdit>,
    /// What is installed: xkb layouts and options, cursor themes, apps.
    pub system: SystemData,
    pub layout_box: iced::widget::combo_box::State<XkbItem>,
    pub option_box: iced::widget::combo_box::State<XkbItem>,
    pub keybinds: pages::keybinds::State,
    pub rows: Rows,
    /// What each library rule says, to tell which the config has.
    pub meanings: niri_center::library::Meanings,
    /// Every page and setting, for the search.
    pub search_index: niri_center::search::Index,
    pub environment: pages::environment::State,
    // autostart
    pub startup: pages::startup::State,
    /// The Animations page's editor.
    pub animation: Editor,
    pub rules: pages::rules::State,
    // displays, windows, events
    pub outputs: Vec<OutputInfo>,
    pub displays: pages::displays::State,
    pub running: Option<Running>,
    pub running_error: Option<String>,
    pub live_events: VecDeque<(niri_center::niri::EventKind, String)>,
    // files
    pub files: pages::files::State,
    /// The History page: the texts of the version shown, and what
    /// restoring the start of the history would change.
    pub history_view: crate::history::State,
    /// Where the disk actor's commands go; `None` in tests, which hand them
    /// to `test_actor`.
    pub disk: Option<UnboundedSender<Command>>,
    /// Where the niri actor's commands go; `None` in tests.
    pub niri: Option<UnboundedSender<niri::Command>>,
    /// Whether the niri actor sends the Events page its lines.
    pub watching: bool,
    #[cfg(test)]
    pub test_actor: Option<niri_center::disk::actor::Actor>,
    pub mode: Option<iced::theme::Mode>,
    pub high_contrast: bool,
    /// The timer that ends the wait before a save.
    pub save_timer: Option<iced::task::Handle>,
    // the history
    /// The config shows beside the page: only when asked for.
    pub source_shown: bool,
    /// The sidebar folded to icons (true) or open; `None` follows the
    /// window's width.
    pub sidebar_folded: Option<bool>,
    /// The config column is in a window of its own: that window's id once
    /// niri reports it, and the width to go back to when it closes.
    pub source_popped: Option<(Option<u64>, bool)>,
    /// The diff of what is not saved yet shows over the page.
    pub review_open: bool,
    // closing, repairing
    pub closing: Option<Closing>,
    /// The line niri cannot read shows in the repair panel.
    pub broken_line_shown: bool,
    /// Logical window size; 0 until the window reports it.
    pub window_width: f32,
    pub window_height: f32,
}

/// Below this window width the sidebar folds to icons, History shows one
/// version at a time and the header drops its words.
pub const COMPACT_WIDTH: f32 = 900.0;

/// The widest content a page gets ([`App::content_width`]): settings keep
/// to a comfortable line; pages with columns of their own get more.
const SETTINGS_CAP: f32 = 680.0;
const PAGE_CAP: f32 = 1200.0;

/// Widths at which a part of a page lays itself out differently, by the
/// width the part gets, not the window's. They stay below the caps above,
/// which the build checks: a breakpoint past its page's cap is a layout no
/// window ever gets.
pub mod room {
    /// A setting's control stands beside its label, not under it.
    pub const CONTROL_BESIDE_LABEL: f32 = 520.0;
    /// The Keybinds toolbar has room to count the bindings.
    pub const BINDING_COUNT: f32 = 560.0;
    /// The rule form puts how a text matches beside the text: the pick
    /// list and a field with room for an app ID.
    pub const MATCH_BESIDE_TEXT: f32 = 440.0;
    /// A page whose columns scroll on their own needs this much height;
    /// shorter, the page scrolls as a whole.
    pub const TALL: f32 = 600.0;
    /// The Rules page shows its index beside the app or rule open.
    pub const RULES_BESIDE_LIST: f32 = 600.0;
    /// The Displays page shows a display's settings beside the list.
    pub const DISPLAY_BESIDE_LIST: f32 = 960.0;
    /// The keyboard and the bindings sit side by side (with the height).
    pub const KEYBOARD_BESIDE_BINDINGS: f32 = 1000.0;

    const _: () = {
        use super::{PAGE_CAP, SETTINGS_CAP};
        assert!(CONTROL_BESIDE_LABEL < SETTINGS_CAP);
        assert!(BINDING_COUNT < PAGE_CAP);
        assert!(MATCH_BESIDE_TEXT < PAGE_CAP);
        assert!(RULES_BESIDE_LIST < PAGE_CAP);
        assert!(DISPLAY_BESIDE_LIST < PAGE_CAP);
        assert!(KEYBOARD_BESIDE_BINDINGS < PAGE_CAP);
    };
}

impl App {
    /// The app as it starts, on `pane` alone when given, and the tasks that
    /// read the config, its history and the system beside each other.
    pub fn boot(pane: Option<Page>) -> (Self, Task<Message>) {
        let page = pane.or_else(Page::from_address).unwrap_or_default();
        let app = Self {
            page,
            pane,
            #[cfg(target_arch = "wasm32")]
            live_events: niri_center::demo::events().into(),
            session: Session::new(Platform::NATIVE),
            ..Self::default()
        };
        let mut app = app;
        // the niri actor: its loop, and its replies as messages
        let (niri_commands, niri_received) = unbounded_channel();
        let (niri_replies, mut niri_answers) = unbounded_channel();
        let niri_run = Task::future(niri::Actor::new().run(niri_received, niri_replies)).discard();
        let niri_listen = Task::stream(iced::futures::stream::poll_fn(move |context| {
            niri_answers.poll_recv(context)
        }))
        .map(crate::message::from_niri);
        app.niri = Some(niri_commands);
        // the disk actor: its loop, and its replies as messages
        let (commands, received) = unbounded_channel();
        let (replies, mut answers) = unbounded_channel();
        let actor = niri_center::start::disk_actor(app.session.platform);
        let run = Task::future(actor.run(received, replies)).discard();
        let listen = Task::stream(iced::futures::stream::poll_fn(move |context| {
            answers.poll_recv(context)
        }))
        .map(|reply| Message::Disk(Box::new(reply)));
        app.disk = Some(commands);
        match niri_center::start::config_path() {
            Some(path) => app.session.start(path),
            None => {
                app.load_error = Some(
                    "No config location is known: XDG_CONFIG_HOME and HOME are not set."
                        .to_string(),
                );
            }
        }
        app.send_commands();
        let tasks = Task::batch([
            Task::done(Message::Page(page)),
            run,
            listen,
            niri_run,
            niri_listen,
            app.ask_niri(niri::Command::Outputs),
            tasks::blocking(
                || Box::new(niri_center::start::system()),
                Message::SystemLoaded,
            ),
            iced::system::theme().map(Message::ModeLoaded),
            Task::perform(
                niri_center::portal::high_contrast(),
                Message::ContrastLoaded,
            ),
        ]);
        // a browser has no monospace font of its own for config paths and diffs
        #[cfg(target_arch = "wasm32")]
        let tasks = Task::batch(
            crate::FONTS
                .map(|font| iced::font::load(font).discard())
                .into_iter()
                .chain([tasks]),
        );
        (app, tasks)
    }

    /// The config, while one is loaded.
    pub const fn store(&self) -> Option<&Store> {
        self.session.store.as_ref()
    }

    /// A window narrower than [`COMPACT_WIDTH`].
    pub fn narrow(&self) -> bool {
        self.width() < COMPACT_WIDTH
    }

    /// Logical window width (desktop-sized until the window reports it).
    pub fn width(&self) -> f32 {
        if self.window_width > 0.0 {
            self.window_width
        } else {
            1280.0
        }
    }

    /// Logical window height (desktop-sized until the window reports it).
    pub fn height(&self) -> f32 {
        if self.window_height > 0.0 {
            self.window_height
        } else {
            800.0
        }
    }

    /// Whether the Keybinds page puts the keyboard and the bindings side
    /// by side, the list scrolling on its own: when the window is wide and
    /// tall enough. A half or third of the screen stacks them and scrolls
    /// the page.
    pub fn keybinds_pinned(&self) -> bool {
        self.content_width() >= room::KEYBOARD_BESIDE_BINDINGS && self.height() >= room::TALL
    }

    /// Whether the Window rules page shows its index of apps beside the app
    /// or rule open, each column scrolling on its own: when the window is
    /// wide and tall enough. Narrower, only the level open shows, and the
    /// page scrolls.
    pub fn rules_beside_list(&self) -> bool {
        self.content_width() >= room::RULES_BESIDE_LIST && self.height() >= room::TALL
    }

    /// Whether something is open over the page: the search, a card or a
    /// dialog. Keys that act on the page wait until it closes.
    pub fn floating(&self) -> bool {
        self.header_search.is_some()
            || self.help_open
            || self.closing.is_some()
            || self.review_open
            || self.rules.dialog
            || self.rules.editor.is_some()
    }

    /// Whether the page scrolls its own columns, under a heading that stays.
    pub fn page_scrolls_itself(&self) -> bool {
        (self.page == Page::Keybinds && self.keybinds_pinned())
            || (self.page == Page::Rules && self.rules_beside_list())
    }

    /// Whether the config source shows beside the page: on pages that edit
    /// a part of it, when the window has room for two columns.
    pub fn shows_source(&self) -> bool {
        self.pane.is_none()
            && self.store().is_some()
            && self.width() >= COMPACT_WIDTH
            && self.page.has_source()
    }

    /// The config column is showing: only when asked for, on a page it
    /// belongs to.
    pub fn source_open(&self) -> bool {
        self.shows_source() && self.source_shown
    }

    /// Width of the config column: what the page does not need.
    pub fn source_width(&self) -> f32 {
        if !self.source_open() {
            return 0.0;
        }
        let page = if self.page.only_settings() {
            760.0
        } else {
            ((self.width() - self.sidebar_width()) * 0.55).max(760.0)
        };
        (self.width() - self.sidebar_width() - page).max(320.0)
    }

    /// The sidebar is a rail of icons: when folded, or by default in a
    /// window narrower than a half screen.
    pub fn sidebar_folded(&self) -> bool {
        self.sidebar_folded.unwrap_or_else(|| self.narrow())
    }

    pub fn sidebar_width(&self) -> f32 {
        if self.pane.is_some() {
            0.0
        } else if self.sidebar_folded() {
            56.0
        } else {
            216.0
        }
    }

    /// Width left for page content: window minus sidebar, config column
    /// and padding. Settings keep to a comfortable line; pages with two
    /// columns of their own (History) get more.
    pub fn content_width(&self) -> f32 {
        let limit = if self.page.only_settings() {
            SETTINGS_CAP
        } else {
            PAGE_CAP
        };
        (self.width() - self.sidebar_width() - self.source_width() - 48.0).min(limit)
    }

    /// The display the Displays page shows the settings of.
    pub fn selected_display(&self) -> Option<&OutputInfo> {
        self.displays
            .selected
            .as_ref()
            .and_then(|name| self.outputs.iter().find(|output| output.name == *name))
            .or_else(|| self.outputs.first())
    }

    /// The colors in use.
    pub const fn colors(&self) -> &'static theme::Colors {
        theme::pick(self.mode, self.high_contrast)
    }

    /// The config's bindings, as the Keybinds page lists them.
    pub fn binding_rows(&self) -> &[BindingRow] {
        &self.rows.bindings
    }

    /// The config's window rules, as the Rules page lists them.
    pub fn rule_rows(&self) -> &[WindowRuleRow] {
        &self.rows.rules
    }

    /// Work the rows out again when the config changed, by an edit here, an
    /// Undo, a restore or another program. What points at a rule or a
    /// binding by its place follows it there, or lets go when it is gone.
    pub fn refresh_rows(&mut self) {
        if self.store().map(Store::revision) != self.rows.revision {
            let new = Rows::of(self.store(), &self.meanings);
            let old = std::mem::replace(&mut self.rows, new);
            self.follow_rows(&old);
        }
        self.refresh_by_app();
    }

    /// Gather the rules and the open windows by app again when either
    /// changed since, or what is installed came in.
    fn refresh_by_app(&mut self) {
        let windows = self
            .running
            .as_ref()
            .map_or(&[][..], |data| data.windows.as_slice());
        let fresh = self
            .rows
            .by_app_from
            .as_ref()
            .is_some_and(|(revision, seen)| *revision == self.rows.revision && seen == windows);
        if !fresh {
            self.rows.by_app = niri_center::by_app::index(&self.rows.rules, windows, &self.system);
            self.rows.by_app_from = Some((self.rows.revision, windows.to_vec()));
        }
    }

    /// The rules and the open windows by app, as Window rules lists them.
    pub fn app_index(&self) -> &[niri_center::by_app::Entry] {
        &self.rows.by_app
    }

    fn follow_rows(&mut self, old: &Rows) {
        if self.rules.open.is_some()
            || self.rules.draft.editing.is_some()
            || self.rules.editor.is_some()
        {
            let new = &self.rows.rules;
            let follow = |index| {
                follow(&old.rules, new, index, |a, b| {
                    a.criteria == b.criteria && a.properties == b.properties
                })
            };
            self.rules.open = self.rules.open.and_then(follow);
            if let Some(editing) = self.rules.draft.editing {
                match follow(editing) {
                    Some(index) => self.rules.draft.editing = Some(index),
                    None => self.rules.draft = RuleDraft::default(),
                }
            }
            self.rules.editor = self
                .rules
                .editor
                .take()
                .and_then(|(editing, content)| Some((follow(editing)?, content)));
        }
        if let Some(editing) = self.keybinds.form.as_ref().and_then(|form| form.editing) {
            let moved = follow(&old.bindings, &self.rows.bindings, editing, |a, b| {
                a.key == b.key && a.action_raw == b.action_raw && a.file == b.file
            });
            self.keybinds.form = self.keybinds.form.take().and_then(|form| {
                Some(BindingForm {
                    editing: Some(moved?),
                    ..form
                })
            });
        }
    }

    /// Show `animation` in the editor, as the config sets it.
    pub fn load_animation(&mut self, animation: animations::Animation) {
        self.animation = Editor::load(self.store(), animation);
    }

    /// Ask for the Files page's backups and snapshots again.
    pub fn list_files(&mut self) {
        self.session.ask(Command::Listing);
    }

    /// Ask the niri actor. Its answer comes back as a message.
    pub fn ask_niri(&self, command: niri::Command) -> Task<Message> {
        if let Some(niri) = &self.niri {
            // the actor is gone only as the app closes
            let _ = niri.send(command);
        }
        Task::none()
    }

    /// Send the session's commands to the disk actor.
    pub fn send_commands(&mut self) {
        let Some(disk) = &self.disk else {
            return;
        };
        for command in self.session.take_commands() {
            // the actor is gone only as the app closes
            let _ = disk.send(command);
        }
    }

    /// The browser demo has no niri and no files: say what the desktop app
    /// does at this point, and stop. `false` everywhere else.
    pub fn desktop_only(&mut self, what: &str) -> bool {
        if self.session.platform == Platform::Browser {
            self.status = info(what);
            true
        } else {
            false
        }
    }
}

/// What the pages draw from the config, worked out once per revision of it.
#[derive(Default)]
pub struct Rows {
    /// The revision they were made from; `None` without a config.
    pub revision: Option<u64>,
    pub bindings: Vec<BindingRow>,
    pub rules: Vec<WindowRuleRow>,
    /// Which library rules the config has, by index.
    pub library_have: Vec<bool>,
    /// The rules and the open windows by app, for the Window rules page.
    pub by_app: Vec<niri_center::by_app::Entry>,
    /// The revision and the open windows `by_app` was made from; `None`
    /// to make it again, as when what is installed is known.
    pub by_app_from: Option<(Option<u64>, Vec<OpenWindow>)>,
}

impl Rows {
    fn of(store: Option<&Store>, meanings: &niri_center::library::Meanings) -> Self {
        let bindings = store.map(Store::binding_rows).unwrap_or_default();
        let rules = store.map(Store::window_rule_rows).unwrap_or_default();
        let library_have = meanings.in_config(&rules);
        Self {
            revision: store.map(Store::revision),
            bindings,
            rules,
            library_have,
            by_app: Vec::new(),
            by_app_from: None,
        }
    }
}
