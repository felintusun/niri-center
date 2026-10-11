//! Helpers that shell out to the `niri` binary (CLI side-car).

use std::process::Command;

/// Parse the first line of `niri --version`, e.g. `niri 26.04 (8ed0da4)`.
pub fn version() -> Option<String> {
    let out = Command::new("niri").arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().next().map(|s| s.trim().to_string())
}
