//! Wrapper around the official [`niri-ipc`] crate (pin `=26.4.0` for niri 26.04).
//!
//! Presents the pieces the GUI actually needs: version, outputs, windows,
//! layers, and the event stream (for live updates). All heavy types come from
//! `niri_ipc` and are re-exported so the UI layer never talks to the wrappers.

use std::collections::HashMap;
use std::io;

use niri_ipc::socket::Socket;
use niri_ipc::{Action, Request, Response};

use crate::niri::EventKind;

pub use niri_ipc::{Event, LayerSurface, LogicalOutput, Mode, Output, Window, Workspace};

/// Blocking IPC client. One per app; the GUI drives it from its async side.
pub struct Client {
    socket: Socket,
}

/// Connection/query failures.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("ipc transport error: {0}")]
    Transport(#[from] io::Error),
    #[error("niri replied with an error: {0}")]
    Reply(String),
}

/// A live niri event as a human-readable line for the Events page.
#[derive(Debug, Clone)]
pub struct LiveEventLine {
    pub kind: EventKind,
    pub text: String,
}

/// A dedicated event-stream connection with a reader thread pushing into a
/// tokio channel that wakes receivers on each event — zero polling required.
pub struct EventListener {
    pub rx: tokio::sync::mpsc::UnboundedReceiver<Event>,
}

/// Open the event stream on a fresh socket and spawn its reader thread.
///
/// The thread blocks on the socket (no CPU when idle) and wakes the tokio
/// channel on every event; the awaiting stream consumer wakes accordingly.
pub fn spawn_event_listener() -> Result<EventListener, IpcError> {
    let client = Client::connect()?;
    let mut read_event = client.into_event_stream()?;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        while let Ok(event) = read_event() {
            if tx.send(event).is_err() {
                break; // consumer gone
            }
        }
    });
    Ok(EventListener { rx })
}

/// Render one niri event as a short, human-readable line.
pub fn describe_event(event: &Event) -> LiveEventLine {
    let (kind, text) = match event {
        Event::WorkspacesChanged { workspaces } => (
            EventKind::Workspaces,
            format!("workspace set changed ({} total)", workspaces.len()),
        ),
        Event::WorkspaceUrgencyChanged { id, urgent } => (
            EventKind::Workspaces,
            format!("workspace #{id} urgent={urgent}"),
        ),
        Event::WorkspaceActivated { id, focused } => (
            EventKind::Workspaces,
            format!("workspace #{id} activated (focused={focused})"),
        ),
        Event::WorkspaceActiveWindowChanged {
            workspace_id,
            active_window_id,
        } => (
            EventKind::Workspaces,
            format!("active window on ws #{workspace_id} → {active_window_id:?}"),
        ),
        Event::WindowsChanged { windows } => (
            EventKind::Windows,
            format!("window set changed ({} total)", windows.len()),
        ),
        Event::WindowOpenedOrChanged { window } => (
            EventKind::Windows,
            format!(
                "window opened/changed: {} — {}",
                window.app_id.as_deref().unwrap_or("?"),
                window.title.as_deref().unwrap_or("")
            ),
        ),
        Event::WindowClosed { id } => (EventKind::Windows, format!("window closed #{id}")),
        Event::WindowFocusChanged { id } => (EventKind::Focus, format!("focused window → {id:?}")),
        Event::WindowFocusTimestampChanged { id, .. } => (
            EventKind::Focus,
            format!("focus timestamp updated for #{id}"),
        ),
        Event::WindowUrgencyChanged { id, urgent } => {
            (EventKind::Windows, format!("window #{id} urgency={urgent}"))
        }
        Event::WindowLayoutsChanged { changes } => (
            EventKind::Windows,
            format!("layouts changed ({} windows)", changes.len()),
        ),
        Event::KeyboardLayoutsChanged { .. } => {
            (EventKind::Input, "keyboard layouts changed".into())
        }
        Event::KeyboardLayoutSwitched { idx: index } => (
            EventKind::Input,
            format!("keyboard layout switched → #{index}"),
        ),
        Event::OverviewOpenedOrClosed { is_open } => (
            EventKind::Overview,
            format!("overview {}", if *is_open { "opened" } else { "closed" }),
        ),
        Event::ConfigLoaded { failed } => (
            EventKind::Config,
            format!(
                "config reloaded ({})",
                if *failed { "FAILED" } else { "ok" }
            ),
        ),
        Event::ScreenshotCaptured { path } => (
            EventKind::System,
            format!(
                "screenshot captured: {}",
                path.as_deref().unwrap_or("<clipboard>")
            ),
        ),
        Event::CastsChanged { casts } => (
            EventKind::Casts,
            format!("screencasts changed ({} total)", casts.len()),
        ),
        Event::CastStartedOrChanged { .. } => {
            (EventKind::Casts, "screencast started/changed".into())
        }
        Event::CastStopped { stream_id } => {
            (EventKind::Casts, format!("screencast stopped #{stream_id}"))
        }
    };
    LiveEventLine { kind, text }
}

/// An I/O-transport-level result from the socket, guaranteed to be a `Response`.
///
/// Every request method accepts exactly one `Response` variant and treats any
/// other variant as a protocol violation. This exhaustive enumeration means a
/// new `Response` variant in the pinned `niri-ipc` breaks this match — the
/// compiler reminds us to decide. `niri_ipc` is pinned with `=26.4.0`, so
/// variant drift only arrives deliberately.
fn unexpected_response(request: &str, response: &Response) -> IpcError {
    IpcError::Reply(match response {
        Response::Handled
        | Response::Version(_)
        | Response::Outputs(_)
        | Response::Workspaces(_)
        | Response::Windows(_)
        | Response::Layers(_)
        | Response::KeyboardLayouts(_)
        | Response::FocusedOutput(_)
        | Response::FocusedWindow(_)
        | Response::PickedWindow(_)
        | Response::PickedColor(_)
        | Response::OutputConfigChanged(_)
        | Response::OverviewState(_)
        | Response::Casts(_) => {
            format!("unexpected response type for {request}: {response:?}")
        }
    })
}

impl Client {
    /// Connect to the niri IPC socket (`$NIRI_SOCKET`, set when running under niri).
    pub fn connect() -> Result<Self, IpcError> {
        Ok(Self {
            socket: Socket::connect()?,
        })
    }

    pub fn connect_to(path: impl AsRef<std::path::Path>) -> Result<Self, IpcError> {
        Ok(Self {
            socket: Socket::connect_to(path)?,
        })
    }

    fn send(&mut self, request: Request) -> Result<Response, IpcError> {
        self.socket.send(request)?.map_err(IpcError::Reply)
    }

    /// Version string of the running compositor, e.g. `26.04 (8ed0da4)`.
    #[allow(clippy::wildcard_enum_match_arm)] // any other Response is an equal protocol error
    pub fn version(&mut self) -> Result<String, IpcError> {
        match self.send(Request::Version)? {
            Response::Version(v) => Ok(v),
            other => Err(unexpected_response("Version", &other)),
        }
    }

    /// Connected outputs: name → [`Output`] (includes modes, VRR, logical pos).
    #[allow(clippy::wildcard_enum_match_arm)] // any other Response is an equal protocol error
    pub fn outputs(&mut self) -> Result<HashMap<String, Output>, IpcError> {
        match self.send(Request::Outputs)? {
            Response::Outputs(map) => Ok(map),
            other => Err(unexpected_response("Outputs", &other)),
        }
    }

    /// All open windows.
    #[allow(clippy::wildcard_enum_match_arm)] // any other Response is an equal protocol error
    pub fn windows(&mut self) -> Result<Vec<Window>, IpcError> {
        match self.send(Request::Windows)? {
            Response::Windows(ws) => Ok(ws),
            other => Err(unexpected_response("Windows", &other)),
        }
    }

    /// Layer-shell surfaces (wallpaper daemons, panels, OSDs, ...).
    #[allow(clippy::wildcard_enum_match_arm)] // any other Response is an equal protocol error
    pub fn layers(&mut self) -> Result<Vec<LayerSurface>, IpcError> {
        match self.send(Request::Layers)? {
            Response::Layers(ls) => Ok(ls),
            other => Err(unexpected_response("Layers", &other)),
        }
    }

    /// Let the user click a window and return it (`None` when they cancel
    /// with Escape). Blocks until they choose.
    #[allow(clippy::wildcard_enum_match_arm)] // any other Response is an equal protocol error
    pub fn pick_window(&mut self) -> Result<Option<Window>, IpcError> {
        match self.send(Request::PickWindow)? {
            Response::PickedWindow(window) => Ok(window),
            other => Err(unexpected_response("PickWindow", &other)),
        }
    }

    /// Let the user click anywhere on screen; the color under the pointer as
    /// sRGB in 0..=1, or `None` when they cancelled with Escape.
    #[allow(clippy::wildcard_enum_match_arm)] // any other Response is an equal protocol error
    pub fn pick_color(&mut self) -> Result<Option<[f64; 3]>, IpcError> {
        match self.send(Request::PickColor)? {
            Response::PickedColor(color) => Ok(color.map(|c| c.rgb)),
            other => Err(unexpected_response("PickColor", &other)),
        }
    }

    /// Have niri start `command`. Unlike a child process, it runs outside
    /// this app's sandbox and environment, like any niri-spawned program.
    /// Bring window `id` to the front, switching workspace if need be.
    pub fn focus_window(&mut self, id: u64) -> Result<(), IpcError> {
        self.action(Action::FocusWindow { id })
    }

    pub fn spawn(&mut self, command: Vec<String>) -> Result<(), IpcError> {
        self.action(Action::Spawn { command })
    }

    /// Perform a compositor [`Action`] (used by the shortcut editor's live-try).
    pub fn action(&mut self, action: Action) -> Result<(), IpcError> {
        self.send(Request::Action(action))?;
        Ok(())
    }

    /// Switch into event-stream mode and return a blocking event reader.
    ///
    /// After this, this client must not be used for further `send`s; use a
    /// dedicated connection for the event stream (the GUI should open a second
    /// [`Client`] for `request`s).
    pub fn into_event_stream(self) -> Result<impl FnMut() -> io::Result<Event>, IpcError> {
        let mut socket = self.socket;
        socket
            .send(Request::EventStream)?
            .map_err(IpcError::Reply)?;
        Ok(socket.read_events())
    }
}
