//! Window rules: listed, toggled, removed and added losslessly.

mod common;
use common::*;
use std::fs;

use niri_center::store::{MatchName, NewWindowRule, RuleProperty, Store, TextMatch};
use niri_center::validate;

const CONFIG: &str = "// my rules\nwindow-rule {\n    match app-id=\"firefox$\" title=\"^Picture-in-Picture$\"\n    open-floating true\n}\n\n/-window-rule {\n    match app-id=\"steam\"\n    open-maximized true\n}\n";

fn loaded(source: &str) -> (tempfile::TempDir, Store) {
    let dir = tempfile::TempDir::new().unwrap();
    fs::write(dir.path().join("config.kdl"), source).unwrap();
    let store = load(dir.path().join("config.kdl"));
    (dir, store)
}

#[test]
fn rules_list_their_criteria_and_properties_as_written() {
    let (_dir, store) = loaded(CONFIG);
    let rows = store.window_rule_rows();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].criteria,
        ["match app-id=\"firefox$\" title=\"^Picture-in-Picture$\""]
    );
    assert_eq!(rows[0].properties, ["open-floating true"]);
    assert!(!rows[0].disabled);
    assert!(rows[1].disabled);
}

#[test]
fn toggling_and_removing_touch_only_that_rule() {
    let (_dir, mut store) = loaded(CONFIG);
    store.set_window_rule_disabled(1, false).unwrap();
    store.set_window_rule_disabled(0, true).unwrap();
    let out = store.document().to_string();
    assert!(out.starts_with("// my rules\n/- window-rule {"), "{out}");
    assert!(
        out.contains("\nwindow-rule {\n    match app-id=\"steam\""),
        "{out}"
    );

    store.remove_window_rule(0).unwrap();
    assert_eq!(
        store.document().to_string(),
        "// my rules\n\nwindow-rule {\n    match app-id=\"steam\"\n    open-maximized true\n}\n"
    );
}

#[test]
fn a_new_rule_matches_exactly_and_goes_last() {
    let (_dir, mut store) = loaded(CONFIG);
    store
        .add_window_rule(&NewWindowRule {
            app_id: Some("org.gnome.Calculator".into()),
            title: Some("Calc (1+1)".into()),
            properties: vec![RuleProperty::OpenFloating],
            ..NewWindowRule::default()
        })
        .unwrap();
    let out = store.document().to_string();
    assert!(
        out.ends_with(
            "\nwindow-rule {\n    match app-id=r#\"^org\\.gnome\\.Calculator$\"# title=r#\"^Calc \\(1\\+1\\)$\"#\n    open-floating true\n}\n"
        ),
        "{out}"
    );
    assert_eq!(store.window_rule_rows().len(), 3);
    if validate::niri_available() {
        check(&store).unwrap();
    }
}

#[test]
fn a_rule_the_form_writes_reads_back_as_the_same_rule() {
    let rule = NewWindowRule {
        app_id: Some("org.gnome.Calculator".into()),
        app_id_how: TextMatch::Exact,
        title: Some("Calc (1+1)".into()),
        title_how: TextMatch::Contains,
        flags: vec![(MatchName::IsFloating, true)],
        properties: RuleProperty::ALL.to_vec(),
    };
    let (_dir, store) = loaded(&format!("{}\n", rule.to_kdl()));
    let rows = store.window_rule_rows();
    assert_eq!(NewWindowRule::from_row(&rows[0]), Some(rule));
}

#[test]
fn rules_the_form_cannot_say_in_full_stay_text() {
    let cannot = [
        // a comment would be lost, between lines or inside one
        "window-rule {\n    // floats\n    match app-id=\"a\"\n    open-floating true\n}\n",
        "window-rule {\n    match app-id=\"a\" /* why */ title=\"b\"\n    open-floating true\n}\n",
        // a line turned off would come back on
        "window-rule {\n    match app-id=\"a\"\n    /- open-floating true\n}\n",
        "window-rule {\n    match app-id=\"a\"\n    exclude title=\"b\"\n    open-floating true\n}\n",
        "window-rule {\n    match app-id=\"a\"\n    match app-id=\"b\"\n    open-floating true\n}\n",
        "window-rule {\n    match app-id=\"a\"\n    opacity 0.5\n}\n",
        "/- window-rule {\n    match app-id=\"a\"\n    open-floating true\n}\n",
    ];
    for source in cannot {
        let (_dir, store) = loaded(source);
        let rows = store.window_rule_rows();
        assert_eq!(NewWindowRule::from_row(&rows[0]), None, "{source}");
    }
    // however it is spaced and quoted, a plain rule opens
    let (_dir, store) = loaded("window-rule { match app-id=\"^firefox$\"; open-floating true; }\n");
    let rule = NewWindowRule::from_row(&store.window_rule_rows()[0]).unwrap();
    assert_eq!(rule.app_id.as_deref(), Some("firefox"));
    assert_eq!(rule.app_id_how, TextMatch::Exact);
}

#[test]
fn every_property_the_builder_offers_is_valid_niri_config() {
    let (_dir, mut store) = loaded("");
    store
        .add_window_rule(&NewWindowRule {
            app_id: Some("example".into()),
            title: None,
            properties: RuleProperty::ALL.to_vec(),
            ..NewWindowRule::default()
        })
        .unwrap();
    if validate::niri_available() {
        check(&store).unwrap();
    }
}
