//! Everything the app reacts to: what people do, what tasks and niri
//! report back, and the clock.

use std::path::PathBuf;

use iced::Color;
use niri_center::disk::actor::Reply;
use niri_center::niri::{self, EventKind, Running};
use niri_center::outputs::OutputInfo;
use niri_center::page::Page;
use niri_center::search;
use niri_center::settings::{Edit, OutputSetting, Setting};
use niri_center::system::SystemData;

use crate::app::ColorField;
use crate::monitors::DroppedMonitor;
use crate::{color, curves, keyboard, shortcuts};

#[derive(Debug, Clone)]
// the browser demo has no event stream to deliver niri's events
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub enum Message {
    /// What the disk actor answers (boxed: a loaded config is large).
    Disk(Box<Reply>),
    Page(Page),
    Keybinds(crate::pages::keybinds::Message),
    Startup(crate::pages::startup::Message),
    Environment(crate::pages::environment::Message),
    // registry settings and per-output settings
    Setting(Setting, Edit),
    Output(String, OutputSetting, Edit),
    /// Open (or close) the color picker under the field `key`, which shows
    /// `Color`; picked colors go into the target message.
    OpenColor(ColorField, Option<Color>, Box<Self>),
    ColorPicked(color::Hsva),
    PickScreenColor,
    ScreenColor(Result<Option<[f64; 3]>, String>),
    CloseColor,
    /// Ask the desktop's file chooser for a path for the target message.
    Browse {
        target: Box<Self>,
        folder: bool,
    },
    Browsed(Box<Self>, Result<Option<String>, String>),
    SystemLoaded(Box<SystemData>),
    Animations(crate::pages::animations::Message),
    Rules(crate::pages::rules::Message),
    // displays, windows, events
    Displays(crate::pages::displays::Message),
    /// A second of a display change's trial has passed.
    TrialTick,
    KeepTrial,
    /// Undo the display change on trial.
    EndTrial,
    OutputsLoaded(Result<Vec<OutputInfo>, String>),
    RunningTick,
    RunningLoaded(Result<Running, String>),
    /// One line from niri's event stream: its kind and text.
    LiveEvent(EventKind, String),
    LiveError(String),
    ClearLive,
    Files(crate::pages::files::Message),
    // chrome
    ModeLoaded(iced::theme::Mode),
    /// Whether the desktop asks for more contrast.
    ContrastLoaded(bool),
    WindowResized(iced::Size),
    Shortcut(shortcuts::Shortcut),
    OpenSearch,
    SearchQuery(String),
    SearchSubmit,
    SearchPick(search::Entry),
    /// A match the search found on the page shown: what picking it does.
    SearchPickHere(crate::pages::rules::Message),
    ShowHelp(bool),
    PopOut(Page),
    PoppedOut(Result<(), String>),
    // saving and the history
    /// The wait after the last edit may be over: save when it is.
    SaveTick,
    SaveNow,
    /// Drop edits not saved yet, else undo the latest change.
    Undo,
    /// Show or hide the config beside the page.
    ShowSource(bool),
    /// Fold the sidebar to icons, or open it.
    ToggleSidebar,
    /// Close the popped-out config window.
    PutBack,
    /// Copy what is not saved, or a change in the history, as a diff.
    CopyPending,
    History(crate::history::Message),
    /// Show (or close) the diff of what is not saved.
    ShowReview(bool),
    /// A second has passed while a toast shows.
    ToastTick,
    DismissToast,
    /// niri reports the popped-out config window, by id.
    SourceWindow(u64),
    /// niri closed a window: the config column comes back when it was its.
    WindowClosed(u64),
    // niri's config reloads
    NiriConfigLoaded(bool),
    /// Rename a misspelled node niri rejected.
    FixTypo {
        path: PathBuf,
        line: usize,
        name: String,
        meant: String,
    },
    FocusWindow(u64),
    WindowFocused(Result<(), String>),
    ShowBrokenLine(bool),
    DismissHealth,
    // closing
    CloseRequested,
    /// Save what waits, then close once it is written.
    SaveAndClose,
    CloseCancel,
    CloseDiscard,
}

impl From<DroppedMonitor> for Message {
    fn from(dropped: DroppedMonitor) -> Self {
        Self::Displays(crate::pages::displays::Message::Dropped(dropped))
    }
}

impl From<color::Hsva> for Message {
    fn from(hsva: color::Hsva) -> Self {
        Self::ColorPicked(hsva)
    }
}

impl From<curves::Bezier> for Message {
    fn from(bezier: curves::Bezier) -> Self {
        Self::Animations(crate::pages::animations::Message::Bezier(bezier))
    }
}

impl From<keyboard::KeyClicked> for Message {
    fn from(clicked: keyboard::KeyClicked) -> Self {
        Self::Keybinds(crate::pages::keybinds::Message::KeyClicked(clicked.token))
    }
}

/// What the niri actor answers, as the message that handles it.
pub fn from_niri(reply: niri::Reply) -> Message {
    match reply {
        niri::Reply::Outputs(result) => Message::OutputsLoaded(result),
        niri::Reply::Running(result) => Message::RunningLoaded(result),
        niri::Reply::Focused(result) => Message::WindowFocused(result),
        niri::Reply::Spawned(result) => Message::PoppedOut(result),
        niri::Reply::PickedWindow(result) => {
            Message::Rules(crate::pages::rules::Message::WindowPicked(result))
        }
        niri::Reply::PickedColor(result) => Message::ScreenColor(result),
        niri::Reply::ConfigLoaded { failed } => Message::NiriConfigLoaded(failed),
        niri::Reply::SourceWindow(id) => Message::SourceWindow(id),
        niri::Reply::WindowClosed(id) => Message::WindowClosed(id),
        niri::Reply::Live { kind, text } => Message::LiveEvent(kind, text),
        niri::Reply::LiveError(e) => Message::LiveError(e),
    }
}
