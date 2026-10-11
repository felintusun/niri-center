//! Which window rules reach which windows, worked out the way niri does: a
//! rule reaches a window when one of its `match` lines does (or it has
//! none) and none of its `exclude` lines does; a line matches when all of
//! its properties do. `app-id` and `title` are niri's own regular
//! expressions, run with the same crate niri uses.

use crate::niri::OpenWindow;
use crate::store::{MatchName, MatchProp, Matcher, RuleProperty, TextMatch, WindowRuleRow};

/// A window's state a new rule can ask for: one `is-…` match property
/// each, in words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Focused,
    NotFocused,
    Floating,
    Tiled,
    AsksForAttention,
    Screencast,
    AtStartup,
}

impl When {
    pub const ALL: [Self; 7] = [
        Self::Focused,
        Self::NotFocused,
        Self::Floating,
        Self::Tiled,
        Self::AsksForAttention,
        Self::Screencast,
        Self::AtStartup,
    ];

    /// The state a match property asks for, when it is one of these.
    pub fn from_prop(name: &MatchName, yes: bool) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|when| when.prop() == (name.clone(), yes))
    }

    /// The match property: its name and value.
    pub const fn prop(self) -> (MatchName, bool) {
        match self {
            Self::Focused => (MatchName::IsFocused, true),
            Self::NotFocused => (MatchName::IsFocused, false),
            Self::Floating => (MatchName::IsFloating, true),
            Self::Tiled => (MatchName::IsFloating, false),
            Self::AsksForAttention => (MatchName::IsUrgent, true),
            Self::Screencast => (MatchName::IsWindowCastTarget, true),
            Self::AtStartup => (MatchName::AtStartup, true),
        }
    }
}

impl std::fmt::Display for When {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Focused => "is focused",
            Self::NotFocused => "is not focused",
            Self::Floating => "is floating",
            Self::Tiled => "is tiled",
            Self::AsksForAttention => "asks for attention",
            Self::Screencast => "is being screencast",
            Self::AtStartup => "opened as niri started",
        })
    }
}

/// Whether a rule reaches a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    Reaches,
    /// It does not; when it nearly does, why not, in a sentence fragment
    /// ("the title has to match …").
    Misses(Option<String>),
    /// It depends on something the app cannot see from here (a window's
    /// workspace, whether it opened at startup).
    Unknown,
}

/// What one property of a line says about the window.
enum Fit {
    Yes,
    No(String),
    Unknown,
}

fn fit(prop: &MatchProp, window: &OpenWindow) -> Fit {
    let MatchProp { name, value, .. } = prop;
    let pattern =
        |text: &str, what: &str| match prop.pattern.as_ref().map(|regex| regex.is_match(text)) {
            Some(true) => Fit::Yes,
            Some(false) => Fit::No(format!("the {what} has to match “{value}”")),
            // niri refuses the config before such a rule matters
            None => Fit::Unknown,
        };
    let flag = |state: bool, what: &str| {
        let wanted = value != "false";
        if state == wanted {
            Fit::Yes
        } else if wanted {
            Fit::No(format!("it only reaches {what} windows"))
        } else {
            Fit::No(format!("it leaves out {what} windows"))
        }
    };
    match name {
        MatchName::AppId => pattern(&window.app_id, "app ID"),
        MatchName::Title => pattern(&window.title, "title"),
        MatchName::IsFloating => flag(window.floating, "floating"),
        MatchName::IsFocused | MatchName::IsActive => flag(window.focused, "focused"),
        MatchName::IsActiveInColumn
        | MatchName::IsUrgent
        | MatchName::IsWindowCastTarget
        | MatchName::AtStartup
        | MatchName::Other(_) => Fit::Unknown,
    }
}

/// Whether one `match` or `exclude` line matches: `None` when it cannot
/// be told, else the reason it does not, if it does not.
fn line(matcher: &Matcher, window: &OpenWindow) -> Option<Result<(), String>> {
    let mut miss = None;
    for prop in &matcher.props {
        match fit(prop, window) {
            Fit::Yes => {}
            Fit::No(why) => {
                if miss.is_none() {
                    miss = Some(why);
                }
            }
            Fit::Unknown => return None,
        }
    }
    Some(miss.map_or(Ok(()), Err))
}

pub fn reach(rule: &WindowRuleRow, window: &OpenWindow) -> Reach {
    let reach = reach_when_on(&rule.matchers, window);
    if !rule.disabled {
        return reach;
    }
    // a rule turned off is only worth a word where it would apply
    if reach == Reach::Reaches {
        Reach::Misses(Some("it is turned off".to_string()))
    } else {
        Reach::Misses(None)
    }
}

/// Whether a rule on is said to reach a window by its `match` and `exclude`
/// lines.
pub fn reach_when_on(matchers: &[Matcher], window: &OpenWindow) -> Reach {
    let (excludes, matches): (Vec<&Matcher>, Vec<&Matcher>) =
        matchers.iter().partition(|m| m.exclude);
    for exclude in excludes {
        match line(exclude, window) {
            Some(Ok(())) => {
                return Reach::Misses(Some("an exclude line leaves this window out".into()));
            }
            Some(Err(_)) => {}
            None => return Reach::Unknown,
        }
    }
    if matches.is_empty() {
        return Reach::Reaches;
    }
    let mut unknown = false;
    let mut near = None;
    for matcher in matches {
        match line(matcher, window) {
            Some(Ok(())) => return Reach::Reaches,
            Some(Err(why)) => {
                // a near miss: every other property of the line fits
                let misses = matcher
                    .props
                    .iter()
                    .filter(|prop| !matches!(fit(prop, window), Fit::Yes))
                    .count();
                if misses == 1 && matcher.props.len() > 1 && near.is_none() {
                    near = Some(why);
                }
            }
            None => unknown = true,
        }
    }
    if unknown {
        Reach::Unknown
    } else {
        Reach::Misses(near)
    }
}

/// A rule's name in lists: who it is for and what it does first.
pub fn name(rule: &WindowRuleRow) -> String {
    let who = rule
        .matchers
        .iter()
        .find(|m| !m.exclude)
        .and_then(|m| {
            m.props
                .iter()
                .find(|prop| prop.name == MatchName::AppId)
                .or_else(|| m.props.iter().find(|prop| prop.name == MatchName::Title))
        })
        .map_or_else(|| "Every window".to_string(), |prop| readable(&prop.value));
    let what = what(rule);
    if what.is_empty() {
        who
    } else {
        format!("{who}: {what}")
    }
}

/// What a rule does first, as its name says it: `open floating`.
pub fn what(rule: &WindowRuleRow) -> String {
    rule.properties.first().map_or_else(String::new, |line| {
        RuleProperty::read(line).map_or_else(
            || line.trim().to_string(),
            |property| property.label().to_lowercase(),
        )
    })
}

/// What else than the app picks a rule's windows, in words: its first
/// `match` line's title and states (`title Picture-in-Picture, is
/// floating`). `None` when the app alone does.
pub fn condition(rule: &WindowRuleRow) -> Option<String> {
    let matcher = rule.matchers.iter().find(|m| !m.exclude)?;
    let words: Vec<String> = matcher
        .props
        .iter()
        .filter_map(|prop| match &prop.name {
            MatchName::AppId => None,
            MatchName::Title => Some(format!("title {}", readable(&prop.value))),
            name @ (MatchName::IsActive
            | MatchName::IsFocused
            | MatchName::IsActiveInColumn
            | MatchName::IsFloating
            | MatchName::IsUrgent
            | MatchName::IsWindowCastTarget
            | MatchName::AtStartup
            | MatchName::Other(_)) => {
                Some(When::from_prop(name, prop.value != "false").map_or_else(
                    || format!("{} {}", name.as_str(), prop.value),
                    |when| when.to_string(),
                ))
            }
        })
        .collect();
    (!words.is_empty()).then(|| words.join(", "))
}

/// A pattern as words where it is plain text: `^org\.gnome\.Calculator$`
/// reads `org.gnome.Calculator`.
fn readable(pattern: &str) -> String {
    match TextMatch::read(pattern) {
        (TextMatch::Pattern, pattern) => pattern
            .trim_start_matches('^')
            .trim_end_matches('$')
            .to_string(),
        (TextMatch::Exact | TextMatch::Contains, text) => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(app_id: &str, title: &str) -> OpenWindow {
        OpenWindow {
            id: 0,
            app_id: app_id.into(),
            title: title.into(),
            floating: false,
            focused: false,
        }
    }

    fn rule(lines: &[(bool, &[(&str, &str)])]) -> WindowRuleRow {
        WindowRuleRow {
            criteria: Vec::new(),
            matchers: lines
                .iter()
                .map(|(exclude, props)| Matcher {
                    exclude: *exclude,
                    props: props
                        .iter()
                        .map(|(name, value)| MatchProp::new(MatchName::read(name), *value))
                        .collect(),
                })
                .collect(),
            line: 1,
            properties: vec!["open-floating true".into()],
            file: "config.kdl".into(),
            included: false,
            disabled: false,
            plain: true,
        }
    }

    #[test]
    fn reach_follows_niris_match_and_exclude() {
        let pip = window("firefox", "Picture-in-Picture");
        assert_eq!(reach(&rule(&[]), &pip), Reach::Reaches);
        assert_eq!(
            reach(&rule(&[(false, &[("app-id", "^firefox$")])]), &pip),
            Reach::Reaches
        );
        // a search, not a whole-string match, unless anchored
        assert_eq!(
            reach(&rule(&[(false, &[("title", "Picture")])]), &pip),
            Reach::Reaches
        );
        assert_eq!(
            reach(
                &rule(&[
                    (false, &[("app-id", "^firefox$")]),
                    (true, &[("title", "^Picture")])
                ]),
                &pip
            ),
            Reach::Misses(Some("an exclude line leaves this window out".into()))
        );
        assert_eq!(
            reach(
                &rule(&[(
                    false,
                    &[("app-id", "^firefox$"), ("title", "Mozilla Firefox$")]
                )]),
                &pip
            ),
            Reach::Misses(Some("the title has to match “Mozilla Firefox$”".into()))
        );
        assert_eq!(
            reach(&rule(&[(false, &[("at-startup", "true")])]), &pip),
            Reach::Unknown
        );
    }

    #[test]
    fn names_read_like_words() {
        let mut calculator = rule(&[(false, &[("app-id", r"^org\.gnome\.Calculator$")])]);
        assert_eq!(name(&calculator), "org.gnome.Calculator: open floating");
        calculator.matchers.clear();
        calculator.properties = vec!["geometry-corner-radius 8".into()];
        assert_eq!(name(&calculator), "Every window: geometry-corner-radius 8");
    }
}
