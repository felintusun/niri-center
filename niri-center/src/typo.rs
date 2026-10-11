//! A name niri does not know, as its diagnostics point at it, and the
//! name it was probably meant to be.

use std::path::PathBuf;

use crate::settings::REGISTRY;
use crate::store::Store;

/// Node names niri knows outside the settings table.
const KNOWN: &[&str] = &[
    "input",
    "output",
    "layout",
    "binds",
    "window-rule",
    "layer-rule",
    "workspace",
    "animations",
    "gestures",
    "overview",
    "environment",
    "cursor",
    "include",
    "spawn-at-startup",
    "spawn-sh-at-startup",
    "switch-events",
    "debug",
    "match",
    "exclude",
    "mode",
    "scale",
    "transform",
    "position",
    "variable-refresh-rate",
    "focus-at-startup",
    "off",
    "on",
];

/// A name niri does not know, where it is, and what it probably meant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Typo {
    pub path: PathBuf,
    pub line: usize,
    pub name: String,
    pub meant: Option<String>,
}

/// The known name closest to `name`, if one is close: a letter or two
/// added, dropped or changed, two swapped counting once.
pub fn meant(name: &str) -> Option<String> {
    let registry = REGISTRY.iter().flat_map(|spec| {
        spec.chain
            .iter()
            .map(|block| block.name)
            .chain(std::iter::once(spec.child))
    });
    registry
        .chain(KNOWN.iter().copied())
        .filter(|known| !known.is_empty() && *known != name)
        .map(|known| (strsim::osa_distance(name, known), known))
        .filter(|(far, _)| *far <= 2)
        .min_by_key(|(far, _)| *far)
        .map(|(_, known)| known.to_string())
}

/// What niri's diagnosis points at, when it is a name it does not know.
pub fn typo(store: &Store, diagnostics: &str) -> Option<Typo> {
    let (name, path, line) = store.misspelled(diagnostics)?;
    Some(Typo {
        path: path.to_path_buf(),
        line,
        meant: meant(&name),
        name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_misspelled_name_finds_the_name_it_was_meant_to_be() {
        assert_eq!(meant("widht").as_deref(), Some("width"));
        assert_eq!(meant("focus-rnig").as_deref(), Some("focus-ring"));
        assert_eq!(meant("completely-unknown-thing"), None);
    }
}
