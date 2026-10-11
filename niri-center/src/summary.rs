//! What a change did, one line per thing in the pages' words, and the
//! names and times the History page gives changes.

use std::path::Path;

use chrono::{DateTime, Local, NaiveDate};

use crate::history::{Entry, Source};
use crate::rules;
use crate::settings::{
    Kind, OnOff, OutputSetting, REGISTRY, Reading, Spec, output_chain, read, size_label, sizes,
};
use crate::store::{Store, WindowRuleRow};
use crate::words::shortened;

/// What changed between two versions of the config, one line each, in the
/// words the pages use.
pub fn describe(before: &Store, after: &Store) -> Vec<String> {
    let mut lines = Vec::new();
    for spec in REGISTRY {
        let (old, new) = (spec.read(before), spec.read(after));
        if old != new {
            lines.push(format!(
                "{} {} → {}",
                name(spec),
                shown(spec, &old),
                shown(spec, &new)
            ));
        }
    }
    lines.extend(output_changes(before, after));
    lines.extend(binding_changes(before, after));
    let (old_env, new_env) = (before.environment(), after.environment());
    let shown = |value: &Option<String>| value.clone().unwrap_or_else(|| "taken away".to_string());
    for (name, value) in &new_env {
        match old_env.iter().find(|(old, _)| old == name) {
            None => lines.push(format!("Sets {name} to {}", shown(value))),
            Some((_, old)) if old != value => {
                lines.push(format!("{name} {} → {}", shown(old), shown(value)));
            }
            Some(_) => {}
        }
    }
    for (name, _) in old_env
        .iter()
        .filter(|(old, _)| !new_env.iter().any(|(new, _)| new == old))
    {
        lines.push(format!("No longer sets {name}"));
    }
    // a rule is its text: an edit above it moves it, which is no change
    let text = |rule: &WindowRuleRow| {
        (
            rule.criteria.clone(),
            rule.properties.clone(),
            rule.disabled,
        )
    };
    let (old_rules, new_rules) = (before.window_rule_rows(), after.window_rule_rows());
    let (old_texts, new_texts): (Vec<_>, Vec<_>) = (
        old_rules.iter().map(text).collect(),
        new_rules.iter().map(text).collect(),
    );
    if old_rules.len() == new_rules.len() {
        // as many rules as before: one changed in its place
        for (old, new) in old_rules.iter().zip(&new_rules) {
            if text(old) == text(new) {
                continue;
            }
            let what = match (old.disabled, new.disabled) {
                (false, true) => "Turned off rule",
                (true, false) => "Turned on rule",
                _ => "Changed rule",
            };
            lines.push(format!("{what}: {}", rules::name(new)));
        }
    } else {
        for rule in new_rules
            .iter()
            .filter(|rule| !old_texts.contains(&text(rule)))
        {
            lines.push(format!("Rule: {}", rules::name(rule)));
        }
        for rule in old_rules
            .iter()
            .filter(|rule| !new_texts.contains(&text(rule)))
        {
            lines.push(format!("Removed rule: {}", rules::name(rule)));
        }
    }
    let (old_starts, new_starts) = (before.autostart_rows(), after.autostart_rows());
    for program in new_starts.iter().filter(|row| !old_starts.contains(row)) {
        lines.push(format!("Starts {}", program.display));
    }
    for program in old_starts.iter().filter(|row| !new_starts.contains(row)) {
        lines.push(format!("No longer starts {}", program.display));
    }
    if lines.is_empty() {
        let files = before
            .saved_texts()
            .zip(after.saved_texts())
            .filter(|((_, old), (_, new))| old != new)
            .map(|((path, _), _)| after.file_label(path))
            .collect::<Vec<_>>();
        lines.push(if files.is_empty() {
            "Edited the config".to_string()
        } else {
            format!("Edited {}", files.join(", "))
        });
    }
    lines
}

/// A setting's name in a summary: its label, with the section in front when
/// the label alone is ambiguous ("Width" is in several sections).
fn name(spec: &Spec) -> String {
    let shared = REGISTRY
        .iter()
        .filter(|other| other.label == spec.label)
        .count()
        > 1;
    if shared {
        let mut label = spec.label.chars();
        let lower = label
            .next()
            .map(|first| first.to_lowercase().chain(label).collect::<String>())
            .unwrap_or_default();
        format!("{} {lower}", spec.section)
    } else {
        spec.label.to_string()
    }
}

/// A value the way the setting's control shows it; niri's default when
/// the config leaves it out.
pub fn shown(spec: &Spec, reading: &Reading) -> String {
    let value = match reading {
        Reading::Switch(true) | Reading::OnOff(OnOff::On) => "on".to_string(),
        Reading::Switch(false) | Reading::OnOff(OnOff::Off) => "off".to_string(),
        Reading::OnOff(OnOff::Default) | Reading::Value(None) if spec.default.is_empty() => {
            "default".to_string()
        }
        Reading::OnOff(OnOff::Default) => spec.default.to_string(),
        Reading::Value(None) => value_text(spec.kind, spec.default),
        Reading::Value(Some(value)) => value_text(spec.kind, value),
    };
    shortened(value, 40)
}

fn value_text(kind: Kind, value: &str) -> String {
    if let Some(word) = kind.word(value) {
        return word.to_string();
    }
    match kind {
        Kind::Sizes { .. } => sizes(value)
            .iter()
            .map(|(name, size)| size_label(name, size))
            .collect::<Vec<_>>()
            .join(", "),
        Kind::Choice(_)
        | Kind::Switch
        | Kind::Enabled
        | Kind::OnOff
        | Kind::Bool
        | Kind::Integer { .. }
        | Kind::Float { .. }
        | Kind::Color
        | Kind::Gradient
        | Kind::Text { .. }
        | Kind::Prop { .. }
        | Kind::Layouts
        | Kind::XkbOptions
        | Kind::CursorTheme
        | Kind::Output
        | Kind::File { .. }
        | Kind::Screenshot => value.to_string(),
    }
}

fn output_changes(before: &Store, after: &Store) -> Vec<String> {
    let mut names = before.block_names("output");
    for name in after.block_names("output") {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    let mut lines = Vec::new();
    for name in names {
        let chain = output_chain(&name);
        for &setting in OutputSetting::ALL {
            let spec = setting.spec();
            let read = |store: &Store| read(store, &chain, spec.child, spec.kind);
            let (old, new) = (read(before), read(after));
            if old == new {
                continue;
            }
            lines.push(if setting == OutputSetting::Off {
                format!(
                    "{name} {}",
                    if new == Reading::Switch(true) {
                        "turned off"
                    } else {
                        "turned on"
                    }
                )
            } else {
                format!(
                    "{name} {} {} → {}",
                    spec.label.to_lowercase(),
                    shown(spec, &old),
                    shown(spec, &new)
                )
            });
        }
        let position = |store: &Store| {
            store.setting_node(&chain, "position").map(|node| {
                node.props
                    .iter()
                    .map(|prop| prop.value.kind.as_str().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
        };
        let (old, new) = (position(before), position(after));
        if old != new {
            lines.push(format!(
                "{name} moved to {}",
                new.unwrap_or_else(|| "where niri puts it".to_string())
            ));
        }
    }
    lines
}

fn binding_changes(before: &Store, after: &Store) -> Vec<String> {
    let (old, new) = (before.binding_rows(), after.binding_rows());
    let mut lines = Vec::new();
    for binding in &new {
        match old.iter().find(|o| o.key == binding.key) {
            None => lines.push(format!("New binding {}: {}", binding.key, binding.action)),
            Some(o) if o.disabled != binding.disabled => lines.push(format!(
                "{} turned {}",
                binding.key,
                if binding.disabled { "off" } else { "on" }
            )),
            Some(o) if o.action != binding.action => lines.push(format!(
                "{}: {} → {}",
                binding.key, o.action, binding.action
            )),
            Some(_) => {}
        }
    }
    for binding in old.iter().filter(|o| !new.iter().any(|n| n.key == o.key)) {
        lines.push(format!("Removed binding {}", binding.key));
    }
    lines
}

// ---- the history's words ----

fn local(time: u128) -> Option<DateTime<Local>> {
    let millis = i64::try_from(time).ok()?;
    Some(DateTime::from_timestamp_millis(millis)?.with_timezone(&Local))
}

/// The day it is: the history names days by it ("Today", "Yesterday").
/// Read once for a list, not once per change in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Today(NaiveDate);

pub fn today() -> Today {
    Today(Local::now().date_naive())
}

/// The day a change was made, as a heading names it.
enum Day {
    Today,
    Yesterday,
    /// Before yesterday: "Monday 6 October".
    Date(String),
}

fn day_of(time: u128, Today(today): Today) -> Option<Day> {
    let when = local(time)?;
    Some(match (today - when.date_naive()).num_days() {
        0 => Day::Today,
        1 => Day::Yesterday,
        _ => Day::Date(when.format("%A %-d %B").to_string()),
    })
}

/// "Today", "Yesterday" or the date.
pub fn day(time: u128, today: Today) -> String {
    match day_of(time, today) {
        None => String::new(),
        Some(Day::Today) => "Today".to_string(),
        Some(Day::Yesterday) => "Yesterday".to_string(),
        Some(Day::Date(date)) => date,
    }
}

/// A moment given in nanoseconds since the Unix epoch, as a date and a
/// time: "2026-10-11 21:08". Backups are named by such a moment.
pub fn stamp(nanos: u128) -> Option<String> {
    let nanos = i64::try_from(nanos).ok()?;
    Some(
        DateTime::from_timestamp_nanos(nanos)
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
    )
}

/// The time of day: "21:08".
pub fn clock(time: u128) -> String {
    local(time).map_or_else(String::new, |when| when.format("%H:%M").to_string())
}

/// A time in a sentence: "21:08" today, "yesterday at 21:08" or the date
/// before that.
pub fn moment(time: u128, today: Today) -> String {
    match day_of(time, today) {
        Some(Day::Today) => clock(time),
        Some(Day::Yesterday) => format!("yesterday at {}", clock(time)),
        Some(Day::Date(date)) => format!("{date} at {}", clock(time)),
        None => String::new(),
    }
}

/// An entry's name in lists and headings.
pub fn title(entry: &Entry) -> String {
    match entry.source {
        Source::Outside => "Edited in another program".to_string(),
        // a restore is named by what it put back
        Source::Restored => entry.summary.first().cloned().unwrap_or_default(),
        Source::App => {
            let first = entry.summary.first().cloned().unwrap_or_default();
            match entry.summary.len() {
                0 | 1 => first,
                n => format!("{first} and {} more", n - 1),
            }
        }
    }
}

/// What an entry changed, one line each: a restore's name line left out.
pub fn details(entry: &Entry) -> &[String] {
    match entry.source {
        Source::Restored => entry.summary.get(1..).unwrap_or_default(),
        Source::App | Source::Outside => &entry.summary,
    }
}

/// An entry's name where room is short: the header.
pub fn short_title(entry: &Entry) -> String {
    shortened(title(entry), 48)
}

/// The files an entry touched, named by `label`, and how many things
/// changed.
pub fn files_line(entry: &Entry, label: impl Fn(&Path) -> String) -> String {
    let names: Vec<String> = entry.paths.iter().map(|path| label(path)).collect();
    match details(entry).len() {
        0 | 1 => names.join(", "),
        n => format!("{}, {n} changes", names.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_as_fractions() {
        let spec = crate::settings::Setting::LayoutPresetColumnWidths.spec();
        assert_eq!(
            shown(
                spec,
                &Reading::Value(Some("proportion 0.33333; proportion 0.5; fixed 800".into()))
            ),
            "1/3, 1/2, 800 px"
        );
    }
}
