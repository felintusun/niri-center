//! Small helpers for the app's words: counts, numbers as people type them,
//! lists and paths in fields.

/// Text written into the app's tables: words people read (a label, a help
/// line) or niri's own words (a node name, a default value). Not a set the
/// code chooses among: those are enums.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Words(pub &'static str);

impl Words {
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl std::ops::Deref for Words {
    type Target = str;

    fn deref(&self) -> &str {
        self.0
    }
}

impl std::fmt::Display for Words {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(self.0)
    }
}

/// `amount` things in words: "1 rule", "3 rules".
pub fn count(amount: usize, one: &str, many: &str) -> String {
    if amount == 1 {
        format!("1 {one}")
    } else {
        format!("{amount} {many}")
    }
}

/// The word that agrees with `amount` things: "DP-1 sets", "DP-1, HDMI-A-1 set".
pub const fn agree<'text>(amount: usize, one: &'text str, many: &'text str) -> &'text str {
    if amount == 1 { one } else { many }
}

/// Names in a sentence with the verb that agrees: "DP-1 sets", "DP-1,
/// HDMI-A-1 set".
pub fn who(names: &[String], one: &str, many: &str) -> String {
    format!("{} {}", names.join(", "), agree(names.len(), one, many))
}

/// `text` cut to at most `max` characters, the last of them "…".
pub fn shortened(text: String, max: usize) -> String {
    if text.chars().count() > max {
        let kept: String = text.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", kept.trim_end())
    } else {
        text
    }
}

/// `0.50` → `0.5`, `1.00` → `1`: what a person would type.
pub fn short_number(value: f64) -> String {
    decimals(value, 2)
}

/// `0.333333` → `0.33333`: the precision niri's own config uses.
pub fn short_fraction(value: f32) -> String {
    decimals(f64::from(value), 5)
}

/// `value` to at most `places` decimals, without trailing zeros.
fn decimals(value: f64, places: usize) -> String {
    let text = format!("{value:.places$}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".to_string()
    } else {
        text.to_string()
    }
}

/// `us, de` as `["us", "de"]`.
pub fn comma_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

/// Folder and file name apart: `~/Pictures/Screenshots` and
/// `Screenshot from %Y-%m-%d %H-%M-%S.png`.
pub fn split_path(path: &str) -> (String, String) {
    path.rsplit_once('/').map_or_else(
        || (String::new(), path.to_string()),
        |(folder, file)| (folder.to_string(), file.to_string()),
    )
}

pub fn join_path(folder: &str, file: &str) -> String {
    let folder = folder.trim_end_matches('/');
    if folder.is_empty() {
        file.to_string()
    } else {
        format!("{folder}/{file}")
    }
}

/// Text cut out of an indented block, its first line already without
/// indentation: the other lines lose what they share.
pub fn dedent(text: &str) -> String {
    let mut lines = text.lines();
    let first = lines.next().unwrap_or_default();
    let rest: Vec<&str> = lines.collect();
    let indent = rest
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    std::iter::once(first.to_string())
        .chain(rest.iter().map(|line| {
            line.get(indent..)
                .unwrap_or_else(|| line.trim_start())
                .to_string()
        }))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Why a `screenshot-path` pattern gives no file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PatternError {
    #[error("The name has a % code niri cannot fill in.")]
    UnknownCode,
}

/// The file name niri would write now for a `screenshot-path` pattern, or
/// why it cannot.
pub fn expand_strftime(pattern: &str) -> Result<String, PatternError> {
    use chrono::format::{Item, StrftimeItems};
    if StrftimeItems::new(pattern).any(|item| matches!(item, Item::Error)) {
        return Err(PatternError::UnknownCode);
    }
    Ok(chrono::Local::now().format(pattern).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_read_the_way_people_type_them() {
        assert_eq!(short_number(0.5), "0.5");
        assert_eq!(short_number(1.0), "1");
        assert_eq!(short_number(-0.001), "0");
        assert_eq!(short_fraction(1.0 / 3.0), "0.33333");
    }

    #[test]
    fn screenshot_names_expand_like_strftime() {
        let name = expand_strftime("Screenshot %Y.png").unwrap();
        assert!(
            name.starts_with("Screenshot 2") && name.ends_with(".png"),
            "{name}"
        );
        assert_eq!(
            expand_strftime("bad %Q").unwrap_err().to_string(),
            "The name has a % code niri cannot fill in."
        );
    }

    #[test]
    fn names_take_the_verb_that_agrees() {
        assert_eq!(who(&["DP-1".into()], "sets", "set"), "DP-1 sets");
        assert_eq!(
            who(&["DP-1".into(), "eDP-1".into()], "sets", "set"),
            "DP-1, eDP-1 set"
        );
    }
}
