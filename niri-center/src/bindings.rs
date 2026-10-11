//! Keybinding model: the action catalog, key combos as niri tells them
//! apart, and turning form input into a binding body.

/// A key combo as niri tells combos apart: its modifiers in any order and
/// case, under any of their names (`Control`, `Win`, `Mod5`, `Mod3`), and
/// the key in any case. `Mod+Shift+T` and `shift+mod+t` are one combo.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    /// One bit per [`Modifier`], by its number.
    modifiers: u8,
    key: String,
}

/// A modifier niri reads in a key combo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modifier {
    Mod,
    Ctrl,
    Shift,
    Alt,
    Super,
    IsoLevel3Shift,
    IsoLevel5Shift,
}

impl Modifier {
    pub const ALL: [Self; 7] = [
        Self::Mod,
        Self::Ctrl,
        Self::Shift,
        Self::Alt,
        Self::Super,
        Self::IsoLevel3Shift,
        Self::IsoLevel5Shift,
    ];

    /// The modifiers a key combo is put together from, as chips and keys
    /// on the keyboard picture offer them.
    pub const COMPOSED: [Self; 5] = [Self::Mod, Self::Super, Self::Ctrl, Self::Alt, Self::Shift];

    /// Every name niri reads for it, in lower case.
    const fn names(self) -> &'static [&'static str] {
        match self {
            Self::Mod => &["mod"],
            Self::Ctrl => &["ctrl", "control"],
            Self::Shift => &["shift"],
            Self::Alt => &["alt"],
            Self::Super => &["super", "win"],
            Self::IsoLevel3Shift => &["iso_level3_shift", "mod5"],
            Self::IsoLevel5Shift => &["iso_level5_shift", "mod3"],
        }
    }

    /// As a combo is written: `Mod+Shift+T`.
    pub const fn written(self) -> &'static str {
        match self {
            Self::Mod => "Mod",
            Self::Ctrl => "Ctrl",
            Self::Shift => "Shift",
            Self::Alt => "Alt",
            Self::Super => "Super",
            Self::IsoLevel3Shift => "ISO_Level3_Shift",
            Self::IsoLevel5Shift => "ISO_Level5_Shift",
        }
    }

    /// The modifier `name` is, in any case and under any of its names.
    pub fn read(name: &str) -> Option<Self> {
        let name = name.trim();
        Self::ALL.into_iter().find(|modifier| {
            modifier
                .names()
                .iter()
                .any(|known| known.eq_ignore_ascii_case(name))
        })
    }
}

impl KeyCombo {
    /// The combo `text` names. A part that is no modifier stays part of the
    /// key: niri refuses such a combo, so it matches no other.
    pub fn parse(text: &str) -> Self {
        let mut parts: Vec<&str> = text.split('+').collect();
        let last = parts.pop().unwrap_or_default();
        let mut modifiers = 0;
        let mut unknown = Vec::new();
        for part in parts {
            match Modifier::read(part) {
                Some(modifier) => modifiers |= 1 << modifier as u8,
                None => unknown.push(part.trim()),
            }
        }
        unknown.push(last);
        Self {
            modifiers,
            key: key_id(&unknown.join("+")),
        }
    }

    /// The key the combo ends on, folded as niri reads it: `t` for `Mod+T`.
    pub fn key(&self) -> &str {
        &self.key
    }
}

/// A key's name as niri tells keys apart: in any case.
pub fn key_id(name: &str) -> String {
    name.trim().to_lowercase()
}

/// One niri action, as the linked `niri-ipc` defines it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionInfo {
    /// The name as written in a binding (`focus-workspace`).
    pub name: String,
    /// niri's own one-line description.
    pub about: String,
    pub arguments: Vec<ArgumentInfo>,
}

/// One argument of an [`ActionInfo`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgumentInfo {
    /// As written in the config: kebab-case (`skip-confirmation`).
    pub name: String,
    /// A plain argument (`focus-workspace 2`) rather than a property
    /// (`quit skip-confirmation=true`).
    pub positional: bool,
    /// A property that takes no value on the command line: `name=true`.
    pub switch: bool,
    /// Takes several values (`spawn "a" "b"`).
    pub repeated: bool,
}

impl ActionInfo {
    /// How the arguments are written in a binding, e.g.
    /// `"<command>" …` or `<reference> focus=<bool>`; empty for none.
    pub fn usage(&self) -> String {
        let parts: Vec<String> = self
            .arguments
            .iter()
            .map(
                |argument| match (argument.positional, argument.switch, argument.repeated) {
                    (true, _, true) => format!("\"<{}>\" …", argument.name),
                    (true, _, false) => format!("<{}>", argument.name),
                    (false, true, _) => format!("{}=true", argument.name),
                    (false, false, _) => format!("{}=<value>", argument.name),
                },
            )
            .collect();
        parts.join(" ")
    }
}

/// Every action the linked niri version knows, in niri's own order. Derived
/// from `niri_ipc::Action`, so it moves in lockstep with the pinned niri.
#[cfg(not(target_arch = "wasm32"))]
pub fn actions() -> Vec<ActionInfo> {
    use clap::CommandFactory;

    niri_ipc::Action::command()
        .get_subcommands()
        .map(|action| ActionInfo {
            name: action.get_name().to_string(),
            about: action
                .get_about()
                .map(|about| about.to_string())
                .unwrap_or_default(),
            arguments: action
                .get_arguments()
                .filter(|argument| !matches!(argument.get_id().as_str(), "help" | "version"))
                .map(|argument| ArgumentInfo {
                    name: argument.get_id().as_str().replace('_', "-"),
                    positional: argument.is_positional(),
                    switch: !argument.get_action().takes_values(),
                    repeated: argument
                        .get_num_args()
                        .is_some_and(|range| range.max_values() > 1),
                })
                .collect(),
        })
        .collect()
}

/// The same catalog in the browser demo, where `niri-ipc` (Unix sockets)
/// cannot build: read from `actions.txt`, which a native test keeps equal
/// to what `niri-ipc` says.
#[cfg(target_arch = "wasm32")]
pub fn actions() -> Vec<ActionInfo> {
    parse_catalog(CATALOG)
}

/// [`actions`] as committed text, for builds without `niri-ipc`.
pub const CATALOG: &str = include_str!("actions.txt");

/// One action per line: name, description and arguments, tab separated;
/// each argument is `name/flags`, flags `p` positional, `s` switch,
/// `r` repeated.
pub fn catalog_text(actions: &[ActionInfo]) -> String {
    let mut out = String::new();
    for action in actions {
        let arguments: Vec<String> = action
            .arguments
            .iter()
            .map(|argument| {
                let flag = |on: bool, letter: char| if on { letter } else { '-' };
                format!(
                    "{}/{}{}{}",
                    argument.name,
                    flag(argument.positional, 'p'),
                    flag(argument.switch, 's'),
                    flag(argument.repeated, 'r'),
                )
            })
            .collect();
        out.push_str(&format!(
            "{}\t{}\t{}\n",
            action.name,
            action.about.replace(['\t', '\n'], " "),
            arguments.join(" ")
        ));
    }
    out
}

pub fn parse_catalog(text: &str) -> Vec<ActionInfo> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next()?.to_string();
            let about = fields.next().unwrap_or_default().to_string();
            let arguments = fields
                .next()
                .unwrap_or_default()
                .split_whitespace()
                .filter_map(|argument| {
                    let (name, flags) = argument.rsplit_once('/')?;
                    Some(ArgumentInfo {
                        name: name.to_string(),
                        positional: flags.contains('p'),
                        switch: flags.contains('s'),
                        repeated: flags.contains('r'),
                    })
                })
                .collect();
            Some(ActionInfo {
                name,
                about,
                arguments,
            })
        })
        .collect()
}

/// A binding's action as written (`spawn "kitty"`): its name and its
/// arguments.
pub fn split_action(source: &str) -> (String, String) {
    match source.split_once(' ') {
        Some((name, args)) => (name.to_string(), args.to_string()),
        None => (source.to_string(), String::new()),
    }
}

/// The KDL body for `action` with arguments `args` as typed into a form.
///
/// Arguments already written as KDL (`"kitty"`, `1`, `focus=false`) are kept
/// verbatim. Plain words are quoted: one string per word for `spawn` (its
/// arguments are an argv), one string for everything else (`spawn-sh "…"`).
pub fn action_body(action: &str, args: &str) -> String {
    let args = args.trim();
    if args.is_empty() {
        action.to_string()
    } else if is_kdl_arguments(args) {
        format!("{action} {args}")
    } else if action == "spawn" {
        let quoted: Vec<String> = args.split_whitespace().map(crate::kdl::quoted).collect();
        format!("{action} {}", quoted.join(" "))
    } else {
        format!("{action} {}", crate::kdl::quoted(args))
    }
}

/// `args` parses as the arguments and properties of one KDL node, with no
/// bare words except the keywords niri accepts as values.
fn is_kdl_arguments(args: &str) -> bool {
    use crate::kdl::ValueKind;

    let Ok(doc) = crate::kdl::parse_document(&format!("action {args}")) else {
        return false;
    };
    let [node] = doc.roots.as_slice() else {
        return false;
    };
    let is_value = |kind: &ValueKind| match kind {
        ValueKind::Quoted(_) | ValueKind::Number(_) => true,
        ValueKind::Bare(word) => matches!(word.as_str(), "true" | "false" | "null"),
    };
    node.children.is_empty()
        && node.block.is_none()
        && node.args.iter().all(|arg| is_value(&arg.kind))
        && node.props.iter().all(|prop| is_value(&prop.value.kind))
}

#[cfg(test)]
mod tests {
    use super::KeyCombo;

    #[test]
    fn combos_are_told_apart_as_niri_does() {
        assert_eq!(
            KeyCombo::parse("Mod+Shift+T"),
            KeyCombo::parse("shift+mod+t")
        );
        assert_eq!(
            KeyCombo::parse("Ctrl+Win+A"),
            KeyCombo::parse("control+super+a")
        );
        assert_ne!(KeyCombo::parse("Mod+T"), KeyCombo::parse("Mod+Shift+T"));
        assert_eq!(KeyCombo::parse("Mod+XF86AudioMute").key(), "xf86audiomute");
    }

    /// The committed catalog is what the pinned `niri-ipc` says; run with
    /// `UPDATE_ACTIONS=1` after bumping niri-ipc to rewrite it.
    #[test]
    fn the_committed_action_catalog_matches_niri_ipc() {
        let current = super::catalog_text(&super::actions());
        if std::env::var_os("UPDATE_ACTIONS").is_some() {
            std::fs::write(
                concat!(env!("CARGO_MANIFEST_DIR"), "/src/actions.txt"),
                &current,
            )
            .unwrap();
            return;
        }
        assert!(
            current == super::CATALOG,
            "src/actions.txt is out of date: run UPDATE_ACTIONS=1 cargo test -p niri-center"
        );
        assert_eq!(super::parse_catalog(super::CATALOG), super::actions());
    }

    use super::action_body;

    #[test]
    fn the_catalog_matches_the_linked_niri() {
        let actions = super::actions();
        let names: Vec<&str> = actions.iter().map(|a| a.name.as_str()).collect();
        // a spread across niri's groups, and nothing duplicated
        for name in [
            "quit",
            "spawn",
            "spawn-sh",
            "focus-workspace",
            "set-column-width",
            "screenshot",
        ] {
            assert!(names.contains(&name), "{name} missing");
        }
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len());
        let usage = |name: &str| actions.iter().find(|a| a.name == name).unwrap().usage();
        assert_eq!(usage("spawn"), "\"<command>\" …");
        assert_eq!(usage("quit"), "skip-confirmation=true");
        assert_eq!(usage("focus-workspace"), "<reference>");
        assert_eq!(
            usage("move-column-to-workspace"),
            "<reference> focus=<value>"
        );
    }

    #[test]
    fn kdl_arguments_are_kept_verbatim() {
        assert_eq!(action_body("spawn", "\"kitty\""), "spawn \"kitty\"");
        assert_eq!(action_body("focus-workspace", "2"), "focus-workspace 2");
        assert_eq!(
            action_body("move-column-to-workspace", "2 focus=false"),
            "move-column-to-workspace 2 focus=false"
        );
        assert_eq!(action_body("quit", ""), "quit");
    }

    #[test]
    fn plain_words_are_quoted_per_action() {
        assert_eq!(
            action_body("spawn", "firefox --new-window"),
            "spawn \"firefox\" \"--new-window\""
        );
        assert_eq!(
            action_body("spawn-sh", "grim -g \"$(slurp)\" - | wl-copy"),
            "spawn-sh \"grim -g \\\"$(slurp)\\\" - | wl-copy\""
        );
        assert_eq!(
            action_body("focus-workspace", "browser"),
            "focus-workspace \"browser\""
        );
    }
}
