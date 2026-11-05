//! The niri actor: the one part of the library that talks to the running
//! niri, over its IPC socket and its event stream.
//!
//! Like the disk actor it is plain state, run as a loop over commands that
//! ends when every sender is gone. It keeps one request connection and the
//! event stream. Picking a window or a colour waits for a click, so it runs
//! on a connection of its own and holds up no other query. The browser demo
//! has no niri: there the same commands answer with the demo's data, or say
//! that the desktop app does this.

use crate::outputs::OutputInfo;

/// An open window as the Windows page lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenWindow {
    /// niri's id for it, to focus it.
    pub id: u64,
    pub app_id: String,
    pub title: String,
    pub floating: bool,
    pub focused: bool,
}

/// What runs right now: displays, open windows and layer surfaces.
#[derive(Debug, Clone)]
pub struct Running {
    pub outputs: Vec<OutputInfo>,
    pub windows: Vec<OpenWindow>,
    pub layers: Vec<String>,
}

impl Running {
    /// The window `store`'s config is open in: one whose title names one
    /// of its files. An editor's title names the file, not its folder.
    pub fn editor_of(&self, store: &crate::store::Store) -> Option<&OpenWindow> {
        let names: Vec<&str> = store
            .saved_texts()
            .filter_map(|(path, _)| path.file_name()?.to_str())
            .collect();
        self.windows
            .iter()
            .find(|window| names.iter().any(|name| window.title.contains(name)))
    }
}

/// What a niri event is about, as the Events page sorts them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Workspaces,
    Windows,
    Focus,
    Input,
    Overview,
    Config,
    System,
    Casts,
}

impl EventKind {
    pub const fn word(self) -> &'static str {
        match self {
            Self::Workspaces => "workspaces",
            Self::Windows => "windows",
            Self::Focus => "focus",
            Self::Input => "input",
            Self::Overview => "overview",
            Self::Config => "config",
            Self::System => "system",
            Self::Casts => "casts",
        }
    }
}

/// What the niri actor is asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Outputs,
    Running,
    /// Bring a window to the front, on whichever workspace it is.
    Focus(u64),
    /// Have niri start a program, outside this app's sandbox.
    Spawn(Vec<String>),
    /// Open a web page in the user's browser.
    OpenLink(String),
    /// Let the user click a window.
    PickWindow,
    /// Let the user click anywhere, for the colour there.
    PickColor,
    /// Send every event as a line for the Events page (or stop).
    Watch(bool),
}

/// Why niri did not do what it was asked, as replies carry it. Replies are
/// `Clone` and an `io::Error` is not, so a transport failure is its text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NiriError {
    #[error("ipc transport error: {0}")]
    Transport(String),
    #[error("niri replied with an error: {0}")]
    Reply(String),
    #[error("not a query")]
    NotAQuery,
    #[error("could not open niri's event stream. Is niri-center running under niri?")]
    NoEventStream,
    /// What the demo says where the desktop app would reach niri.
    #[error("This talks to niri, so it works in the desktop app but not in this demo.")]
    DesktopOnly,
    #[error("The browser did not open the page.")]
    BrowserRefused,
}

#[cfg(not(target_arch = "wasm32"))]
impl From<crate::ipc::IpcError> for NiriError {
    fn from(error: crate::ipc::IpcError) -> Self {
        match error {
            crate::ipc::IpcError::Transport(source) => Self::Transport(source.to_string()),
            crate::ipc::IpcError::Reply(words) => Self::Reply(words),
        }
    }
}

/// What the niri actor answers, and what it follows in niri's events.
#[derive(Debug, Clone)]
pub enum Reply {
    /// Every connected output, in name order.
    Outputs(Result<Vec<OutputInfo>, NiriError>),
    Running(Result<Running, NiriError>),
    Focused(Result<(), NiriError>),
    Spawned(Result<(), NiriError>),
    /// Whether a web page could be opened.
    Opened(Result<(), NiriError>),
    /// The picked window's app ID and title; `None` when cancelled.
    PickedWindow(Result<Option<(String, String)>, NiriError>),
    PickedColor(Result<Option<[f64; 3]>, NiriError>),
    /// niri read its config again: whether that failed.
    ConfigLoaded {
        failed: bool,
    },
    /// The config column's own window (`--pane source`), by id.
    SourceWindow(u64),
    WindowClosed(u64),
    /// The configured keyboard layout niri types with, by its place in
    /// the config's list.
    KeyboardLayout(usize),
    /// One event, as a line for the Events page.
    Live {
        kind: EventKind,
        text: String,
    },
    /// The event stream could not be opened.
    LiveError(NiriError),
}

#[cfg(target_arch = "wasm32")]
pub use in_browser::Actor;
#[cfg(not(target_arch = "wasm32"))]
pub use native::Actor;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::task::Poll;

    use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

    use super::{Command, NiriError, OpenWindow, Reply, Running};
    use crate::ipc::{self, Client, Event, EventListener};
    use crate::outputs::{self, MonitorInfo, OutputInfo, RawMode};
    use crate::page::Page;

    /// What wakes the actor: a command, or an event from niri.
    enum Input {
        Command(Option<Command>),
        Event(Option<Event>),
    }

    /// One request connection, opened when first needed and again after
    /// it failed.
    #[derive(Default)]
    pub struct Actor {
        requests: Option<Client>,
    }

    impl Actor {
        pub const fn new() -> Self {
            Self { requests: None }
        }

        /// Answer commands and follow niri's events until every sender is
        /// gone, then hand the state back.
        pub async fn run(
            mut self,
            mut commands: UnboundedReceiver<Command>,
            replies: UnboundedSender<Reply>,
        ) -> Self {
            let mut events: Option<EventListener> = ipc::spawn_event_listener().ok();
            let mut watching = false;
            loop {
                let input = std::future::poll_fn(|context| {
                    if let Poll::Ready(command) = commands.poll_recv(context) {
                        return Poll::Ready(Input::Command(command));
                    }
                    match events.as_mut().map(|events| events.rx.poll_recv(context)) {
                        Some(Poll::Ready(event)) => Poll::Ready(Input::Event(event)),
                        Some(Poll::Pending) | None => Poll::Pending,
                    }
                })
                .await;
                match input {
                    Input::Command(None) => return self,
                    Input::Command(Some(command)) => {
                        if !self
                            .command(command, &mut watching, events.is_some(), &replies)
                            .await
                        {
                            return self;
                        }
                    }
                    // niri closed the stream
                    Input::Event(None) => events = None,
                    Input::Event(Some(event)) => {
                        let line = watching.then(|| ipc::describe_event(&event));
                        let live = line.map(|line| Reply::Live {
                            kind: line.kind,
                            text: line.text,
                        });
                        for reply in live.into_iter().chain(followed(event)) {
                            if replies.send(reply).is_err() {
                                return self;
                            }
                        }
                    }
                }
            }
        }

        /// Answer one command; `false` once nobody listens any more.
        async fn command(
            &mut self,
            command: Command,
            watching: &mut bool,
            streaming: bool,
            replies: &UnboundedSender<Reply>,
        ) -> bool {
            match command {
                Command::Watch(on) => {
                    *watching = on;
                    if on && !streaming {
                        return replies
                            .send(Reply::LiveError(NiriError::NoEventStream))
                            .is_ok();
                    }
                    true
                }
                Command::PickWindow | Command::PickColor => {
                    // waits for a click: on a connection of its own
                    let replies = replies.clone();
                    tokio::spawn(async move {
                        if let Ok(reply) = tokio::task::spawn_blocking(move || pick(&command)).await
                            && replies.send(reply).is_err()
                        {
                            // nobody to tell once the app has closed
                        }
                    });
                    true
                }
                Command::Outputs
                | Command::Running
                | Command::Focus(_)
                | Command::Spawn(_)
                | Command::OpenLink(_) => {
                    let requests = self.requests.take();
                    let Ok((requests, reply)) = tokio::task::spawn_blocking(move || {
                        let mut requests = requests;
                        let reply = request(&mut requests, command);
                        (requests, reply)
                    })
                    .await
                    else {
                        return true;
                    };
                    self.requests = requests;
                    replies.send(reply).is_ok()
                }
            }
        }
    }

    /// What the app follows in an event: config reloads, its own config
    /// window and the keyboard layout in use.
    fn followed(event: Event) -> Option<Reply> {
        let source = Page::Source.app_id();
        let is_source = |app_id: Option<&String>| app_id.is_some_and(|id| *id == source);
        #[allow(clippy::wildcard_enum_match_arm)] // the rest is not ours to follow
        match event {
            Event::ConfigLoaded { failed } => Some(Reply::ConfigLoaded { failed }),
            Event::WindowOpenedOrChanged { window } if is_source(window.app_id.as_ref()) => {
                Some(Reply::SourceWindow(window.id))
            }
            Event::WindowsChanged { windows } => windows
                .iter()
                .find(|window| is_source(window.app_id.as_ref()))
                .map(|window| Reply::SourceWindow(window.id)),
            Event::WindowClosed { id } => Some(Reply::WindowClosed(id)),
            Event::KeyboardLayoutsChanged { keyboard_layouts } => Some(Reply::KeyboardLayout(
                usize::from(keyboard_layouts.current_idx),
            )),
            Event::KeyboardLayoutSwitched { idx } => Some(Reply::KeyboardLayout(usize::from(idx))),
            _ => None,
        }
    }

    /// Answer a query on the held connection, opening it first when there
    /// is none. A failed query drops the connection: the next opens anew.
    fn request(requests: &mut Option<Client>, command: Command) -> Reply {
        let client = match requests.take() {
            Some(client) => Ok(client),
            None => Client::connect(),
        };
        let mut client = match client {
            Ok(client) => client,
            Err(error) => return failed(&command, error.into()),
        };
        let reply = match command {
            Command::Outputs => Reply::Outputs(outputs_from(&mut client)),
            Command::Running => Reply::Running(running_from(&mut client)),
            Command::Focus(id) => Reply::Focused(client.focus_window(id).map_err(Into::into)),
            Command::Spawn(argv) => Reply::Spawned(client.spawn(argv).map_err(Into::into)),
            // niri starts the opener, so the browser runs outside the sandbox
            Command::OpenLink(url) => Reply::Opened(
                client
                    .spawn(vec!["xdg-open".to_string(), url])
                    .map_err(Into::into),
            ),
            Command::PickWindow | Command::PickColor | Command::Watch(_) => {
                return failed(&command, NiriError::NotAQuery);
            }
        };
        let ok = match &reply {
            Reply::Outputs(result) => result.is_ok(),
            Reply::Running(result) => result.is_ok(),
            Reply::Focused(result) | Reply::Spawned(result) | Reply::Opened(result) => {
                result.is_ok()
            }
            Reply::PickedWindow(_)
            | Reply::PickedColor(_)
            | Reply::ConfigLoaded { .. }
            | Reply::SourceWindow(_)
            | Reply::WindowClosed(_)
            | Reply::KeyboardLayout(_)
            | Reply::Live { .. }
            | Reply::LiveError(_) => false,
        };
        if ok {
            *requests = Some(client);
        }
        reply
    }

    /// The reply to `command` when niri could not be asked.
    fn failed(command: &Command, why: NiriError) -> Reply {
        match command {
            Command::Outputs => Reply::Outputs(Err(why)),
            Command::Running => Reply::Running(Err(why)),
            Command::Focus(_) => Reply::Focused(Err(why)),
            Command::Spawn(_) => Reply::Spawned(Err(why)),
            Command::OpenLink(_) => Reply::Opened(Err(why)),
            Command::PickWindow => Reply::PickedWindow(Err(why)),
            Command::PickColor => Reply::PickedColor(Err(why)),
            Command::Watch(_) => Reply::LiveError(why),
        }
    }

    /// A pick, on a connection of its own: it waits for the click.
    fn pick(command: &Command) -> Reply {
        let mut client = match Client::connect() {
            Ok(client) => client,
            Err(error) => return failed(command, error.into()),
        };
        if *command == Command::PickColor {
            return Reply::PickedColor(client.pick_color().map_err(Into::into));
        }
        Reply::PickedWindow(
            client
                .pick_window()
                .map(|picked| {
                    picked.map(|window| {
                        (
                            window.app_id.unwrap_or_default(),
                            window.title.unwrap_or_default(),
                        )
                    })
                })
                .map_err(Into::into),
        )
    }

    fn outputs_from(client: &mut Client) -> Result<Vec<OutputInfo>, NiriError> {
        let mut found: Vec<OutputInfo> = client
            .outputs()?
            .into_values()
            .map(|output| OutputInfo {
                description: format!("{} {}", output.make, output.model)
                    .trim()
                    .to_string(),
                modes: outputs::modes(
                    &output
                        .modes
                        .iter()
                        .map(|mode| RawMode {
                            width: u32::from(mode.width),
                            height: u32::from(mode.height),
                            millihertz: mode.refresh_rate,
                            preferred: mode.is_preferred,
                        })
                        .collect::<Vec<_>>(),
                ),
                preferred: output.modes.iter().position(|mode| mode.is_preferred),
                current: output.current_mode,
                vrr_supported: output.vrr_supported,
                vrr_enabled: output.vrr_enabled,
                scale: output.logical.as_ref().map_or(1.0, |logical| logical.scale),
                rect: output.logical.map(|logical| MonitorInfo {
                    x: f64::from(logical.x),
                    y: f64::from(logical.y),
                    width: f64::from(logical.width),
                    height: f64::from(logical.height),
                }),
                name: output.name,
            })
            .collect();
        found.sort_by(|first, second| first.name.cmp(&second.name));
        Ok(found)
    }

    fn running_from(client: &mut Client) -> Result<Running, NiriError> {
        let outputs = outputs_from(client)?;
        let windows = client
            .windows()?
            .into_iter()
            .map(|window| OpenWindow {
                id: window.id,
                app_id: window.app_id.unwrap_or_default(),
                title: window.title.unwrap_or_default(),
                floating: window.is_floating,
                focused: window.is_focused,
            })
            .collect();
        let layers = client
            .layers()?
            .into_iter()
            .map(|surface| {
                format!(
                    "{} on {} ({:?})",
                    surface.namespace, surface.output, surface.layer
                )
            })
            .collect();
        Ok(Running {
            outputs,
            windows,
            layers,
        })
    }
}

#[cfg(target_arch = "wasm32")]
mod in_browser {
    use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

    use super::{Command, NiriError, Reply};

    /// The demo's niri: its data, and no event stream.
    #[derive(Default)]
    pub struct Actor;

    impl Actor {
        pub const fn new() -> Self {
            Self
        }

        pub async fn run(
            self,
            mut commands: UnboundedReceiver<Command>,
            replies: UnboundedSender<Reply>,
        ) -> Self {
            while let Some(command) = commands.recv().await {
                let reply = match command {
                    Command::Outputs => Reply::Outputs(Ok(crate::demo::outputs())),
                    Command::Running => Reply::Running(Ok(crate::demo::running())),
                    Command::Focus(_) => Reply::Focused(Err(NiriError::DesktopOnly)),
                    Command::Spawn(_) => Reply::Spawned(Err(NiriError::DesktopOnly)),
                    Command::OpenLink(url) => Reply::Opened(
                        web_sys::window()
                            .and_then(|window| window.open_with_url_and_target(&url, "_blank").ok())
                            .map(|_| ())
                            .ok_or(NiriError::BrowserRefused),
                    ),
                    Command::PickWindow => Reply::PickedWindow(Err(NiriError::DesktopOnly)),
                    Command::PickColor => Reply::PickedColor(Err(NiriError::DesktopOnly)),
                    // the demo's Events page shows its samples
                    Command::Watch(_) => continue,
                };
                if replies.send(reply).is_err() {
                    break;
                }
            }
            self
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::{Actor, Command, NiriError, Reply};
    use crate::ipc::IpcError;

    #[test]
    fn a_failed_request_reads_as_the_ipc_error_it_came_from() {
        assert_eq!(
            NiriError::from(IpcError::Transport(std::io::Error::other("socket gone"))).to_string(),
            "ipc transport error: socket gone"
        );
        assert_eq!(
            NiriError::from(IpcError::Reply("no such window".into())).to_string(),
            "niri replied with an error: no such window"
        );
    }

    #[test]
    fn what_no_niri_could_answer_is_said_in_words() {
        assert_eq!(NiriError::NotAQuery.to_string(), "not a query");
        assert_eq!(
            NiriError::NoEventStream.to_string(),
            "could not open niri's event stream. Is niri-center running under niri?"
        );
        assert_eq!(
            NiriError::DesktopOnly.to_string(),
            "This talks to niri, so it works in the desktop app but not in this demo."
        );
        assert_eq!(
            NiriError::BrowserRefused.to_string(),
            "The browser did not open the page."
        );
    }

    #[test]
    fn without_niri_each_command_is_answered_with_the_failure_to_reach_it() {
        if std::env::var_os("NIRI_SOCKET").is_some() {
            return;
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let (commands, received) = tokio::sync::mpsc::unbounded_channel();
        let (replies, mut answers) = tokio::sync::mpsc::unbounded_channel();
        for command in [
            Command::Outputs,
            Command::Running,
            Command::Focus(1),
            Command::Spawn(vec!["true".to_string()]),
            Command::OpenLink("https://example.org".to_string()),
            Command::Watch(true),
        ] {
            commands.send(command).unwrap();
        }
        drop(commands);
        runtime.block_on(Actor::new().run(received, replies));
        let mut all = Vec::new();
        while let Ok(reply) = answers.try_recv() {
            all.push(reply);
        }
        assert!(
            matches!(
                all.as_slice(),
                [
                    Reply::Outputs(Err(NiriError::Transport(_))),
                    Reply::Running(Err(NiriError::Transport(_))),
                    Reply::Focused(Err(NiriError::Transport(_))),
                    Reply::Spawned(Err(NiriError::Transport(_))),
                    Reply::Opened(Err(NiriError::Transport(_))),
                    Reply::LiveError(NiriError::NoEventStream),
                ]
            ),
            "{all:?}"
        );
    }

    #[test]
    fn the_actor_answers_from_the_running_niri_and_ends_with_its_senders() {
        if std::env::var_os("NIRI_SOCKET").is_none() {
            return;
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let (commands, received) = tokio::sync::mpsc::unbounded_channel();
        let (replies, mut answers) = tokio::sync::mpsc::unbounded_channel();
        commands.send(Command::Outputs).unwrap();
        drop(commands);
        runtime.block_on(Actor::new().run(received, replies));
        let mut outputs = None;
        while let Ok(reply) = answers.try_recv() {
            if let Reply::Outputs(found) = reply {
                outputs = Some(found.unwrap());
            }
        }
        assert!(!outputs.expect("an answer").is_empty());
    }
}
