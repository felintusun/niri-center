//! P0 acceptance tests for the lossless KDL engine against a realistic config.

mod common;
use niri_center::kdl::{PathSeg, parse_document};

const FIXTURE: &str = include_str!("fixtures/config.kdl");

#[test]
fn identity_roundtrip_is_byte_identical() {
    let doc = parse_document(FIXTURE).unwrap();
    assert_eq!(doc.to_string(), FIXTURE);
    assert_eq!(doc.pending_edits(), 0);
}

#[test]
fn edits_apply_and_only_touch_the_target() {
    let mut doc = parse_document(FIXTURE).unwrap();

    // change scale of eDP-1 (argument 0 of its scale child)
    doc.edit_arg(
        &[
            PathSeg::Name("output"),
            PathSeg::Index(1),
            PathSeg::Name("scale"),
        ],
        0,
        "1.6",
    )
    .unwrap();

    // change position.x of DP-1
    doc.edit_prop(
        &[
            PathSeg::Name("output"),
            PathSeg::Index(0),
            PathSeg::Name("position"),
        ],
        "x",
        "3200",
    )
    .unwrap();

    let out = doc.to_string();

    // the two targets changed
    assert!(out.contains("scale 1.6"), "scale edit missing");
    assert!(out.contains("position x=3200 y=0"), "position edit missing");
    assert!(!out.contains("scale 1.5"));

    // comment lines intact, in order
    for needle in [
        "// This is my daily driver — don't mess with the comments",
        "// outputs ---------------------------------------------------------",
        "/* keep this */",
        "// layout -----------------------------------------------------------------",
        "// keybinds --------------------------------------------------------------",
        "// wallpaper: awww",
        "// noctalia wall (foreign — must never be touched)",
    ] {
        assert!(out.contains(needle), "comment {needle:?} lost");
    }

    // foreign layer-rule untouched byte-for-byte
    let foreign = "layer-rule {\n    match namespace=\"^noctalia-backdrop\"\n    place-within-backdrop true\n}";
    assert!(out.contains(foreign));

    // foreign window-rule untouched byte-for-byte
    let window_rule = "window-rule {\n    match app-id=\"dev.noctalia.Noctalia\"\n    open-floating true\n    default-column-width { fixed 1080; }\n}";
    assert!(out.contains(window_rule));

    // diff is exactly the two edited lines
    let orig: Vec<&str> = FIXTURE.lines().collect();
    let new: Vec<&str> = out.lines().collect();
    assert_eq!(orig.len(), new.len());
    let mut changed: Vec<(usize, &str, &str)> = vec![];
    for (i, (a, b)) in orig.iter().zip(&new).enumerate() {
        if a != b {
            changed.push((i, a, b));
        }
    }
    assert_eq!(
        changed.len(),
        2,
        "exactly two lines should differ: {changed:?}"
    );
}

#[test]
fn binds_are_read_as_rows_and_a_rebound_combo_is_a_duplicate() {
    let store = niri_center::store::Store::from_text("config.kdl".into(), FIXTURE).unwrap();
    let rows = store.binding_rows();
    assert_eq!(rows.len(), 4, "four binds in fixture");
    let mod_space = rows.iter().find(|row| row.key == "Mod+Space").unwrap();
    assert!(
        mod_space.action.starts_with("spawn"),
        "{}",
        mod_space.action
    );
    assert!(store.duplicate_keys().is_empty());
    // the same combo, written another way, is the same binding to niri
    let doubled = FIXTURE.replacen("binds {", "binds {\n    MOD+space { quit; }", 1);
    let store = niri_center::store::Store::from_text("config.kdl".into(), &doubled).unwrap();
    assert_eq!(store.duplicate_keys().len(), 1);
}

#[test]
fn config_view_extracts_outputs_and_backdrop_rule() {
    let config = niri_center::config::NiriConfig::parse(FIXTURE).unwrap();
    let names = config.output_names();
    assert_eq!(names, vec!["DP-1", "eDP-1"]);
    assert!(config.has_backdrop_layer_rule());
    let dp1 = config.output(0).unwrap();
    assert_eq!(dp1.args.first().map(|v| v.kind.as_str()), Some("DP-1"));
}
