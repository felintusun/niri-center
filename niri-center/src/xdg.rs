//! Where the app keeps its own files, sorted as the XDG Base Directory
//! spec sorts them: what you set goes in the config dir, what you make in
//! the data dir, what the app keeps track of in the state dir, and what
//! one save needs for a moment in the runtime dir. Each is a `niri-center`
//! folder in its base dir. Without a home folder there is nowhere to keep
//! anything, and nothing is written.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const APP: &str = "niri-center";

/// An XDG base directory: `$<variable>`, else `$HOME/<under_home>`.
pub fn base(variable: &str, under_home: &str) -> Option<PathBuf> {
    base_from(env::var_os(variable), env::var_os("HOME"), under_home)
}

/// The spec ignores a value that is empty or relative, and so does this.
fn base_from(value: Option<OsString>, home: Option<OsString>, under_home: &str) -> Option<PathBuf> {
    let absolute = |value: OsString| Some(PathBuf::from(value)).filter(|path| path.is_absolute());
    value
        .and_then(absolute)
        .or_else(|| Some(home.and_then(absolute)?.join(under_home)))
}

/// The app's own folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Places {
    /// `$XDG_CONFIG_HOME/niri-center/preferences`.
    pub preferences: PathBuf,
    /// `$XDG_DATA_HOME/niri-center/snapshots`: the snapshots you name.
    pub snapshots: PathBuf,
    /// `$XDG_STATE_HOME/niri-center/history`.
    pub history: PathBuf,
    /// `$XDG_STATE_HOME/niri-center/backups`, taken before every save.
    pub backups: PathBuf,
    /// `$XDG_RUNTIME_DIR/niri-center/probes`, where a save's files are
    /// mirrored for `niri validate` and removed after. Without a runtime
    /// dir, the cache dir stands in, as the spec asks.
    pub probes: PathBuf,
}

impl Places {
    /// This user's.
    pub fn find() -> Option<Self> {
        // the runtime dir has no default under home
        let runtime = base_from(env::var_os("XDG_RUNTIME_DIR"), None, "")
            .or_else(|| base("XDG_CACHE_HOME", ".cache"))?;
        Some(Self::from_bases(
            &base("XDG_CONFIG_HOME", ".config")?,
            &base("XDG_DATA_HOME", ".local/share")?,
            &base("XDG_STATE_HOME", ".local/state")?,
            &runtime,
        ))
    }

    fn from_bases(config: &Path, data: &Path, state: &Path, runtime: &Path) -> Self {
        Self {
            preferences: config.join(APP).join("preferences"),
            snapshots: data.join(APP).join("snapshots"),
            history: state.join(APP).join("history"),
            backups: state.join(APP).join("backups"),
            probes: runtime.join(APP).join("probes"),
        }
    }

    /// Every folder the app writes in.
    pub fn folders(&self) -> [PathBuf; 5] {
        // the preferences file's folder: it is always in one
        let config = self.preferences.parent().unwrap_or(&self.preferences);
        [
            config.to_path_buf(),
            self.snapshots.clone(),
            self.history.clone(),
            self.backups.clone(),
            self.probes.clone(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_base_dir_is_the_variable_else_under_home() {
        let home = Some(OsString::from("/home/me"));
        assert_eq!(
            base_from(Some("/xdg/state".into()), home.clone(), ".local/state"),
            Some(PathBuf::from("/xdg/state"))
        );
        assert_eq!(
            base_from(None, home, ".local/state"),
            Some(PathBuf::from("/home/me/.local/state"))
        );
        assert_eq!(base_from(None, None, ".local/state"), None);
    }

    #[test]
    fn an_empty_or_relative_base_dir_is_ignored() {
        let home = Some(OsString::from("/home/me"));
        for value in ["", "relative/state", "."] {
            assert_eq!(
                base_from(Some(value.into()), home.clone(), ".local/state"),
                Some(PathBuf::from("/home/me/.local/state")),
                "{value:?}"
            );
        }
        assert_eq!(base_from(None, Some("".into()), ".local/state"), None);
    }

    #[test]
    fn each_kind_of_file_goes_to_its_own_base_dir() {
        let places = Places::from_bases(
            Path::new("/c"),
            Path::new("/d"),
            Path::new("/s"),
            Path::new("/r"),
        );
        assert_eq!(places.preferences, Path::new("/c/niri-center/preferences"));
        assert_eq!(places.snapshots, Path::new("/d/niri-center/snapshots"));
        assert_eq!(places.history, Path::new("/s/niri-center/history"));
        assert_eq!(places.backups, Path::new("/s/niri-center/backups"));
        assert_eq!(places.probes, Path::new("/r/niri-center/probes"));
        assert_eq!(places.folders()[0], Path::new("/c/niri-center"));
    }
}
