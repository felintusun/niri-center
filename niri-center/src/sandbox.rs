//! Landlock-based filesystem sandbox.
//!
//! Restricts every thread of this process (and its children, e.g.
//! `niri validate`) so that:
//!
//! * **read + execute** are allowed anywhere on the filesystem (so the niri
//!   binary, its libraries, and the config files it loads stay readable);
//! * **write access** (create/remove/rename/truncate…) is only allowed beneath
//!   an explicit allow-list: the niri config directories and the app's own
//!   folders ([`Places`]: preferences, snapshots, history, backups, probes);
//! * **connecting** to pathname UNIX sockets (Landlock ABI v9+) is only
//!   allowed for niri's IPC socket and the Wayland display.
//!
//! Everything else — `/tmp`, `$HOME`, other config roots, other sockets — is
//! denied by the kernel. Only files opened *after* [`apply`] are checked, which
//! is why the GUI applies it once iced holds its GPU and display handles.
//!
//! On kernels below ABI v8 only the calling thread (and threads it spawns
//! later) is restricted; [`SandboxStatus::Active`] reports which case held. The app already serializes all file writes through a tiny
//! surface (the disk actor, and the preferences), so the allow-list has a single
//! place to be audited.
//!
//! This is a *best-effort* security feature: on kernels without Landlock
//! (or when `NIRI_CENTER_NO_SANDBOX` is set) the app still works, fully
//! unsandboxed, and reports why via [`apply`]'s status.

use std::env;
use std::path::PathBuf;

use crate::xdg::Places;

/// Create the app's folders before the sandbox is applied: it lets the
/// app write only in folders that exist.
#[cfg(target_os = "linux")]
fn ensure_folders(places: &Places) {
    for dir in places.folders() {
        let _ = std::fs::create_dir_all(&dir);
    }
}

/// Outcome of a sandboxing attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxStatus {
    /// Sandbox is enforced (kernel Landlock active at the given ABI).
    /// `all_threads` is false on kernels without thread sync (ABI < 8): then
    /// threads that already existed at [`apply`] stay unrestricted.
    Active { abi: String, all_threads: bool },
    /// Disabled via `NIRI_CENTER_NO_SANDBOX`.
    OptedOut,
    /// Kernel/container cannot enforce it; the app keeps working unsandboxed.
    Unsupported(Unsupported),
}

/// Why the system cannot enforce the sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsupported {
    NotBuiltIn,
    /// Compiled in, but the kernel's LSM order leaves it out.
    NotEnabled,
    NotLinux,
}

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotBuiltIn => "Landlock not built into this kernel",
            Self::NotEnabled => "Landlock compiled but not enabled (LSM ordering)",
            Self::NotLinux => "not Linux",
        })
    }
}

impl SandboxStatus {
    /// The status in a sentence, for the Files page.
    pub fn describe(&self) -> String {
        match self {
            Self::Active {
                abi,
                all_threads: true,
            } => {
                format!(
                    "Writes are limited to the config folder and the app's own (Landlock ABI {abi})."
                )
            }
            Self::Active {
                abi,
                all_threads: false,
            } => format!(
                "Writes are limited on the main thread only (Landlock ABI {abi}). This kernel cannot restrict threads that already run."
            ),
            Self::OptedOut => {
                "The sandbox is off because NIRI_CENTER_NO_SANDBOX is set.".to_string()
            }
            Self::Unsupported(reason) => format!("No sandbox: {reason}."),
        }
    }
}

/// Allow-list constructor: where the app may write.
pub fn writable_roots(config_dirs: &[PathBuf], places: Option<&Places>) -> Vec<PathBuf> {
    let mut roots = config_dirs.to_vec();
    roots.extend(places.map(Places::folders).unwrap_or_default());
    roots
}

/// The only pathname sockets the app may connect to.
///
/// niri's IPC socket (`$NIRI_SOCKET`), the Wayland display, which pop-out
/// panes need, and the session bus, where the file chooser portal answers
/// Browse buttons.
pub fn connectable_sockets() -> Vec<PathBuf> {
    let mut sockets = Vec::new();
    if let Some(niri) = env::var_os("NIRI_SOCKET") {
        sockets.push(PathBuf::from(niri));
    }
    sockets.extend(session_bus());
    if let Some(display) = env::var_os("WAYLAND_DISPLAY").map(PathBuf::from) {
        if display.is_absolute() {
            sockets.push(display);
        } else if let Some(runtime_dir) = env::var_os("XDG_RUNTIME_DIR") {
            sockets.push(PathBuf::from(runtime_dir).join(display));
        }
    }
    sockets
}

/// The session bus socket from `$DBUS_SESSION_BUS_ADDRESS`
/// (`unix:path=/run/user/1000/bus,guid=…`), or the usual
/// `$XDG_RUNTIME_DIR/bus`. Abstract sockets have no path to allow.
fn session_bus() -> Option<PathBuf> {
    env::var("DBUS_SESSION_BUS_ADDRESS").map_or_else(
        |_| env::var_os("XDG_RUNTIME_DIR").map(|dir| PathBuf::from(dir).join("bus")),
        |address| {
            address.split(';').find_map(|entry| {
                entry
                    .strip_prefix("unix:")?
                    .split(',')
                    .find_map(|pair| pair.strip_prefix("path="))
                    .map(PathBuf::from)
            })
        },
    )
}

/// Why the sandbox could not be set up (distinct from "unsupported", which is
/// a status, not a failure).
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[cfg(target_os = "linux")]
    #[error("Landlock {step}: {source}")]
    Landlock {
        step: Step,
        #[source]
        source: landlock::RulesetError,
    },
}

/// A step of setting up Landlock, to say which one failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    HandleAccess,
    CreateRuleset,
    AddReadRules,
    AddWriteRules,
    AddSocketRules,
    AllThreads,
    RestrictSelf,
}

impl std::fmt::Display for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::HandleAccess => "handle_access",
            Self::CreateRuleset => "create ruleset",
            Self::AddReadRules => "add read rules",
            Self::AddWriteRules => "add write rules",
            Self::AddSocketRules => "add socket rules",
            Self::AllThreads => "all threads",
            Self::RestrictSelf => "restrict_self",
        })
    }
}

/// Apply the filesystem sandbox.
///
/// `config_dirs` are where saves write (the config directory and, for a
/// symlinked config, its resolved directory); they and the app's own folders become
/// the only writable places.
#[cfg(target_os = "linux")]
pub fn apply(config_dirs: &[PathBuf]) -> Result<SandboxStatus, SandboxError> {
    use landlock::{
        ABI, Access, AccessFs, CompatLevel, Compatible, LandlockStatus, RestrictSelfAttr, Ruleset,
        RulesetAttr, RulesetCreatedAttr, path_beneath_rules,
    };

    if env::var_os("NIRI_CENTER_NO_SANDBOX").is_some() {
        return Ok(SandboxStatus::OptedOut);
    }

    let places = Places::find();
    if let Some(places) = &places {
        ensure_folders(places);
    }
    let writable = writable_roots(config_dirs, places.as_ref());

    // Best-effort: kernels with a lower ABI simply drop the newer rights,
    // and an entirely absent Landlock LSM yields a status we can inspect.
    let abi = ABI::V9;
    let read_all = AccessFs::from_read(abi);
    let write_all = AccessFs::from_all(abi);
    let failed = |step| move |source| SandboxError::Landlock { step, source };

    let created = Ruleset::default()
        .set_compatibility(CompatLevel::BestEffort)
        .handle_access(read_all | write_all)
        .map_err(failed(Step::HandleAccess))?
        .create()
        .map_err(failed(Step::CreateRuleset))?;

    let status = created
        .add_rules(path_beneath_rules(
            std::iter::once(PathBuf::from("/")),
            read_all,
        ))
        .map_err(failed(Step::AddReadRules))?
        .add_rules(path_beneath_rules(writable, write_all))
        .map_err(failed(Step::AddWriteRules))?
        // missing sockets are skipped; below ABI v9 connects are unrestricted
        .add_rules(path_beneath_rules(
            connectable_sockets(),
            AccessFs::ResolveUnix,
        ))
        .map_err(failed(Step::AddSocketRules))?
        .all_threads(true)
        .map_err(failed(Step::AllThreads))?
        .restrict_self()
        .map_err(failed(Step::RestrictSelf))?;

    match status.landlock {
        LandlockStatus::NotImplemented => Ok(SandboxStatus::Unsupported(Unsupported::NotBuiltIn)),
        LandlockStatus::NotEnabled => Ok(SandboxStatus::Unsupported(Unsupported::NotEnabled)),
        LandlockStatus::Available { effective_abi, .. } => Ok(SandboxStatus::Active {
            abi: format!("{:?}", effective_abi)
                .trim_start_matches("V")
                .to_string(),
            all_threads: status.all_threads,
        }),
    }
}

/// Non-Linux fallback (the project targets Linux; the browser demo builds
/// this one).
#[cfg(not(target_os = "linux"))]
pub fn apply(_config_dirs: &[PathBuf]) -> Result<SandboxStatus, SandboxError> {
    Ok(SandboxStatus::Unsupported(Unsupported::NotLinux))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opt_out_env_is_honored_without_touching_the_kernel() {
        unsafe {
            std::env::set_var("NIRI_CENTER_NO_SANDBOX", "1");
        }
        let status = apply(&[PathBuf::from("/tmp")]).unwrap();
        assert_eq!(status, SandboxStatus::OptedOut);
        unsafe {
            std::env::remove_var("NIRI_CENTER_NO_SANDBOX");
        }
    }

    #[test]
    fn writable_roots_are_the_config_dirs_and_the_apps_own() {
        let places = Places::find();
        let roots = writable_roots(&[PathBuf::from("/tmp/niri-config-tests")], places.as_ref());
        assert!(
            roots
                .iter()
                .any(|r| r == std::path::Path::new("/tmp/niri-config-tests"))
        );
        for folder in places.map(|places| places.folders()).unwrap_or_default() {
            assert!(
                folder.ends_with("niri-center")
                    || folder.parent().is_some_and(|p| p.ends_with("niri-center"))
            );
            assert!(roots.contains(&folder));
        }
        assert!(
            roots.len() <= 6,
            "nothing but the config and the app's folders"
        );
    }
}
