//! The app's own preferences, in [`Places::preferences`](crate::xdg::Places):
//! one `key = value` a line.
//! Unknown keys are kept as they are, so an older app does not drop a
//! newer one's settings.

use std::fs;
use std::io;
use std::path::Path;

/// The value of `key`, if set.
pub fn get(path: &Path, key: &str) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    text.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim().to_string())
    })
}

/// Set `key` to `value`, keeping every other line.
pub fn set(path: &Path, key: &str, value: &str) -> io::Result<()> {
    let text = fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = text
        .lines()
        .filter(|line| {
            line.split_once('=')
                .is_none_or(|(name, _)| name.trim() != key)
        })
        .map(str::to_string)
        .collect();
    lines.push(format!("{key} = {value}"));
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, lines.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_set_replaced_and_others_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences");
        assert_eq!(get(&path, "save"), None);
        fs::write(&path, "future = yes\n").unwrap();
        set(&path, "save", "on-request").unwrap();
        set(&path, "save", "on-change").unwrap();
        assert_eq!(get(&path, "save").as_deref(), Some("on-change"));
        assert_eq!(get(&path, "future").as_deref(), Some("yes"));
    }
}
