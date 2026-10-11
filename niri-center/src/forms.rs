//! The forms that put a binding or a window rule together, and following
//! an entry the config moved.

use std::collections::BTreeSet;

use crate::bindings::action_body;
use crate::rules::When;
use crate::store::{BindingRow, NewWindowRule, RuleProperty, Store, TextMatch, WindowRuleRow};

/// The rule being put together on the Rules page: which windows (app,
/// title, a state), and what happens to them.
#[derive(Debug, Clone, Default)]
pub struct RuleDraft {
    pub app: String,
    pub app_how: TextMatch,
    pub title: String,
    pub title_how: TextMatch,
    /// Only while the window is in this state.
    pub when: Option<When>,
    pub properties: BTreeSet<RuleProperty>,
    /// The config's rule this draft edits; a new rule when `None`.
    pub editing: Option<usize>,
}

impl RuleDraft {
    /// An existing rule in the form, when the form can say all of it:
    /// what core reads back as a rule it could have written
    /// ([`NewWindowRule::from_row`]), with at most one state. Anything
    /// more stays text, so editing in the form never drops a line.
    pub fn from_rule(index: usize, rule: &WindowRuleRow) -> Option<Self> {
        let written = NewWindowRule::from_row(rule)?;
        let when = match written.flags.as_slice() {
            [] => None,
            [(name, yes)] => Some(When::from_prop(name, *yes)?),
            _ => return None,
        };
        Some(Self {
            app: written.app_id.unwrap_or_default(),
            app_how: written.app_id_how,
            title: written.title.unwrap_or_default(),
            title_how: written.title_how,
            when,
            properties: written.properties.into_iter().collect(),
            editing: Some(index),
        })
    }

    pub fn rule(&self) -> NewWindowRule {
        let text = |text: &str| Some(text.trim().to_string()).filter(|t| !t.is_empty());
        NewWindowRule {
            app_id: text(&self.app),
            app_id_how: self.app_how,
            title: text(&self.title),
            title_how: self.title_how,
            flags: self.when.map(|when| vec![when.prop()]).unwrap_or_default(),
            properties: self.properties.iter().copied().collect(),
        }
    }
}

/// The form for a binding: a new one, or one of the config's.
#[derive(Debug, Clone, Default)]
pub struct BindingForm {
    pub key: String,
    pub action: Option<String>,
    pub args: String,
    /// The binding it changes, by index; a new binding when `None`.
    pub editing: Option<usize>,
}

impl BindingForm {
    /// Add the binding, or change the one being edited, in `store`, whose
    /// bindings are `rows`. What happened, in words, or what is missing.
    pub fn apply(&self, store: &mut Store, rows: &[BindingRow]) -> Result<String, String> {
        let key = self.key.trim();
        if key.is_empty() {
            return Err("Enter a key combo, for example Mod+T.".to_string());
        }
        let Some(action) = &self.action else {
            return Err("Choose an action.".to_string());
        };
        if let Some(taken) = store
            .binding_in_the_way(key, self.editing)
            .and_then(|index| rows.get(index))
        {
            return Err(format!(
                "{key} is already bound to {} in this file. Edit that binding instead.",
                taken.action
            ));
        }
        let body = action_body(action, &self.args);
        if let Some(index) = self.editing {
            store
                .edit_binding(index, key, &body)
                .map_err(|e| e.to_string())?;
            return Ok("Binding updated.".to_string());
        }
        let at = store.add_binding(key, &body).map_err(|e| e.to_string())?;
        // the same key in another file: whichever niri reads last wins
        let rows = store.binding_rows();
        let added = rows.get(at);
        Ok(
            match (
                added.and_then(|row| row.hidden_by.as_ref()),
                added.and_then(|row| row.replaces.first()),
            ) {
                (Some(later), _) => format!(
                    "Added {key}. {later} binds it too and is read later, so niri uses that one."
                ),
                (None, Some(earlier)) => format!("Added {key}. It overrides the one in {earlier}."),
                (None, None) => format!("Added {key}."),
            },
        )
    }
}

impl RuleDraft {
    /// Add the rule, or change the one being edited, in `store`: where it
    /// is now and what happened, in words, or what is missing.
    pub fn apply(&self, store: &mut Store) -> Result<(usize, &'static str), String> {
        let rule = self.rule();
        if rule.app_id.is_none() && rule.title.is_none() && rule.flags.is_empty() {
            return Err(
                "Say which windows: an app ID, a title or a state. Or pick a window.".to_string(),
            );
        }
        if rule.properties.is_empty() {
            return Err("Choose at least one thing the rule does.".to_string());
        }
        let (written, done) = match self.editing {
            Some(index) => (
                store.replace_window_rule(index, &rule.to_kdl()),
                "Rule changed.",
            ),
            None => (store.add_window_rule(&rule), "Rule added."),
        };
        written.map(|at| (at, done)).map_err(|e| e.to_string())
    }
}

/// Why `name` cannot be an environment variable's name, if it cannot.
pub fn environment_name_problem(name: &str) -> Option<&'static str> {
    let valid = !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    (!valid).then_some(
        "A variable name is letters, digits and underscores, and does not start with a digit.",
    )
}

/// Where the item at `index` of `before` is in `after`: in its place when
/// it is still there, else the nearest one that reads the same (an Undo
/// or an edit elsewhere moved it), else in its place when nothing was
/// added or taken away (it was itself changed). `None`: it is gone.
pub fn follow<T>(
    before: &[T],
    after: &[T],
    index: usize,
    same: impl Fn(&T, &T) -> bool,
) -> Option<usize> {
    let old = before.get(index)?;
    if after.get(index).is_some_and(|new| same(old, new)) {
        return Some(index);
    }
    after
        .iter()
        .enumerate()
        .filter(|(_, new)| same(old, new))
        .min_by_key(|(at, _)| at.abs_diff(index))
        .map(|(at, _)| at)
        .or_else(|| (before.len() == after.len()).then_some(index))
}
