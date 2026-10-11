//! The settings engine against niri's documented include semantics (niri
//! wiki, Configuration: Include): includes are positional, most sections
//! merge field by field, animation subsections are replaced whole.

mod common;
use common::*;
use std::fs;
use std::path::Path;

use niri_center::store::{Block, Store};
use niri_center::validate;

const LAYOUT: [Block<'static>; 1] = [Block::named("layout")];

/// Write `files` (name, content) into a temp dir; load `config.kdl`.
fn tree(files: &[(&str, &str)]) -> (tempfile::TempDir, Store) {
    let dir = tempfile::TempDir::new().unwrap();
    for (name, content) in files {
        fs::write(dir.path().join(name), content).unwrap();
    }
    let store = load(dir.path().join("config.kdl"));
    (dir, store)
}

fn file(store: &Store, name: &str) -> String {
    if name == "config.kdl" {
        return store.document().to_string();
    }
    store
        .included_files()
        .iter()
        .find(|f| f.path.file_name() == Some(Path::new(name).as_os_str()))
        .unwrap()
        .doc
        .to_string()
}

fn assert_niri_accepts(store: &Store) {
    if validate::niri_available() {
        check(store).unwrap();
    }
}

#[test]
fn a_value_after_the_include_overrides_it_and_takes_the_edit() {
    let (_dir, mut store) = tree(&[
        ("config.kdl", "include \"a.kdl\"\nlayout {\n    gaps 8\n}\n"),
        ("a.kdl", "layout {\n    gaps 4\n}\n"),
    ]);
    assert_eq!(store.setting(&LAYOUT, "gaps").as_deref(), Some("8"));
    store.set_setting(&LAYOUT, "gaps", "10").unwrap();
    assert!(file(&store, "config.kdl").contains("gaps 10"));
    assert_eq!(file(&store, "a.kdl"), "layout {\n    gaps 4\n}\n");
}

#[test]
fn an_include_after_the_value_overrides_it_and_takes_the_edit() {
    let (_dir, mut store) = tree(&[
        ("config.kdl", "layout {\n    gaps 8\n}\ninclude \"a.kdl\"\n"),
        ("a.kdl", "layout {\n    gaps 4\n}\n"),
    ]);
    assert_eq!(store.setting(&LAYOUT, "gaps").as_deref(), Some("4"));
    store.set_setting(&LAYOUT, "gaps", "10").unwrap();
    assert_eq!(file(&store, "a.kdl"), "layout {\n    gaps 10\n}\n");
    assert!(file(&store, "config.kdl").contains("gaps 8"));
}

#[test]
fn sections_merge_field_by_field_across_files() {
    let (_dir, store) = tree(&[
        ("config.kdl", "include \"a.kdl\"\nlayout {\n    gaps 8\n}\n"),
        (
            "a.kdl",
            "layout {\n    focus-ring {\n        width 4\n    }\n}\n",
        ),
    ]);
    let ring = [Block::named("layout"), Block::named("focus-ring")];
    assert_eq!(store.setting(&ring, "width").as_deref(), Some("4"));
    assert_eq!(store.setting(&LAYOUT, "gaps").as_deref(), Some("8"));
}

#[test]
fn missing_blocks_are_created_with_the_files_indentation() {
    let ring = [Block::named("layout"), Block::named("focus-ring")];

    let (_dir, mut store) = tree(&[("config.kdl", "")]);
    store.set_setting(&ring, "width", "6").unwrap();
    assert_eq!(
        store.document().to_string(),
        "layout {\n    focus-ring {\n        width 6\n    }\n}\n"
    );
    assert_niri_accepts(&store);

    let (_dir, mut store) = tree(&[("config.kdl", "layout {\n    gaps 8\n}\n")]);
    store.set_setting(&ring, "width", "6").unwrap();
    assert_eq!(
        store.document().to_string(),
        "layout {\n    gaps 8\n    focus-ring {\n        width 6\n    }\n}\n"
    );
    assert_niri_accepts(&store);

    let (_dir, mut store) = tree(&[("config.kdl", "layout {}\n")]);
    store.set_setting(&LAYOUT, "gaps", "8").unwrap();
    assert_eq!(store.document().to_string(), "layout {\n    gaps 8\n}\n");
}

#[test]
fn resetting_removes_every_occurrence_so_niris_default_applies() {
    let (_dir, mut store) = tree(&[
        ("config.kdl", "include \"a.kdl\"\nlayout {\n    gaps 8\n}\n"),
        ("a.kdl", "layout {\n    gaps 4\n}\n"),
    ]);
    store.unset_setting(&LAYOUT, "gaps").unwrap();
    assert_eq!(store.setting(&LAYOUT, "gaps"), None);
    assert!(!file(&store, "config.kdl").contains("gaps"));
    assert!(!file(&store, "a.kdl").contains("gaps"));
    assert_niri_accepts(&store);
}

#[test]
fn turning_a_flag_off_removes_it_from_every_file() {
    let touchpad = [Block::named("input"), Block::named("touchpad")];
    let (_dir, mut store) = tree(&[
        (
            "config.kdl",
            "input {\n    touchpad {\n        tap\n    }\n}\ninclude \"a.kdl\"\n",
        ),
        ("a.kdl", "input {\n    touchpad {\n        tap\n    }\n}\n"),
    ]);
    store.set_flag_setting(&touchpad, "tap", false).unwrap();
    assert!(!store.is_set(&touchpad, "tap"));
    assert!(!file(&store, "a.kdl").contains("tap"));
    assert!(!file(&store, "config.kdl").contains("tap"));
}

#[test]
fn one_line_blocks_stay_one_line() {
    let touchpad = [Block::named("input"), Block::named("touchpad")];
    let (_dir, mut store) = tree(&[("config.kdl", "input {\n    touchpad { tap; }\n}\n")]);
    store
        .set_flag_setting(&touchpad, "natural-scroll", true)
        .unwrap();
    assert_eq!(
        store.document().to_string(),
        "input {\n    touchpad { tap; natural-scroll; }\n}\n"
    );
    store.set_flag_setting(&touchpad, "tap", false).unwrap();
    assert_eq!(
        store.document().to_string(),
        "input {\n    touchpad { natural-scroll; }\n}\n"
    );
    assert_niri_accepts(&store);
}

#[test]
fn a_later_animation_block_replaces_an_earlier_one_whole() {
    let (_dir, store) = tree(&[
        (
            "config.kdl",
            "include \"a.kdl\"\nanimations {\n    window-open {\n        duration-ms 200\n    }\n}\n",
        ),
        (
            "a.kdl",
            "animations {\n    window-open {\n        curve \"linear\"\n    }\n}\n",
        ),
    ]);
    // the later window-open block has no curve, and it replaces the earlier
    assert_eq!(store.easing_curve("window-open"), None);
}

#[test]
fn top_level_settings_use_an_empty_chain() {
    let (_dir, mut store) = tree(&[("config.kdl", "layout {\n    gaps 8\n}\n")]);
    assert!(!store.is_set(&[], "prefer-no-csd"));
    store.set_flag_setting(&[], "prefer-no-csd", true).unwrap();
    assert_eq!(
        store.document().to_string(),
        "layout {\n    gaps 8\n}\nprefer-no-csd\n"
    );
    assert!(store.is_set(&[], "prefer-no-csd"));
    assert_niri_accepts(&store);
}

#[test]
fn values_with_props_are_replaced_without_touching_the_rest_of_the_line() {
    let (_dir, mut store) = tree(&[(
        "config.kdl",
        "output \"DP-1\" {\n    position x=0 y=0 // left of the laptop\n}\n",
    )]);
    let output = [Block::with_arg("output", "DP-1")];
    store
        .set_setting(&output, "position", "x=1920 y=0")
        .unwrap();
    assert_eq!(
        store.document().to_string(),
        "output \"DP-1\" {\n    position x=1920 y=0 // left of the laptop\n}\n"
    );
}

fn entries(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
        .collect()
}

#[test]
fn sizes_change_entry_by_entry_and_keep_the_comments_around_them() {
    let source = "layout {\n    preset-column-widths {\n        // thirds\n        proportion 0.33333\n        proportion 0.5 // half\n        /- fixed 1920\n    }\n}\n";
    let (_dir, mut store) = tree(&[("config.kdl", source)]);
    // one more at the end
    let more = entries(&[
        ("proportion", "0.33333"),
        ("proportion", "0.5"),
        ("proportion", "1.0"),
    ]);
    store
        .set_sizes(&LAYOUT, "preset-column-widths", Some(&more))
        .unwrap();
    assert_eq!(
        file(&store, "config.kdl"),
        "layout {\n    preset-column-widths {\n        // thirds\n        proportion 0.33333\n        proportion 0.5 // half\n        proportion 1.0\n        /- fixed 1920\n    }\n}\n"
    );
    assert_niri_accepts(&store);
    // one changed in its place, the last dropped
    let fewer = entries(&[("proportion", "0.25"), ("proportion", "0.5")]);
    store
        .set_sizes(&LAYOUT, "preset-column-widths", Some(&fewer))
        .unwrap();
    assert_eq!(
        file(&store, "config.kdl"),
        "layout {\n    preset-column-widths {\n        // thirds\n        proportion 0.25\n        proportion 0.5 // half\n        /- fixed 1920\n    }\n}\n"
    );
    assert_eq!(
        store.sizes(&LAYOUT, "preset-column-widths"),
        Some(fewer.clone())
    );
}

#[test]
fn removing_a_middle_size_takes_out_that_line_alone() {
    let source = "layout {\n    preset-column-widths {\n        proportion 0.25 // quarter\n        proportion 0.5 // half\n        proportion 0.75 // three quarters\n    }\n}\n";
    let (_dir, mut store) = tree(&[("config.kdl", source)]);
    let without_half = entries(&[("proportion", "0.25"), ("proportion", "0.75")]);
    store
        .set_sizes(&LAYOUT, "preset-column-widths", Some(&without_half))
        .unwrap();
    assert_eq!(
        file(&store, "config.kdl"),
        "layout {\n    preset-column-widths {\n        proportion 0.25 // quarter\n        proportion 0.75 // three quarters\n    }\n}\n"
    );
    // and one put in front goes before the first, indented like it
    let with_tenth = entries(&[
        ("proportion", "0.1"),
        ("proportion", "0.25"),
        ("proportion", "0.75"),
    ]);
    store
        .set_sizes(&LAYOUT, "preset-column-widths", Some(&with_tenth))
        .unwrap();
    assert_eq!(
        file(&store, "config.kdl"),
        "layout {\n    preset-column-widths {\n        proportion 0.1\n        proportion 0.25 // quarter\n        proportion 0.75 // three quarters\n    }\n}\n"
    );
    assert_niri_accepts(&store);
}

#[test]
fn sizes_on_one_line_stay_on_one_line() {
    let (_dir, mut store) = tree(&[(
        "config.kdl",
        "layout {\n    preset-column-widths { proportion 0.5; }\n}\n",
    )]);
    let two = entries(&[("proportion", "0.5"), ("fixed", "800")]);
    store
        .set_sizes(&LAYOUT, "preset-column-widths", Some(&two))
        .unwrap();
    assert_eq!(
        file(&store, "config.kdl"),
        "layout {\n    preset-column-widths { proportion 0.5; fixed 800; }\n}\n"
    );
    let one = entries(&[("fixed", "800")]);
    store
        .set_sizes(&LAYOUT, "preset-column-widths", Some(&one))
        .unwrap();
    assert_eq!(store.sizes(&LAYOUT, "preset-column-widths"), Some(one));
    assert_niri_accepts(&store);
}

/// A connected display, on screen at `x`.
fn connected(name: &str, x: f64) -> niri_center::outputs::OutputInfo {
    niri_center::outputs::OutputInfo {
        name: name.to_string(),
        description: String::new(),
        modes: Vec::new(),
        preferred: None,
        current: None,
        vrr_supported: false,
        vrr_enabled: false,
        scale: 1.0,
        rect: Some(niri_center::outputs::MonitorInfo {
            x,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        }),
    }
}

#[test]
fn the_last_display_that_is_on_stays_on() {
    use niri_center::settings::{Drafts, Edit, OutputSetting, edit_output};
    let (_dir, mut store) = tree(&[("config.kdl", "output \"DP-1\" {\n    off\n}\n")]);
    let mut drafts = Drafts::default();
    let outputs = [connected("DP-1", 0.0), connected("eDP-1", 1920.0)];
    // DP-1 is off already: eDP-1 is the only one on
    assert!(
        edit_output(
            &mut store,
            &mut drafts,
            &outputs,
            "eDP-1",
            OutputSetting::Off,
            Edit::Switch(true),
        )
        .is_err()
    );
    assert!(!store.document().to_string().contains("eDP-1"));
    // with DP-1 back on, eDP-1 may go off
    edit_output(
        &mut store,
        &mut drafts,
        &outputs,
        "DP-1",
        OutputSetting::Off,
        Edit::Switch(false),
    )
    .unwrap();
    edit_output(
        &mut store,
        &mut drafts,
        &outputs,
        "eDP-1",
        OutputSetting::Off,
        Edit::Switch(true),
    )
    .unwrap();
    assert!(store.document().to_string().contains("output \"eDP-1\""));
}
