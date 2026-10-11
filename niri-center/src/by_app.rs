//! The window rules and the open windows, gathered by the app they are
//! for: the Window rules page's index.
//!
//! An app is known by its name, so the IDs one app goes by across
//! packagings (`thunderbird`, `org.mozilla.Thunderbird`) are one entry. A
//! rule is listed under every app it names; a pattern that is not one app
//! ID is listed under the apps of the open windows it reaches, and under
//! its own text while it reaches none. Rules that pick no app are listed
//! apart: those for every window, and those by title or state alone.

use std::collections::{BTreeMap, BTreeSet};

use crate::library::KnownApp;
use crate::niri::OpenWindow;
use crate::rules::{Reach, name, reach};
use crate::store::{MatchName, MatchProp, TextMatch, WindowRuleRow};
use crate::system::SystemData;
use crate::words::count;

/// What an entry of the index is for.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Who {
    /// Rules with no `match` line: every window, but what they exclude.
    Every,
    /// One app, by its name in lower case.
    App(String),
    /// Rules that pick windows by title or state, not by app.
    Others,
}

/// One entry: an app, with its open windows and the rules for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub who: Who,
    /// What the list shows: the app's name, or what the rules are for.
    pub name: String,
    /// The app IDs seen for it, on its windows and in its rules.
    pub app_ids: BTreeSet<String>,
    /// Its open windows, by index into the windows given.
    pub windows: Vec<usize>,
    /// Its rules, by index, in the order niri applies them.
    pub rules: Vec<usize>,
}

/// Every window rule and open window, by app: rules for every window
/// first, then the apps by name, then rules by title or state.
pub fn index(rules: &[WindowRuleRow], windows: &[OpenWindow], system: &SystemData) -> Vec<Entry> {
    let mut apps: BTreeMap<String, Entry> = BTreeMap::new();
    let mut every = entry(Who::Every, "Every window");
    let mut others = entry(Who::Others, "By title or state");
    let add = |entry: &mut Entry, index: usize| {
        if !entry.rules.contains(&index) {
            entry.rules.push(index);
        }
    };
    for (index, window) in windows.iter().enumerate() {
        let found = app(&mut apps, app_name(system, &window.app_id));
        found.windows.push(index);
        found.app_ids.insert(window.app_id.clone());
    }
    for (index, rule) in rules.iter().enumerate() {
        if rule.matchers.iter().all(|matcher| matcher.exclude) {
            every.rules.push(index);
        }
        for matcher in rule.matchers.iter().filter(|matcher| !matcher.exclude) {
            // a line without an app ID picks by title or state
            let Some(prop) = matcher.props.iter().find(|p| p.name == MatchName::AppId) else {
                add(&mut others, index);
                continue;
            };
            for (name, app_id) in named(prop, windows, system) {
                let found = app(&mut apps, name);
                add(found, index);
                found.app_ids.extend(app_id);
            }
        }
    }
    let mut index: Vec<Entry> = Vec::new();
    if !every.rules.is_empty() {
        index.push(every);
    }
    index.extend(apps.into_values());
    if !others.rules.is_empty() {
        index.push(others);
    }
    index
}

/// The entry for the app called `name`, made when there is none yet.
fn app(apps: &mut BTreeMap<String, Entry>, name: String) -> &mut Entry {
    let key = name.to_lowercase();
    apps.entry(key.clone())
        .or_insert_with(|| entry(Who::App(key), &name))
}

/// The entry an open window's app has: apps are known by their name.
pub fn who(system: &SystemData, app_id: &str) -> Who {
    Who::App(app_name(system, app_id).to_lowercase())
}

/// The entry a rule shows under: `shown` when the rule is one of its,
/// else the first that has it.
pub fn card<'a>(index: &'a [Entry], shown: Option<&Who>, rule: usize) -> Option<&'a Entry> {
    let has = |entry: &&Entry| entry.rules.contains(&rule);
    index
        .iter()
        .filter(has)
        .find(|entry| shown == Some(&entry.who))
        .or_else(|| index.iter().find(has))
}

impl Entry {
    /// How many rules it has, how many of them are off, and its open
    /// windows, in words: `2 rules, 1 off, 1 open window`.
    pub fn counts(&self, rules: &[WindowRuleRow]) -> String {
        let off = self
            .rules
            .iter()
            .filter(|&&at| rules.get(at).is_some_and(|rule| rule.disabled))
            .count();
        let mut counts = Vec::new();
        if !self.rules.is_empty() {
            counts.push(count(self.rules.len(), "rule", "rules"));
        }
        if off > 0 {
            counts.push(format!("{off} off"));
        }
        if !self.windows.is_empty() {
            counts.push(count(self.windows.len(), "open window", "open windows"));
        }
        counts.join(", ")
    }

    /// Its open windows, those alike (five terminals titled "Alacritty",
    /// all tiled) as one, with how many there are.
    pub fn alike_windows<'a>(&self, windows: &'a [OpenWindow]) -> Vec<(&'a OpenWindow, usize)> {
        let mut alike: Vec<(&OpenWindow, usize)> = Vec::new();
        for window in self.windows.iter().filter_map(|&at| windows.get(at)) {
            match alike
                .iter_mut()
                .find(|(seen, _)| seen.title == window.title && seen.floating == window.floating)
            {
                Some((_, count)) => *count += 1,
                None => alike.push((window, 1)),
            }
        }
        alike
    }

    /// What the config's other rules do to its open windows: those that
    /// apply, and a sentence for each that nearly does or may.
    pub fn elsewhere(&self, rules: &[WindowRuleRow], windows: &[OpenWindow]) -> Elsewhere {
        let mine: Vec<&OpenWindow> = self
            .windows
            .iter()
            .filter_map(|&at| windows.get(at))
            .collect();
        let mut elsewhere = Elsewhere::default();
        if mine.is_empty() {
            return elsewhere;
        }
        for (at, rule) in rules.iter().enumerate() {
            if self.rules.contains(&at) {
                continue;
            }
            let reaches: Vec<Reach> = mine.iter().map(|window| reach(rule, window)).collect();
            let near = reaches.iter().find_map(|reach| match reach {
                Reach::Misses(Some(why)) => Some(format!("does not apply: {why}")),
                Reach::Misses(None) | Reach::Reaches | Reach::Unknown => None,
            });
            let note = if reaches.contains(&Reach::Reaches) {
                elsewhere.applies.push(at);
                continue;
            } else if let Some(near) = near {
                near
            } else if reaches.contains(&Reach::Unknown) {
                "may apply: it depends on more than the app ID and title".to_string()
            } else {
                continue;
            };
            elsewhere.notes.push(format!(
                "{} ({}:{}) {note}.",
                name(rule),
                rule.file,
                rule.line
            ));
        }
        elsewhere
    }
}

/// What the config's other rules do to an app's open windows.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Elsewhere {
    /// The rules that apply, by index.
    pub applies: Vec<usize>,
    /// A sentence for each rule that nearly applies, or may.
    pub notes: Vec<String>,
}

fn entry(who: Who, name: &str) -> Entry {
    Entry {
        who,
        name: name.to_string(),
        app_ids: BTreeSet::new(),
        windows: Vec::new(),
        rules: Vec::new(),
    }
}

/// The apps an `app-id` property names, each with the app ID it says when
/// it says one.
fn named(
    prop: &MatchProp,
    windows: &[OpenWindow],
    system: &SystemData,
) -> Vec<(String, Option<String>)> {
    let (how, text) = TextMatch::read(&prop.value);
    if how == TextMatch::Exact {
        return vec![(app_name(system, &text), Some(text))];
    }
    let mut names: Vec<(String, Option<String>)> = Vec::new();
    for window in windows {
        if prop
            .pattern
            .as_ref()
            .is_some_and(|pattern| pattern.is_match(&window.app_id))
        {
            let name = app_name(system, &window.app_id);
            if !names
                .iter()
                .any(|(known, _)| known.eq_ignore_ascii_case(&name))
            {
                names.push((name, None));
            }
        }
    }
    if names.is_empty() {
        names.push(match how {
            TextMatch::Contains => (app_name(system, &text), Some(text)),
            TextMatch::Exact | TextMatch::Pattern => (
                one_app(prop, system).unwrap_or_else(|| prop.value.clone()),
                None,
            ),
        });
    }
    names
}

/// The one app a pattern matches among the installed apps and the
/// library's: `firefox$` is Firefox. A pattern for several apps (`com\.system76\.`)
/// is a family of its own, and keeps its words.
fn one_app(prop: &MatchProp, system: &SystemData) -> Option<String> {
    let pattern = prop.pattern.as_ref()?;
    let known = KnownApp::ALL
        .into_iter()
        .flat_map(|app| app.ids().iter().copied());
    let ids: Vec<&str> = system
        .apps
        .iter()
        .map(|app| app.id.as_str())
        .chain(known)
        .collect();
    // the first name, unless a second one turns up
    let mut names = ids
        .into_iter()
        .filter(|id| pattern.is_match(id))
        .map(|id| app_name(system, id));
    let first = names.next()?;
    names
        .all(|name| name.eq_ignore_ascii_case(&first))
        .then_some(first)
}

/// What an app ID is called: its desktop file's name, the library's name
/// for it, the name of a desktop file whose ID ends the same
/// (`thunderbird` and `org.mozilla.Thunderbird`), else the ID itself.
pub fn app_name(system: &SystemData, app_id: &str) -> String {
    if let Some(name) = system.app_name(app_id) {
        return name.to_string();
    }
    if let Some(app) = KnownApp::ALL.into_iter().find(|app| app.owns(app_id)) {
        return app.name().to_string();
    }
    let end = last_part(app_id);
    if !end.is_empty()
        && let Some(app) = system
            .apps
            .iter()
            .find(|app| last_part(&app.id).eq_ignore_ascii_case(end))
    {
        return app.name.clone();
    }
    app_id.to_string()
}

/// An app ID's last dot-separated part: `Thunderbird` of
/// `org.mozilla.Thunderbird`.
fn last_part(app_id: &str) -> &str {
    app_id.rsplit('.').next().unwrap_or(app_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Matcher;
    use crate::system::InstalledApp;

    fn window(app_id: &str) -> OpenWindow {
        OpenWindow {
            id: 0,
            app_id: app_id.into(),
            title: String::new(),
            floating: false,
            focused: false,
        }
    }

    /// A rule of `match` lines (`exclude` ones start with `!`), each of
    /// (name, value) pairs.
    fn rule(lines: &[&[(&str, &str)]]) -> WindowRuleRow {
        WindowRuleRow {
            criteria: Vec::new(),
            matchers: lines
                .iter()
                .map(|props| Matcher {
                    exclude: props.first().is_some_and(|(name, _)| name.starts_with('!')),
                    props: props
                        .iter()
                        .map(|(name, value)| {
                            MatchProp::new(MatchName::read(name.trim_start_matches('!')), *value)
                        })
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

    fn system(apps: &[(&str, &str)]) -> SystemData {
        SystemData {
            apps: apps
                .iter()
                .map(|(id, name)| InstalledApp {
                    id: (*id).into(),
                    name: (*name).into(),
                    command: String::new(),
                })
                .collect(),
            ..SystemData::default()
        }
    }

    fn names(index: &[Entry]) -> Vec<&str> {
        index.iter().map(|entry| entry.name.as_str()).collect()
    }

    #[test]
    fn rules_and_windows_are_listed_by_app_name_every_window_first() {
        let rules = [
            rule(&[&[("app-id", "^org\\.wezfurlong\\.wezterm$")]]),
            rule(&[]),
            rule(&[&[("app-id", "^dev\\.zed\\.Zed$")]]),
            rule(&[&[("title", "popup")]]),
            rule(&[&[("app-id", "^org\\.wezfurlong\\.wezterm$")]]),
        ];
        let windows = [window("dev.zed.Zed"), window("signal")];
        let index = index(
            &rules,
            &windows,
            &system(&[("dev.zed.Zed", "Zed"), ("signal", "Signal")]),
        );
        assert_eq!(
            names(&index),
            [
                "Every window",
                "Signal",
                "WezTerm",
                "Zed",
                "By title or state"
            ]
        );
        // in the order niri applies them
        assert_eq!(index[2].rules, [0, 4]);
        assert_eq!(index[3].windows, [0]);
        assert_eq!(index[3].rules, [2]);
        assert!(
            index[1].rules.is_empty(),
            "an app with windows and no rules"
        );
    }

    #[test]
    fn the_ids_one_app_goes_by_are_one_entry() {
        let rules = [
            rule(&[&[("app-id", "^thunderbird$")]]),
            rule(&[&[("app-id", "^org\\.mozilla\\.Thunderbird$")]]),
        ];
        let index = index(
            &rules,
            &[],
            &system(&[("org.mozilla.Thunderbird", "Thunderbird")]),
        );
        assert_eq!(names(&index), ["Thunderbird"]);
        assert_eq!(index[0].rules, [0, 1]);
        assert_eq!(
            index[0]
                .app_ids
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["org.mozilla.Thunderbird", "thunderbird"]
        );
    }

    #[test]
    fn a_pattern_is_listed_under_the_apps_it_reaches_else_as_written() {
        let rules = [rule(&[&[("app-id", "com\\.system76\\.")]])];
        let windows = [
            window("com.system76.CosmicFiles"),
            window("com.system76.CosmicTerm"),
        ];
        let reaching = index(&rules, &windows, &SystemData::default());
        assert_eq!(
            names(&reaching),
            ["com.system76.CosmicFiles", "com.system76.CosmicTerm"]
        );
        assert!(reaching.iter().all(|entry| entry.rules == [0]));
        let none_open = index(
            &rules,
            &[],
            &system(&[
                ("com.system76.CosmicFiles", "COSMIC Files"),
                ("com.system76.CosmicTerm", "COSMIC Terminal"),
            ]),
        );
        assert_eq!(names(&none_open), ["com.system76."]);
    }

    #[test]
    fn a_pattern_for_one_known_app_is_listed_under_it() {
        let rules = [rule(&[&[
            ("app-id", "firefox$"),
            ("title", "^Picture-in-Picture$"),
        ]])];
        let index = index(&rules, &[], &SystemData::default());
        assert_eq!(names(&index), ["Firefox"]);
    }

    #[test]
    fn a_rule_naming_two_apps_is_under_both() {
        let rules = [rule(&[
            &[("app-id", "^firefox$")],
            &[("app-id", "^gimp$")],
            &[("!app-id", "^gimp$"), ("title", "Export")],
        ])];
        let index = index(&rules, &[], &SystemData::default());
        assert_eq!(names(&index), ["Firefox", "GIMP"]);
    }

    #[test]
    fn a_rule_with_only_exclude_lines_is_for_every_window() {
        let rules = [rule(&[&[("!app-id", "^firefox$")]])];
        let index = index(&rules, &[], &SystemData::default());
        assert_eq!(names(&index), ["Every window"]);
    }

    #[test]
    fn an_entry_counts_its_rules_and_windows_in_words() {
        let mut off = rule(&[&[("app-id", "^kitty$")]]);
        off.disabled = true;
        let rules = [rule(&[&[("app-id", "^kitty$")]]), off];
        let windows = [window("kitty"), window("kitty")];
        let index = index(&rules, &windows, &SystemData::default());
        assert_eq!(index[0].counts(&rules), "2 rules, 1 off, 2 open windows");
        // the two untitled tiled terminals are one line
        assert_eq!(index[0].alike_windows(&windows).len(), 1);
        assert_eq!(index[0].alike_windows(&windows)[0].1, 2);
    }

    #[test]
    fn a_window_finds_its_entry_and_a_rule_the_one_shown() {
        let rules = [rule(&[&[("app-id", "^kitty$")], &[("app-id", "^foot$")]])];
        let index = index(&rules, &[], &SystemData::default());
        let foot = who(&SystemData::default(), "foot");
        assert_eq!(
            card(&index, None, 0).map(|entry| &entry.name),
            Some(&"foot".to_string())
        );
        assert_eq!(
            card(&index, Some(&foot), 0).map(|entry| &entry.who),
            Some(&foot)
        );
        let kitty = who(&SystemData::default(), "kitty");
        assert_eq!(
            card(&index, Some(&kitty), 0).map(|entry| &entry.who),
            Some(&kitty)
        );
    }

    #[test]
    fn other_rules_that_reach_an_apps_windows_are_named_with_those_that_nearly_do() {
        let rules = [
            rule(&[&[("app-id", "^kitty$")]]),
            rule(&[]),
            rule(&[&[("title", "^htop$"), ("is-floating", "false")]]),
        ];
        let windows = [window("kitty")];
        let index = index(&rules, &windows, &SystemData::default());
        let kitty = index
            .iter()
            .find(|entry| entry.who == who(&SystemData::default(), "kitty"))
            .unwrap();
        let elsewhere = kitty.elsewhere(&rules, &windows);
        assert_eq!(elsewhere.applies, [1], "the rule for every window");
        // tiled as asked, but not titled htop
        assert_eq!(elsewhere.notes.len(), 1, "{:?}", elsewhere.notes);
        assert!(
            elsewhere.notes[0].contains("does not apply: the title has to match"),
            "{:?}",
            elsewhere.notes
        );
    }

    #[test]
    fn an_app_id_is_named_by_its_desktop_file_the_library_or_itself() {
        let system = system(&[("dev.zed.Zed", "Zed")]);
        assert_eq!(app_name(&system, "dev.zed.Zed"), "Zed");
        assert_eq!(app_name(&system, "zed"), "Zed");
        assert_eq!(app_name(&system, "org.mozilla.firefox"), "Firefox");
        assert_eq!(app_name(&system, "niri-center.displays"), "niri-center");
        assert_eq!(app_name(&system, "popup"), "popup");
    }
}
