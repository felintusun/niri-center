//! Named, restorable config snapshots (beyond the rolling auto-backups).
//!
//! A snapshot captures the current state of every config file under a
//! user-chosen name, stored in `$XDG_DATA_HOME/niri-center/snapshots/<name>/`.
//! Unlike the timestamped backups written on every save, snapshots are
//! explicit checkpoints the user creates, lists, restores, and deletes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Metadata for a listed snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotMeta {
    pub name: String,
    pub created_nanos: u128,
    pub note: String,
}

/// Restored snapshot contents, by config-file display name.
#[derive(Debug, Default)]
pub struct SnapshotFiles {
    pub files: Vec<(String, String)>,
    pub note: String,
}

/// Snapshot directory name for a user-supplied name.
fn slug(name: &str) -> Option<String> {
    let cleaned: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect::<String>()
        .trim()
        .trim_start_matches('.')
        .to_string();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        None
    } else {
        Some(cleaned)
    }
}

fn invalid(name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("invalid snapshot name: {name:?}"),
    )
}

/// Store `files` (display name → content) under snapshot `name`.
pub fn create(
    dir: &Path,
    name: &str,
    files: &[(String, String)],
    note: &str,
) -> io::Result<PathBuf> {
    let slug = slug(name).ok_or_else(|| invalid(name))?;
    let target = dir.join(&slug);
    fs::create_dir_all(&target)?;
    for (display, content) in files {
        fs::write(target.join(display.replace('/', "_")), content)?;
    }
    let nanos = crate::clock::now_nanos();
    fs::write(target.join(".meta"), format!("{nanos}\n{name}\n{note}"))?;
    Ok(target)
}

/// List snapshots, oldest first.
pub fn list(dir: &Path) -> Vec<SnapshotMeta> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let (display, created_nanos, note) = fs::read_to_string(path.join(".meta")).map_or_else(
            |_| (name.clone(), 0, String::new()),
            |text| {
                let mut lines = text.lines();
                let created = lines
                    .next()
                    .and_then(|s| s.parse::<u128>().ok())
                    .unwrap_or(0);
                let stored_name = lines.next().unwrap_or(&name);
                let display = if stored_name.is_empty() {
                    &name
                } else {
                    stored_name
                };
                let note = lines.collect::<Vec<_>>().join("\n");
                (display.to_string(), created, note)
            },
        );
        out.push(SnapshotMeta {
            name: display,
            created_nanos,
            note,
        });
    }
    out.sort_by_key(|s| s.created_nanos);
    out
}

/// Load a snapshot's files and note.
pub fn load(dir: &Path, name: &str) -> io::Result<SnapshotFiles> {
    let root = dir.join(slug(name).ok_or_else(|| invalid(name))?);
    let mut files = Vec::new();
    for entry in fs::read_dir(&root)?.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.file_name().and_then(|s| s.to_str()) == Some(".meta") {
            continue;
        }
        let display = entry.file_name().to_string_lossy().into_owned();
        files.push((display, fs::read_to_string(&path)?));
    }
    let note = fs::read_to_string(root.join(".meta")).map_or_else(
        |_| String::new(),
        |text| text.lines().skip(2).collect::<Vec<_>>().join("\n"),
    );
    Ok(SnapshotFiles { files, note })
}

/// Remove a snapshot directory.
pub fn delete(dir: &Path, name: &str) -> io::Result<()> {
    fs::remove_dir_all(dir.join(slug(name).ok_or_else(|| invalid(name))?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_list_load_delete_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap().path().join("snapshots");
        fs::create_dir_all(&dir).unwrap();

        create(
            &dir,
            "my setup",
            &[
                ("config.kdl".into(), "a".into()),
                ("keybinds.kdl".into(), "b".into()),
            ],
            "first snapshot",
        )
        .unwrap();

        let metas = list(&dir);
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].name, "my setup");
        assert_eq!(metas[0].note, "first snapshot");

        let loaded = load(&dir, "my setup").unwrap();
        assert_eq!(loaded.files.len(), 2);
        assert!(
            loaded
                .files
                .iter()
                .any(|(n, c)| n == "config.kdl" && c == "a")
        );
        assert_eq!(loaded.note, "first snapshot");

        delete(&dir, "my setup").unwrap();
        assert!(list(&dir).is_empty());
    }

    #[test]
    fn rejects_path_traversal_names() {
        let dir = tempfile::TempDir::new().unwrap().path().join("snapshots");
        fs::create_dir_all(&dir).unwrap();
        // `../escape` sanitizes to `escape`; absolute-path chars are dropped
        let ok = slug("../escape").unwrap();
        assert_ne!(ok, "../escape");
        assert!(slug("").is_none());
        assert!(slug("..").is_none());
    }
}
