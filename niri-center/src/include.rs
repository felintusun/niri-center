//! `include`-file resolution and node→origin-file mapping.
//!
//! Niri supports `include "path.kdl"` (niri ≥ 25.11). Real-world niri setups
//! split their config the way the default tooling encourages: a thin
//! `config.kdl` that `include`s `general.kdl`, `keybinds.kdl`,
//! `spawn_at_startup.kdl`, `window_rules.kdl`, … (exactly what the default
//! `/usr/share/doc/niri/default-config.kdl` documents).
//!
//! This module walks that tree: it resolves `include` nodes depth-first,
//! guarding against cycles and depth bombs, and hands the UI a flat list of
//! parsed files so edits can be written back to the file that *owns* each node.
//!
//! Guarantee shared with the rest of the core: only the file containing the
//! edited node is ever touched; every other byte of every file survives.

use std::collections::HashSet;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::kdl::{Document, active_named, parse_document};

/// Maximum include depth (mirrors NiriMod's limit).
pub const MAX_INCLUDE_DEPTH: usize = 5;

/// A file pulled in via `include`, in depth-first order.
#[derive(Debug, Clone)]
pub struct IncludedFile {
    pub path: PathBuf,
    /// Pristine on-disk content (never mutated) — the baseline for diffs.
    pub original: String,
    pub doc: Document,
}

/// Resolve all `include`s reachable from `main_doc` (which lives at
/// `main_path`).
///
/// Returns the included files in depth-first order plus any non-fatal problems
/// (missing files, parse errors, cycles) as warnings.
/// Files are read through `read`, so the caller decides where they come
/// from: the disk, or memory.
pub fn resolve(
    main_path: &Path,
    main_doc: &Document,
    read: &mut impl FnMut(&Path) -> io::Result<String>,
) -> (Vec<IncludedFile>, Vec<String>) {
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    seen.insert(normalized(main_path));
    resolve_from(
        main_path,
        main_doc,
        0,
        read,
        &mut out,
        &mut warnings,
        &mut seen,
    );
    (out, warnings)
}

fn resolve_from(
    file_path: &Path,
    doc: &Document,
    depth: usize,
    read: &mut impl FnMut(&Path) -> io::Result<String>,
    out: &mut Vec<IncludedFile>,
    warnings: &mut Vec<String>,
    seen: &mut HashSet<PathBuf>,
) {
    if depth >= MAX_INCLUDE_DEPTH {
        warnings.push(format!(
            "maximum include depth ({MAX_INCLUDE_DEPTH}) reached while processing {}",
            file_path.display()
        ));
        return;
    }
    // niri skips `/-include`, so a disabled include is never loaded or edited
    let includes: Vec<&str> = active_named(&doc.roots, "include")
        .filter_map(|n| n.args.first())
        .filter_map(|v| match &v.kind {
            crate::kdl::ValueKind::Quoted(s) if !s.is_empty() => Some(s.as_str()),
            crate::kdl::ValueKind::Quoted(_)
            | crate::kdl::ValueKind::Bare(_)
            | crate::kdl::ValueKind::Number(_) => None,
        })
        .collect();

    for rel in includes {
        let target = include_path(file_path, rel);
        if !seen.insert(normalized(&target)) {
            warnings.push(format!(
                "include cycle or duplicate: {rel} (requested by {})",
                file_path.display()
            ));
            continue;
        }
        let source = match read(&target) {
            Ok(s) => s,
            Err(e) => {
                warnings.push(format!(
                    "cannot read include {rel} (requested by {}): {e}",
                    file_path.display()
                ));
                continue;
            }
        };
        match parse_document(&source) {
            Ok(included_doc) => {
                // recurse *before* moving included_doc into the result list
                resolve_from(&target, &included_doc, depth + 1, read, out, warnings, seen);
                out.push(IncludedFile {
                    path: target.clone(),
                    original: source,
                    doc: included_doc,
                });
            }
            Err(e) => warnings.push(format!(
                "parse error in {rel} (requested by {}): {e}",
                file_path.display()
            )),
        }
    }
}

/// Where `include "<rel>"` in `including_file` points: relative to that
/// file's directory, absolute as written, or under `$HOME` for `~/…`
/// (niri ≥ 26.04).
pub fn include_path(including_file: &Path, rel: &str) -> PathBuf {
    if let Some(in_home) = rel.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(in_home);
    }
    including_file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join(rel)
}

/// A path with `.` and `..` worked out, for telling a cycle or a file
/// included twice. Links are not followed: the depth limit stops a cycle
/// through one.
fn normalized(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in p.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => out.push(part),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::include_path;
    use std::path::{Path, PathBuf};

    #[test]
    fn include_paths_resolve_like_niri() {
        let config = Path::new("/etc/niri/config.kdl");
        assert_eq!(
            include_path(config, "binds.kdl"),
            PathBuf::from("/etc/niri/binds.kdl")
        );
        assert_eq!(
            include_path(config, "/opt/shared.kdl"),
            PathBuf::from("/opt/shared.kdl")
        );
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap();
        assert_eq!(include_path(config, "~/local.kdl"), home.join("local.kdl"));
    }
}
