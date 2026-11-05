//! The forms that put a binding or a window rule together, and following
//! an entry the config moved.

use std::collections::BTreeSet;

use crate::bindings::action_body;
use crate::rules::When;
use crate::store::{
    BindingRow, NewWindowRule, RuleProperty, Store, StoreError, TextMatch, WindowRuleRow,
};

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
        let text = |text: &str| Some(text.trim().to_string()).filter(|trimmed| !trimmed.is_empty());
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

/// What keeps a binding's form from going into the config.
#[derive(Debug, thiserror::Error)]
pub enum BindingFormError {
    #[error("Enter a key combo, for example Mod+T.")]
    NoKey,
    #[error("Choose an action.")]
    NoAction,
    #[error("{key} is already bound to {action} in this file. Edit that binding instead.")]
    KeyTaken { key: String, action: String },
    #[error(transparent)]
    Store(#[from] StoreError),
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
    pub fn apply(
        &self,
        store: &mut Store,
        rows: &[BindingRow],
    ) -> Result<String, BindingFormError> {
        let key = self.key.trim();
        if key.is_empty() {
            return Err(BindingFormError::NoKey);
        }
        let Some(action) = &self.action else {
            return Err(BindingFormError::NoAction);
        };
        if let Some(taken) = store
            .binding_in_the_way(key, self.editing)
            .and_then(|index| rows.get(index))
        {
            return Err(BindingFormError::KeyTaken {
                key: key.to_string(),
                action: taken.action.clone(),
            });
        }
        let body = action_body(action, &self.args);
        if let Some(index) = self.editing {
            store.edit_binding(index, key, &body)?;
            return Ok("Binding updated.".to_string());
        }
        let at = store.add_binding(key, &body)?;
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

/// What keeps a rule's form from going into the config.
#[derive(Debug, thiserror::Error)]
pub enum RuleDraftError {
    #[error("Say which windows: an app ID, a title or a state. Or pick a window.")]
    NoWindows,
    #[error("Choose at least one thing the rule does.")]
    NoProperties,
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl RuleDraft {
    /// Add the rule, or change the one being edited, in `store`: where it
    /// is now and what happened, in words, or what is missing.
    pub fn apply(&self, store: &mut Store) -> Result<(usize, &'static str), RuleDraftError> {
        let rule = self.rule();
        if rule.app_id.is_none() && rule.title.is_none() && rule.flags.is_empty() {
            return Err(RuleDraftError::NoWindows);
        }
        if rule.properties.is_empty() {
            return Err(RuleDraftError::NoProperties);
        }
        let (written, done) = match self.editing {
            Some(index) => (
                store.replace_window_rule(index, &rule.to_kdl()),
                "Rule changed.",
            ),
            None => (store.add_window_rule(&rule), "Rule added."),
        };
        Ok((written?, done))
    }
}

/// Why `name` cannot be an environment variable's name, if it cannot.
pub fn environment_name_problem(name: &str) -> Option<&'static str> {
    let valid = !name.is_empty()
        && !name.starts_with(|character: char| character.is_ascii_digit())
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_');
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn store(text: &str) -> Store {
        Store::read(PathBuf::from("/niri/config.kdl"), |_| Ok(text.to_string())).unwrap()
    }

    #[test]
    fn a_binding_form_says_what_is_missing_before_it_touches_the_config() {
        let mut config = store("binds {\n    Mod+T { spawn \"kitty\"; }\n}\n");
        let rows = config.binding_rows();
        let before = config.document().to_string();
        let mut form = BindingForm::default();
        assert_eq!(
            form.apply(&mut config, &rows).unwrap_err().to_string(),
            "Enter a key combo, for example Mod+T."
        );
        form.key = "Mod+Q".into();
        assert_eq!(
            form.apply(&mut config, &rows).unwrap_err().to_string(),
            "Choose an action."
        );
        form.key = "Mod+T".into();
        form.action = Some("close-window".into());
        assert_eq!(
            form.apply(&mut config, &rows).unwrap_err().to_string(),
            format!(
                "Mod+T is already bound to {} in this file. Edit that binding instead.",
                rows[0].action
            )
        );
        assert_eq!(config.document().to_string(), before);
    }

    #[test]
    fn a_binding_that_is_gone_is_the_stores_to_explain() {
        let mut config = store("binds {\n    Mod+T { spawn \"kitty\"; }\n}\n");
        let rows = config.binding_rows();
        let form = BindingForm {
            key: "Mod+Q".into(),
            action: Some("close-window".into()),
            editing: Some(9),
            ..BindingForm::default()
        };
        assert_eq!(
            form.apply(&mut config, &rows).unwrap_err().to_string(),
            "entry at index 9 not found"
        );
    }

    #[test]
    fn a_rule_draft_says_what_is_missing_before_it_touches_the_config() {
        let mut config = store("layout {\n}\n");
        let before = config.document().to_string();
        let mut draft = RuleDraft::default();
        assert_eq!(
            draft.apply(&mut config).unwrap_err().to_string(),
            "Say which windows: an app ID, a title or a state. Or pick a window."
        );
        draft.app = "firefox".into();
        assert_eq!(
            draft.apply(&mut config).unwrap_err().to_string(),
            "Choose at least one thing the rule does."
        );
        assert_eq!(config.document().to_string(), before);
    }

    #[test]
    fn a_rule_that_is_gone_is_the_stores_to_explain() {
        let mut config = store("layout {\n}\n");
        let draft = RuleDraft {
            app: "firefox".into(),
            properties: BTreeSet::from([RuleProperty::OpenFloating]),
            editing: Some(9),
            ..RuleDraft::default()
        };
        assert_eq!(
            draft.apply(&mut config).unwrap_err().to_string(),
            "entry at index 9 not found"
        );
    }
}
