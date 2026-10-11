//! Lossless KDL document model for niri configuration files.
//!
//! # Why "lossless" is the whole game
//!
//! NiriMod and niri-settings both promise that editing one value never touches
//! anything else in a user's hand-written config: comments, blank lines,
//! formatting quirks, and — critically — *blocks owned by third-party shells*
//! (Noctalia, DankMaterialShell, ...) must survive byte-for-byte.
//!
//! The crates.io `kdl` crate is a *values* parser: it drops comments and
//! re-serializes with its own formatting decisions. That makes it useless for
//! this feature. Instead we:
//!
//! 1. parse into a tree of [`Node`]s where every token keeps its [`Span`] into
//!    the original source text;
//! 2. never regenerate the file — mutations are recorded as *splices*
//!    ([`Edit`]) into the original text;
//! 3. serialize with [`Document::to_string`], which applies the splices and
//!    leaves every untouched byte exactly as the user wrote it.
//!
//! Comments and blank lines need no AST representation at all: they are simply
//! text we skip while parsing, and survive because we never rewrite them.

mod doc;
mod parse;

pub use doc::{
    Document, Edit, EditError, Node, PathSeg, Prop, Span, Value, ValueKind, active_named, quoted,
};
pub use parse::{ParseError, parse};

/// Parse `source` into a lossless document.
pub fn parse_document(source: &str) -> Result<Document, ParseError> {
    parse(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_strings_in_props_and_args() {
        let src = "window-rule {\n    match app-id=r##\"^org\\.wezfurlong\\.wezterm$\"## title=r#\"(?i)^(Password)\"#\n    default-column-width {}\n}\n";
        let doc = parse_document(src).unwrap();
        let rule = doc.find(&[PathSeg::Name("window-rule")]).unwrap();
        let m = rule.children.iter().find(|c| c.name == "match").unwrap();
        assert_eq!(m.props.len(), 2);
        assert_eq!(
            doc.value_text(&m.props[0].value),
            "^org\\.wezfurlong\\.wezterm$"
        );
        // no escapes were processed in the raw string
        assert!(!doc.value_text(&m.props[0].value).contains("\\\\"));
        // empty block `{}` parses to a block with no children
        let dcw = rule
            .children
            .iter()
            .find(|c| c.name == "default-column-width")
            .unwrap();
        assert!(dcw.children.is_empty());
        assert!(dcw.block.is_some());
        assert_eq!(doc.to_string(), src);
    }

    #[test]
    fn bare_ident_values_in_props() {
        // `hotkey-overlay-title=null` and `repeat=false` are niri-isms
        let src = "Mod+O repeat=false { toggle-overview; }\nSuper+Alt+S allow-when-locked=true hotkey-overlay-title=null { spawn-sh \"x\"; }\n";
        let doc = parse_document(src).unwrap();
        assert_eq!(doc.roots[1].name, "Super+Alt+S");
        let super_alt_s = &doc.roots[1].props;
        assert_eq!(super_alt_s[1].name, "hotkey-overlay-title");
        assert_eq!(super_alt_s[1].value.kind, ValueKind::Bare("null".into()));
        assert_eq!(doc.to_string(), src);
    }

    #[test]
    fn slash_dash_marks_disabled_nodes_losslessly() {
        let src = "binds {\n    Mod+X { quit; }\n    /- Mod+Y { spawn \"kitty\"; }\n}\n";
        let doc = parse_document(src).unwrap();
        let binds = doc.find_all("binds").pop().unwrap();
        assert_eq!(binds.children.len(), 2);
        assert!(!binds.children[0].disabled);
        assert!(binds.children[1].disabled);
        // node content still fully parsed despite the marker
        assert_eq!(binds.children[1].name, "Mod+Y");
        assert_eq!(binds.children[1].children[0].name, "spawn");
        assert_eq!(doc.to_string(), src);
    }

    #[test]
    fn parses_realistic_niri_output_block() {
        let src = r#"
// built-in panel first
output "DP-1" {
    mode "2560x1440@165.001"
    scale 1.0
    position x=2560 y=0
    variable-refresh-rate on-demand=true
}

/* desktop monitors */
output "HDMI-A-1" {
    scale 1.25
}
"#;
        let doc = parse_document(src).unwrap();
        let outputs: Vec<&Node> = doc.find_all("output");
        assert_eq!(outputs.len(), 2);

        let dp1 = doc
            .find(&[PathSeg::Name("output"), PathSeg::Index(0)])
            .unwrap();
        assert_eq!(dp1.args.len(), 1);
        assert_eq!(dp1.args[0].kind, ValueKind::Quoted("DP-1".into()));
        assert_eq!(dp1.children.len(), 4);
        // children: mode, scale, position, variable-refresh-rate
        assert_eq!(dp1.children[0].name, "mode");
        assert_eq!(
            dp1.children[0].args[0].kind,
            ValueKind::Quoted("2560x1440@165.001".into())
        );
        assert_eq!(dp1.children[1].name, "scale");
        assert_eq!(
            dp1.children[1].args[0].kind,
            ValueKind::Number("1.0".into())
        );
        // position x=2560 y=0 is a child NODE with two props
        let position = &dp1.children[2];
        assert_eq!(position.name, "position");
        assert_eq!(position.props.len(), 2);
        assert_eq!(position.props[0].name, "x");
        assert_eq!(
            position.props[0].value.kind,
            ValueKind::Number("2560".into())
        );
        assert_eq!(position.props[1].name, "y");
        // variable-refresh-rate on-demand=true
        let vrr = &dp1.children[3];
        assert_eq!(vrr.name, "variable-refresh-rate");
        assert_eq!(vrr.props[0].name, "on-demand");
        assert_eq!(vrr.props[0].value.kind, ValueKind::Bare("true".into()));
        // lossless: no edits -> byte-identical output
        assert_eq!(doc.to_string(), src);
    }

    #[test]
    fn edit_prop_only_touches_one_value_and_keeps_comments() {
        let src = r#"// Hi! this is my niri config
// outputs

output "DP-1" {
    mode "2560x1440@165.001"
    scale 1.0 /* comment that must survive */
    position x=2560 y=0
}

// noctalia block — foreign, must not be touched
layer-rule {
    match namespace="^noctalia-backdrop"
    place-within-backdrop true
}
"#;
        let mut doc = parse_document(src).unwrap();
        doc.edit_prop(
            &[
                PathSeg::Name("output"),
                PathSeg::Index(0),
                PathSeg::Name("scale"),
            ],
            "scale",
            "scale",
        )
        .unwrap_err(); // scale is an ARG, not a prop — must fail
        doc.edit_arg(
            &[
                PathSeg::Name("output"),
                PathSeg::Index(0),
                PathSeg::Name("scale"),
            ],
            0,
            "1.5",
        )
        .unwrap();
        let out = doc.to_string();
        // every comment still present in order
        assert!(out.contains("// Hi! this is my niri config"));
        assert!(out.contains("/* comment that must survive */"));
        assert!(out.contains("// noctalia block — foreign, must not be touched"));
        // the changed value
        assert!(out.contains("scale 1.5"));
        assert!(!out.contains("scale 1.0"));
        // foreign block byte-identical
        let noctalia = "layer-rule {\n    match namespace=\"^noctalia-backdrop\"\n    place-within-backdrop true\n}";
        assert!(out.contains(noctalia));
        // exactly one line differs from the original
        let orig_lines: Vec<&str> = src.lines().collect();
        let out_lines: Vec<&str> = out.lines().collect();
        assert_eq!(orig_lines.len(), out_lines.len());
        let diffs: Vec<_> = orig_lines
            .iter()
            .zip(&out_lines)
            .filter(|(a, b)| a != b)
            .collect();
        assert_eq!(diffs.len(), 1, "only the scale line should change");
        assert_eq!(*diffs[0].0, "    scale 1.0 /* comment that must survive */");
        assert_eq!(*diffs[0].1, "    scale 1.5 /* comment that must survive */");
    }

    #[test]
    fn bare_ident_argument_is_an_argument_not_a_child() {
        let src = "hot-corners {\n    off\n}\n";
        let doc = parse_document(src).unwrap();
        let hot = doc.find(&[PathSeg::Name("hot-corners")]).unwrap();
        assert_eq!(hot.children.len(), 1);
        assert_eq!(hot.children[0].name, "off");
        assert!(hot.children[0].args.is_empty());
        assert_eq!(doc.to_string(), src);
    }

    #[test]
    fn multiple_nodes_on_one_line() {
        let src = "a 1 b 2 c\n";
        let doc = parse_document(src).unwrap();
        // KDL bare-identifier rule: `c` is the last element on the line, so it
        // becomes a bare argument of `b`, not a third node.
        assert_eq!(doc.roots.len(), 2);
        assert_eq!(doc.roots[0].name, "a");
        assert_eq!(doc.roots[1].name, "b");
        assert_eq!(doc.roots[1].args.len(), 2);
        assert_eq!(doc.roots[1].args[1].kind, ValueKind::Bare("c".into()));
        assert_eq!(doc.to_string(), src);
    }

    #[test]
    fn ident_followed_by_value_start_new_sibling() {
        // classic niri one-node-per-line shape
        let src = "mode \"1920x1080@120.030\"\nscale 2.0\n";
        let doc = parse_document(src).unwrap();
        assert_eq!(doc.roots.len(), 2);
        assert_eq!(doc.roots[0].name, "mode");
        assert_eq!(doc.roots[1].name, "scale");
    }

    #[test]
    fn quoted_strings_with_escapes() {
        let src = "spawn-sh-at-startup \"bash -c 'echo \\\"hi\\\"'\"\n";
        let doc = parse_document(src).unwrap();
        let node = &doc.roots[0];
        assert_eq!(node.args.len(), 1);
        assert!(matches!(&node.args[0].kind, ValueKind::Quoted(_)));
        assert_eq!(doc.to_string(), src);
    }

    #[test]
    fn semicolons_end_nodes() {
        // `;` is KDL's node terminator; niri accepts this one-line block
        let src = "input {\n    touchpad { tap; natural-scroll; }\n}\n";
        let doc = parse_document(src).unwrap();
        let touchpad = &doc.roots[0].children[0];
        let names: Vec<&str> = touchpad.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["tap", "natural-scroll"]);
        assert!(touchpad.children.iter().all(|c| c.args.is_empty()));
        assert_eq!(doc.to_string(), src);
    }

    #[test]
    fn keyword_before_semicolon_is_an_argument() {
        let src = "window-rule { match app-id=\"x\"; open-floating true; }\nMod+T { quit; };\n";
        let doc = parse_document(src).unwrap();
        let rule = &doc.roots[0];
        let names: Vec<&str> = rule.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["match", "open-floating"]);
        assert_eq!(
            rule.children[1].args[0].kind,
            ValueKind::Bare("true".into())
        );
        // a stray `;` after a block closes nothing and is kept verbatim
        assert_eq!(doc.roots[1].name, "Mod+T");
        assert_eq!(doc.roots.len(), 2);
        assert_eq!(doc.to_string(), src);
    }

    #[test]
    fn negative_and_decimal_numbers() {
        let src = "layout {\n\tspacing -4\n\tgaps 0.5\n}\n";
        let doc = parse_document(src).unwrap();
        let layout = &doc.roots[0];
        assert_eq!(layout.children.len(), 2);
        assert_eq!(layout.children[0].name, "spacing");
        assert_eq!(doc.to_string(), src);
    }
}
