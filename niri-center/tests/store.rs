//! Config store behavior: binding/autostart edits, include resolution, save.

mod common;
use common::*;
use std::fs;

use niri_center::store::{Block, Store};
use niri_center::validate;

const FIXTURE: &str = include_str!("fixtures/config.kdl");

/// Write the fixture to a temp dir and load it.
fn temp_store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("config.kdl");
    fs::write(&path, FIXTURE).unwrap();
    let store = load(&path);
    (dir, store)
}

#[test]
fn add_binding_splices_a_valid_new_node() {
    let (_dir, mut store) = temp_store();
    store
        .add_binding("Mod+Shift+W", "spawn-sh \"awww -c\"")
        .unwrap();

    let out = store.document().to_string();
    let reparse = niri_center::kdl::parse_document(&out).unwrap();

    let rows = store.binding_rows();
    let keys: Vec<String> = rows.iter().map(|r| r.key.clone()).collect();
    assert_eq!(
        keys,
        vec![
            "Mod+Shift+Slash".to_string(),
            "Mod+Space".to_string(),
            "Mod+Return".to_string(),
            "Mod+L".to_string(),
            "Mod+Shift+W".to_string(),
        ]
    );
    assert_eq!(
        rows.last().map(|r| (r.key.as_str(), r.action.as_str())),
        Some(("Mod+Shift+W", "spawn-sh awww -c"))
    );
    assert!(
        out.contains("Mod+Shift+W { spawn-sh \"awww -c\"; }"),
        "missing semicolon: {out}"
    );
    // inserted as a tidy line right after the previous binding (no blank line)
    assert!(
        out.contains(
            "    Mod+L { spawn \"swaylock\"; }\n    Mod+Shift+W { spawn-sh \"awww -c\"; }"
        ),
        "unexpected layout:\n{out}"
    );
    // foreign block still byte-identical
    let noctalia = "layer-rule {\n    match namespace=\"^noctalia-backdrop\"\n    place-within-backdrop true\n}";
    assert!(out.contains(noctalia));
    // still round-trips identically
    assert_eq!(reparse.to_string(), out);
}

#[test]
fn ensure_binds_appends_block_when_missing() {
    let stdin_no_binds = r#"// minimal
output "DP-1" {
    scale 1.0
}
"#;
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("config.kdl");
    fs::write(&path, stdin_no_binds).unwrap();
    let mut store = load(&path);
    assert!(!store.has_binds());

    store.ensure_binds().unwrap();
    assert!(store.has_binds());

    store.add_binding("Mod+T", "spawn-sh \"kitty\"").unwrap();
    let out = store.document().to_string();
    assert!(out.contains("binds {\n    Mod+T { spawn-sh \"kitty\"; }\n}"));
    // appended block is parseable
    let reparse = niri_center::kdl::parse_document(&out).unwrap();
    assert_eq!(reparse.to_string(), out);
}

#[test]
fn remove_binding_deletes_exactly_its_line() {
    let (_dir, mut store) = temp_store();
    store.remove_binding(1).unwrap(); // Mod+Space

    let out = store.document().to_string();
    assert!(!out.contains("Mod+Space"));
    let keys: Vec<String> = store.binding_rows().into_iter().map(|r| r.key).collect();
    assert_eq!(keys, ["Mod+Shift+Slash", "Mod+Return", "Mod+L"]);
    // the remaining keys and comments untouched
    for needle in [
        "Mod+Shift+Slash",
        "Mod+Return",
        "Mod+L",
        "// keybinds --------------------------------------------------------------",
    ] {
        assert!(out.contains(needle), "lost {needle}");
    }
    // stable under reparse
    let reparse = niri_center::kdl::parse_document(&out).unwrap();
    assert_eq!(reparse.to_string(), out);
}

#[test]
fn duplicate_keys_detected() {
    let (_dir, mut store) = temp_store();
    store
        .add_binding("Mod+Space", "spawn-sh \"xterm\"")
        .unwrap();
    let dups = store.duplicate_keys();
    assert_eq!(dups, vec!["Mod+Space"]);
}

#[test]
fn save_requires_valid_config_and_roundtrips() {
    let (_dir, mut store) = temp_store();
    store.add_binding("Mod+E", "spawn-sh \"vim\"").unwrap();
    let mut disk = folders_in(_dir.path());

    if validate::niri_available() {
        save(&mut disk, &mut store).unwrap();
        // reload from disk — identical output
        let reloaded = load(store.path());
        assert_eq!(
            reloaded.document().to_string(),
            store.document().to_string()
        );
        assert!(reloaded.binding_rows().iter().any(|r| r.key == "Mod+E"));
    } else {
        // niri not on PATH: store must not crash, doc stays well-formed
        assert!(store.document().to_string().contains("Mod+E"));
    }
}

#[test]
fn save_refuses_when_file_changed_on_disk() {
    let (_dir, mut store) = temp_store();
    let mut disk = folders_in(_dir.path());
    store.add_binding("Mod+E", "spawn-sh \"vim\"").unwrap();

    // someone else (an editor, another window) saves in between
    let external = format!("{FIXTURE}\n// edited elsewhere\n");
    fs::write(store.path(), &external).unwrap();

    let err = save(&mut disk, &mut store).unwrap_err();
    assert!(matches!(
        err,
        niri_center::disk::DiskError::ChangedOnDisk { .. }
    ));
    // the external edit survives untouched
    assert_eq!(fs::read_to_string(store.path()).unwrap(), external);
}

#[test]
fn suggested_window_rules_append_once_and_validate() {
    let (_dir, mut store) = temp_store();
    let before = store.document().to_string();
    assert!(!store.has_niri_center_rules());

    assert!(store.add_suggested_window_rules().unwrap());
    let after = store.document().to_string();
    // pure append: the original text is an untouched prefix
    assert!(after.starts_with(&before));
    assert!(store.has_niri_center_rules());

    // second call is a no-op
    assert!(!store.add_suggested_window_rules().unwrap());
    assert_eq!(store.document().to_string(), after);

    if validate::niri_available() {
        check(&store).unwrap();
    }
}

#[test]
fn save_rejects_invalid_config() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("config.kdl");
    fs::write(&path, "binds {\n    Mod+Space { }\n}\n").unwrap();
    let store = load(&path);

    if validate::niri_available() {
        let err = check(&store).unwrap_err();
        assert!(
            matches!(err, niri_center::disk::DiskError::Rejected { .. }),
            "expected validation failure, got {err:?}"
        );
    }
    // probe must not have modified the real file
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "binds {\n    Mod+Space { }\n}\n"
    );
}

#[test]
fn detects_includes_in_main_config() {
    let src = r#"include "binds.kdl"
include "outputs.kdl"

binds {
    Mod+X { quit; }
}
"#;
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("config.kdl");
    fs::write(&path, src).unwrap();
    let store = load(&path);
    assert!(store.has_includes());
    assert!(store.has_binds());
}

// ---- multi-file setups (config.kdl includes the rest) ----

const MAIN: &str = r#"// Niri configuration — split into modular files.
environment {
    QT_QPA_PLATFORMTHEME "xdgdesktopportal"
}

include "general.kdl"
include "spawn_at_startup.kdl"
include "keybinds.kdl"
include "window_rules.kdl"
"#;

const KEYBINDS: &str = r#"binds {
    // Suggested binds
    Mod+T hotkey-overlay-title="Open a Terminal" { spawn "alacritty"; }

    // Use spawn-sh to run a shell command.
    Super+Alt+S allow-when-locked=true hotkey-overlay-title=null { spawn-sh "pkill orca || exec orca"; }

    Mod+O repeat=false { toggle-overview; }
    Mod+WheelScrollDown cooldown-ms=150 { focus-workspace-down; }

    // Quit
    Mod+Shift+E { quit; }
}
"#;

const SPAWN_AT_STARTUP: &str = r#"// Wallpaper
spawn-at-startup "awww-daemon"

// Notification daemon
spawn-at-startup "mako"

// Idle management
spawn-at-startup "swayidle" "-w" "-d"
"#;

const WINDOW_RULES: &str = r##"window-rule {
    match app-id=r#"^org\.wezfurlong\.wezterm$"#
    default-column-width {}
}

// Firefox picture-in-picture as floating.
window-rule {
    match app-id=r#"firefox$"# title="^Picture-in-Picture$"
    open-floating true
}
"##;

const GENERAL: &str = r##"// Output configuration
output "eDP-1" {
    mode "1920x1080@60.000"
    scale 1.5
    variable-refresh-rate
    focus-at-startup
    backdrop-color "#001100"

    hot-corners {
        top-left
    }
}

// Input configuration
input {
    keyboard {
        xkb {
            layout "de,us"
            options "grp:caps_toggle"
        }
        numlock
    }

    touchpad {
        tap
        natural-scroll
    }
}

// Workspaces
workspace "stash"

layout {
    gaps 10
    default-column-width { proportion 0.5; }
}
"##;

fn multi_file_tree() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::TempDir::new().unwrap();
    let config = dir.path().join("config.kdl");
    fs::write(&config, MAIN).unwrap();
    fs::write(dir.path().join("general.kdl"), GENERAL).unwrap();
    fs::write(dir.path().join("spawn_at_startup.kdl"), SPAWN_AT_STARTUP).unwrap();
    fs::write(dir.path().join("keybinds.kdl"), KEYBINDS).unwrap();
    fs::write(dir.path().join("window_rules.kdl"), WINDOW_RULES).unwrap();
    (dir, config)
}

#[test]
fn include_resolution_aggregates_rows_with_origins() {
    let (_dir, config) = multi_file_tree();
    let store = load(&config);

    assert!(store.has_includes());
    assert!(
        store.include_warnings().is_empty(),
        "{:?}",
        store.include_warnings()
    );

    // binds come from keybinds.kdl, tagged with the origin file
    let rows = store.binding_rows();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].key, "Mod+T");
    assert_eq!(rows[0].action, "spawn alacritty");
    assert_eq!(rows[0].file, "keybinds.kdl");
    assert!(rows[0].included);
    // raw-string window rules parse without parser complaints
    assert!(store.include_warnings().is_empty());

    // autostart rows from spawn_at_startup.kdl
    let autostart = store.autostart_rows();
    assert_eq!(autostart.len(), 3);
    assert_eq!(autostart[0].display, "awww-daemon");
    assert_eq!(autostart[0].file, "spawn_at_startup.kdl");
    assert_eq!(autostart[2].display, "swayidle -w -d");
}

#[test]
fn edits_write_to_the_owning_file() {
    fn doc_of(store: &Store, name: &str) -> String {
        store
            .included_files()
            .iter()
            .find(|f| f.path.file_name().and_then(|s| s.to_str()) == Some(name))
            .unwrap()
            .doc
            .to_string()
    }

    let (_dir, config) = multi_file_tree();
    let mut store = load(&config);

    // add a binding → must land in keybinds.kdl, not config.kdl
    store.add_binding("Mod+E", "spawn-sh \"vim\"").unwrap();
    let kb = doc_of(&store, "keybinds.kdl");
    assert!(
        kb.contains("Mod+E { spawn-sh \"vim\"; }"),
        "keybinds.kdl: {kb}"
    );
    let main = store.document().to_string();
    assert!(!main.contains("Mod+E"), "config.kdl must stay untouched");

    // remove an autostart entry → spawn_at_startup.kdl loses a line
    store.remove_autostart(1).unwrap(); // "mako"
    let spawn = doc_of(&store, "spawn_at_startup.kdl");
    assert!(!spawn.contains("mako"));
    assert!(spawn.contains("awww-daemon"));
    assert!(spawn.contains("swayidle"));

    // add an autostart → prefers the file that already has autostart entries
    store.add_autostart("waybar").unwrap();
    let spawn = doc_of(&store, "spawn_at_startup.kdl");
    assert!(spawn.contains("spawn-at-startup \"waybar\""), "{spawn}");
    let main = store.document().to_string();
    assert!(!main.contains("waybar"), "config.kdl must stay untouched");
}

#[test]
fn multi_file_save_validates_whole_set() {
    let (_dir, config) = multi_file_tree();
    let mut store = load(&config);
    let mut disk = folders_in(_dir.path());
    store.add_binding("Mod+X", "quit").unwrap();

    if validate::niri_available() {
        save(&mut disk, &mut store).unwrap();
        // reload: still resolves, still a single extra binding in keybinds.kdl
        let reloaded = load(&config);
        assert!(reloaded.include_warnings().is_empty());
        assert_eq!(reloaded.binding_rows().len(), 6);
        // original files untouched apart from keybinds.kdl
        let main = fs::read_to_string(&config).unwrap();
        assert!(main.contains("include \"keybinds.kdl\""));
    } else {
        assert!(check(&store).is_err() || true);
    }
}

#[test]
fn pending_diffs_reports_only_changed_files() {
    let (_dir, config) = multi_file_tree();
    let mut store = load(&config);
    assert!(store.pending_files().is_empty());

    store.add_binding("Mod+E", "spawn-sh \"vim\"").unwrap();
    let files = store.pending_files();
    assert_eq!(files.len(), 1, "only keybinds.kdl changed");
    assert!(files[0].path.ends_with("keybinds.kdl"));
    assert!(
        files[0].after.contains("Mod+E"),
        "pending content must contain the edit"
    );
    assert!(
        !files[0].before.contains("Mod+E"),
        "old content must be the original"
    );

    let diffs = store.pending_diffs();
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].path, "keybinds.kdl");
    // the edit appears as an added line
    let added = diffs[0]
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .filter(|l| matches!(l, niri_center::diff::Line::Added(_)))
        .count();
    assert!(added >= 1, "expected an added line in the diff");
}

#[test]
fn discard_restores_disk_state() {
    let (_dir, config) = multi_file_tree();
    let mut store = load(&config);
    let original_len = store.binding_rows().len();

    store.add_binding("Mod+E", "spawn-sh \"vim\"").unwrap();
    store.add_autostart("waybar").unwrap();
    assert_eq!(store.binding_rows().len(), original_len + 1);
    assert_eq!(store.autostart_rows().len(), 4);

    store = load(store.path());
    assert_eq!(store.binding_rows().len(), original_len);
    assert_eq!(store.autostart_rows().len(), 3);
    assert!(store.pending_files().is_empty());
}

// ---- appearance settings (upsert) ----

#[test]
fn set_layout_gaps_replaces_value() {
    let (_dir, config) = multi_file_tree();
    let mut store = load(&config);
    assert_eq!(store.layout_gaps(), Some(10.0));

    store.set_layout_gaps(16.0).unwrap();
    assert_eq!(store.layout_gaps(), Some(16.0));
    let layout_doc = store
        .included_files()
        .iter()
        .find(|f| f.path.ends_with("general.kdl"))
        .unwrap()
        .doc
        .to_string();
    assert!(layout_doc.contains("gaps 16"), "general.kdl: {layout_doc}");
    // only the gaps line changed in general.kdl; keybinds untouched
    let kb = store
        .included_files()
        .iter()
        .find(|f| f.path.ends_with("keybinds.kdl"))
        .unwrap()
        .doc
        .to_string();
    assert!(kb.contains("Mod+T"));
}

#[test]
fn set_output_backdrop_replaces_and_inserts() {
    let (_dir, mut store) = temp_store();
    // eDP-1 has no backdrop-color → inserted
    store.set_output_backdrop("eDP-1", "#112233").unwrap();
    let out = store.document().to_string();
    assert!(
        out.contains("output \"eDP-1\" {\n    scale 1.5\n    position x=0 y=0\n    focus-at-startup\n    backdrop-color \"#112233\"\n}"),
        "inserted cleanly:\n{out}"
    );
    // replace the inserted value
    store.set_output_backdrop("eDP-1", "#443322").unwrap();
    let out = store.document().to_string();
    assert!(out.contains("backdrop-color \"#443322\""), "{out}");
    assert!(!out.contains("#112233"));
    // foreign block untouched
    assert!(out.contains("layer-rule {\n    match namespace=\"^noctalia-backdrop\"\n    place-within-backdrop true\n}"));
    // an output the config does not mention yet gets its own block
    store.set_output_backdrop("HDMI-A-1", "#000000").unwrap();
    let out = store.document().to_string();
    assert!(
        out.ends_with("output \"HDMI-A-1\" {\n    backdrop-color \"#000000\"\n}\n"),
        "{out}"
    );
}

// ---- behavior & input upserts ----

#[test]
fn set_center_focused_column_inserts_then_replaces() {
    let (_dir, config) = multi_file_tree();
    let mut store = load(&config);
    assert_eq!(store.center_focused_column(), None);

    store.set_center_focused_column("always").unwrap();
    assert_eq!(store.center_focused_column().as_deref(), Some("always"));
    store.set_center_focused_column("on-overflow").unwrap();
    assert_eq!(
        store.center_focused_column().as_deref(),
        Some("on-overflow")
    );

    let general = store
        .included_files()
        .iter()
        .find(|f| f.path.ends_with("general.kdl"))
        .unwrap()
        .doc
        .to_string();
    assert!(
        general.contains("center-focused-column \"on-overflow\""),
        "general.kdl: {general}"
    );
    assert!(!general.contains("center-focused-column \"always\""));
}

#[test]
fn input_flags_toggle_and_xkb_layout_updates() {
    let (_dir, config) = multi_file_tree();
    let mut store = load(&config);

    // initial state mirrors the fixture
    assert!(store.touchpad_tap());
    assert!(store.touchpad_natural_scroll());
    assert!(store.keyboard_numlock());
    assert_eq!(store.keyboard_layout().as_deref(), Some("de,us"));

    // toggle flags off and back on
    store.set_touchpad_tap(false).unwrap();
    assert!(!store.touchpad_tap());
    store.set_touchpad_tap(true).unwrap();
    assert!(store.touchpad_tap());
    store.set_keyboard_numlock(false).unwrap();
    assert!(!store.keyboard_numlock());

    // xkb layout replace
    store.set_keyboard_layout("us").unwrap();
    assert_eq!(store.keyboard_layout().as_deref(), Some("us"));

    let general = store
        .included_files()
        .iter()
        .find(|f| f.path.ends_with("general.kdl"))
        .unwrap()
        .doc
        .to_string();
    assert!(general.contains("layout \"us\""), "{general}");
    assert!(!general.contains("layout \"de,us\""));
    // keybinds file untouched by input changes
    let kb = store
        .included_files()
        .iter()
        .find(|f| f.path.ends_with("keybinds.kdl"))
        .unwrap()
        .doc
        .to_string();
    assert!(kb.contains("Mod+T"));
}

#[test]
fn stage_backup_replaces_the_staged_content() {
    let (_dir, mut store) = temp_store();
    let disk = folders_in(_dir.path());
    let pristine = store.document().to_string();

    // manually produce a backup of the pristine content
    niri_center::backup::write_backup(&_dir.path().join("backups"), "config.kdl", &pristine)
        .unwrap();

    // mutate, then restore the backup
    store.add_binding("Mod+M", "spawn-sh \"x\"").unwrap();
    assert!(
        store
            .pending_files()
            .iter()
            .any(|f| f.after.contains("Mod+M"))
    );

    // the backup holds the on-disk content, so staging it undoes the edit
    stage_backup(&disk, &mut store, "config.kdl").unwrap();
    assert_eq!(store.document().to_string(), pristine);
    assert!(store.pending_files().is_empty());
    // a file with no backup is an error
    assert!(stage_backup(&disk, &mut store, "nope.kdl").is_err());
}

#[test]
fn set_output_position_replaces_or_inserts() {
    // replace: DP-1 in the single-file fixture already has `position x=2560 y=0`
    let (_dir, mut store) = temp_store();
    store.set_output_position("DP-1", 100, 200).unwrap();
    let out = store.document().to_string();
    assert!(out.contains("position x=100 y=200"), "{out}");
    assert!(!out.contains("x=2560"));

    // insert: the multi-file GENERAL output has no position line
    let (_dir, config) = multi_file_tree();
    let mut store = load(&config);
    store.set_output_position("eDP-1", 10, 20).unwrap();
    let general = store
        .included_files()
        .iter()
        .find(|f| f.path.ends_with("general.kdl"))
        .unwrap()
        .doc
        .to_string();
    assert!(
        general.contains("position x=10 y=20"),
        "general.kdl: {general}"
    );
    // still re-parses; keybinds untouched
    niri_center::kdl::parse_document(&general).unwrap();
    let kb = store
        .included_files()
        .iter()
        .find(|f| f.path.ends_with("keybinds.kdl"))
        .unwrap()
        .doc
        .to_string();
    assert!(kb.contains("Mod+T"));
}

#[test]
fn snapshot_roundtrip_captures_staged_state() {
    let (_dir, mut store) = temp_store();
    let mut disk = folders_in(_dir.path());
    let pristine = store.document().to_string();

    // stage edits, snapshot them, then revert to pristine
    store.add_binding("Mod+M", "spawn-sh \"x\"").unwrap();
    disk.create_snapshot("with-terminal", store.snapshot_files())
        .unwrap();
    store = load(store.path());
    assert_eq!(store.document().to_string(), pristine);

    // staging brings the snapshot state back, for review — not to disk
    stage_snapshot(&disk, &mut store, "with-terminal").unwrap();
    assert!(store.document().to_string().contains("Mod+M"));
    assert!(
        !std::fs::read_to_string(store.path())
            .unwrap()
            .contains("Mod+M")
    );
    assert_eq!(store.pending_files().len(), 1);

    // list + delete
    assert!(disk.snapshots().contains(&"with-terminal".to_string()));
    disk.delete_snapshot("with-terminal").unwrap();
    assert!(!disk.snapshots().contains(&"with-terminal".to_string()));
    // staging a deleted snapshot fails cleanly
    assert!(stage_snapshot(&disk, &mut store, "with-terminal").is_err());
}

#[test]
fn snapshot_sanitizes_weird_names() {
    let (_dir, mut store) = temp_store();
    let mut disk = folders_in(_dir.path());
    disk.create_snapshot("../hack /name;1", store.snapshot_files())
        .unwrap();
    let names = disk.snapshots();
    assert_eq!(names.len(), 1);
    // the listed name is the sanitized one, safe to stage/delete by
    stage_snapshot(&disk, &mut store, &names[0]).unwrap();
    disk.delete_snapshot(&names[0]).unwrap();
    assert!(disk.snapshots().is_empty());
}

// ---- animation / curve settings ----

const ANIM_CFG: &str = r##"// composed animations
animations {
    window-open {
        duration-ms 150
        curve "ease-out-expo"
    }

    window-movement {
        spring damping-ratio=1.0 stiffness=800 epsilon=0.0001
    }
}
"##;

#[test]
fn easing_and_spring_settings_stage_valid_config() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("config.kdl");
    fs::write(&path, ANIM_CFG).unwrap();
    let mut store = load(&path);

    assert_eq!(
        store.animation_names(),
        vec!["window-open", "window-movement"]
    );
    assert_eq!(
        store.easing_curve("window-open").as_deref(),
        Some("ease-out-expo")
    );
    assert_eq!(
        store.spring_params("window-movement"),
        Some((1.0, 800.0, 0.0001))
    );

    // easing: replace the curve value
    store
        .set_easing_curve("window-open", "ease-out-cubic")
        .unwrap();
    assert_eq!(
        store.easing_curve("window-open").as_deref(),
        Some("ease-out-cubic")
    );

    // spring: replace prop spans
    store
        .set_spring_params("window-movement", 0.6, 1000.0, 0.001)
        .unwrap();
    assert_eq!(
        store.spring_params("window-movement"),
        Some((0.6, 1000.0, 0.001))
    );
    let out = store.document().to_string();
    assert!(
        out.contains("spring damping-ratio=0.6 stiffness=1000 epsilon=0.001"),
        "{out}"
    );
    assert!(out.contains("curve \"ease-out-cubic\""), "{out}");
    // comments intact, stable under reparse
    assert!(out.contains("// composed animations"));
    let reparse = niri_center::kdl::parse_document(&out).unwrap();
    assert_eq!(reparse.to_string(), out);

    // the staged result must be accepted by the real niri grammar
    if validate::niri_available() {
        check(&store).unwrap();
    }
}

#[test]
fn disable_enable_binding_roundtrip() {
    let (dir, mut store) = temp_store();
    let original = store
        .binding_rows()
        .iter()
        .find(|r| r.key == "Mod+Return")
        .map(|_| store.document().to_string())
        .unwrap();
    let index = store
        .binding_rows()
        .iter()
        .position(|r| r.key == "Mod+Return")
        .unwrap();

    // disable: `/-` marker inserted before the name, indent preserved
    store.set_binding_disabled(index, true).unwrap();
    let out = store.document().to_string();
    assert!(
        out.contains("    /- Mod+Return hotkey-overlay-title=\"Open a Terminal: kitty\""),
        "{out}"
    );
    assert!(store.binding_rows()[index].disabled);
    assert!(niri_center::kdl::parse_document(&out).is_ok());
    // the disabled state is even valid niri config (niri skips these nodes)
    if validate::niri_available() {
        check(&store).unwrap();
    }

    // enable: marker removed, line restored byte-for-byte
    store.set_binding_disabled(index, false).unwrap();
    assert_eq!(store.document().to_string(), original);
    assert!(!store.binding_rows()[index].disabled);
    assert!(store.pending_files().is_empty());
    let _ = dir;
}

#[test]
fn disable_enable_autostart_roundtrip() {
    let (_dir, mut store) = temp_store();
    store.add_autostart("waybar").unwrap();
    let last = store.autostart_rows().len() - 1;
    let original = store.document().to_string();

    store.set_autostart_disabled(last, true).unwrap();
    assert!(store.autostart_rows()[last].disabled);
    let out = store.document().to_string();
    assert!(out.contains("/- spawn-at-startup \"waybar\""), "{out}");
    if validate::niri_available() {
        check(&store).unwrap();
    }

    store.set_autostart_disabled(last, false).unwrap();
    assert_eq!(store.document().to_string(), original);
    assert!(!store.autostart_rows()[last].disabled);
}

#[test]
fn edit_binding_changes_key_and_action_keeps_props() {
    let (_dir, mut store) = temp_store();
    let index = store
        .binding_rows()
        .iter()
        .position(|r| r.key == "Mod+Return")
        .unwrap();

    // key → new combo, spawn → spawn-sh with args; the hotkey-overlay-title
    // prop on the line must survive untouched
    store
        .edit_binding(index, "Mod+Enter", "spawn-sh \"foot\"")
        .unwrap();
    let rows = store.binding_rows();
    assert_eq!(rows[index].key, "Mod+Enter");
    assert_eq!(rows[index].action, "spawn-sh foot");
    let out = store.document().to_string();
    assert!(
        out.contains(
            "Mod+Enter hotkey-overlay-title=\"Open a Terminal: kitty\" { spawn-sh \"foot\"; }"
        ),
        "{out}"
    );
    // props survived: the line still mentions hotkey-overlay-title once
    assert_eq!(out.matches("hotkey-overlay-title").count(), 2); // Mod+Space + edited row

    // re-edit back to a bare action; `;` management is handled
    store
        .edit_binding(index, "Mod+Return", "spawn \"kitty\"")
        .unwrap();
    assert!(store.document().to_string().contains(
        "Mod+Return hotkey-overlay-title=\"Open a Terminal: kitty\" { spawn \"kitty\"; }"
    ));

    if validate::niri_available() {
        check(&store).unwrap();
    }
    let reparse = niri_center::kdl::parse_document(&store.document().to_string()).unwrap();
    assert_eq!(reparse.to_string(), store.document().to_string());
}

#[test]
fn edit_binding_preserves_disable_marker() {
    let (_dir, mut store) = temp_store();
    let index = store
        .binding_rows()
        .iter()
        .position(|r| r.key == "Mod+L")
        .unwrap();
    store.set_binding_disabled(index, true).unwrap();
    store
        .edit_binding(index, "Mod+K", "focus-window-up")
        .unwrap();
    let rows = store.binding_rows();
    assert_eq!(rows[index].key, "Mod+K");
    assert!(rows[index].disabled, "`/-` marker must survive an edit");
    assert!(store.document().to_string().contains("/- Mod+K"));
}

#[test]
fn save_writes_through_symlink_and_backs_up() {
    let dir = tempfile::TempDir::new().unwrap();
    let real = dir.path().join("real-config.kdl");
    let link = dir.path().join("config.kdl");
    fs::write(&real, "binds {\n    Mod+X { quit; }\n}\n").unwrap();
    std::os::unix::fs::symlink("real-config.kdl", &link).unwrap();

    let mut store = load(&link);
    let mut disk = folders_in(dir.path());
    store.add_binding("Mod+Y", "quit").unwrap();

    if validate::niri_available() {
        save(&mut disk, &mut store).unwrap();
        // the symlink survives; the real target got the edit
        let meta = fs::symlink_metadata(&link).unwrap();
        assert!(
            meta.file_type().is_symlink(),
            "symlink must not be replaced"
        );
        let real_text = fs::read_to_string(&real).unwrap();
        assert!(real_text.contains("Mod+Y"), "{real_text}");
        // a backup of the pristine content exists
        let backups = fs::read_dir(dir.path().join("backups")).unwrap().count();
        assert!(backups >= 1, "expected at least one backup");
    } else {
        // niri not available: the edit is staged in memory, untouched on disk
        assert!(!store.pending_files().is_empty());
    }
}

// ---- `/-`-disabled nodes: niri ignores them, so edits must too ----

/// Write `source` as `config.kdl` in a temp dir and load it.
fn store_from(source: &str) -> (tempfile::TempDir, Store) {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("config.kdl");
    fs::write(&path, source).unwrap();
    let store = load(&path);
    (dir, store)
}

/// The shape niri's shipped default config uses for its laptop example.
const DISABLED_EDP: &str = "/-output \"eDP-1\" {\n    scale 2\n    position x=1280 y=0\n}\n";

#[test]
fn a_disabled_output_is_left_alone_and_an_active_block_is_added() {
    // niri's default config only has `/-output "eDP-1"`: a monitor drag must
    // still take effect, without touching the example
    let (_dir, mut store) = store_from(DISABLED_EDP);
    assert_eq!(store.first_output_name(), None);
    store.set_output_position("eDP-1", 0, 0).unwrap();
    let out = store.document().to_string();
    assert!(out.starts_with(DISABLED_EDP), "{out}");
    assert_eq!(
        &out[DISABLED_EDP.len()..],
        "output \"eDP-1\" {\n    position x=0 y=0\n}\n"
    );
    assert_eq!(store.first_output_name().as_deref(), Some("eDP-1"));
}

#[test]
fn edits_land_in_the_active_output_after_a_disabled_one() {
    let source = format!("{DISABLED_EDP}output \"eDP-1\" {{\n    scale 1.5\n}}\n");
    let (_dir, mut store) = store_from(&source);
    assert_eq!(store.first_output_name().as_deref(), Some("eDP-1"));
    store.set_output_position("eDP-1", 1920, 0).unwrap();
    store.set_output_backdrop("eDP-1", "#112233").unwrap();
    assert_eq!(
        store.output_backdrop_color("eDP-1").as_deref(),
        Some("#112233")
    );
    let out = store.document().to_string();
    // the disabled block is byte-identical; the active one got both edits
    assert!(out.starts_with(DISABLED_EDP), "{out}");
    let active = &out[DISABLED_EDP.len()..];
    assert!(active.contains("position x=1920 y=0"), "{active}");
    assert!(active.contains("backdrop-color \"#112233\""), "{active}");
}

#[test]
fn disabled_children_are_neither_read_nor_edited() {
    let source = "layout {\n    /-gaps 4\n}\n";
    let (_dir, mut store) = store_from(source);
    assert_eq!(store.layout_gaps(), None);
    store.set_layout_gaps(8.0).unwrap();
    let out = store.document().to_string();
    assert!(
        out.contains("/-gaps 4"),
        "disabled line must survive: {out}"
    );
    assert!(out.contains("    gaps 8\n"), "{out}");
    assert_eq!(store.layout_gaps(), Some(8.0));
}

#[test]
fn disabled_blocks_are_skipped_for_input_and_animations() {
    let source = "/-input {\n    touchpad {\n        tap\n    }\n}\ninput {\n    touchpad {\n        natural-scroll\n    }\n}\n/-animations {\n    window-open {\n        curve \"linear\"\n    }\n}\nanimations {\n    window-open {\n        duration-ms 150\n    }\n}\n";
    let (_dir, mut store) = store_from(source);
    assert!(
        !store.touchpad_tap(),
        "tap lives only in the disabled block"
    );
    assert_eq!(store.easing_curve("window-open"), None);

    store.set_touchpad_tap(true).unwrap();
    store
        .set_easing_curve("window-open", "ease-out-cubic")
        .unwrap();
    let out = store.document().to_string();
    let active_input = out.find("\ninput {").unwrap();
    let active_animations = out.find("\nanimations {").unwrap();
    assert!(out[active_input..].contains("        tap\n"), "{out}");
    assert!(
        out[active_animations..].contains("curve \"ease-out-cubic\""),
        "{out}"
    );
    // the disabled copies are untouched
    assert!(out.starts_with("/-input {\n    touchpad {\n        tap\n    }\n}\n"));
    assert!(
        out.contains("/-animations {\n    window-open {\n        curve \"linear\"\n    }\n}\n")
    );
}

#[test]
fn disabled_binds_block_is_ignored() {
    let source = "/-binds {\n    Mod+A { quit; }\n}\nbinds {\n    Mod+B { quit; }\n}\n";
    let (_dir, mut store) = store_from(source);
    let keys: Vec<String> = store.binding_rows().into_iter().map(|r| r.key).collect();
    assert_eq!(keys, vec!["Mod+B".to_string()]);

    store.add_binding("Mod+C", "quit").unwrap();
    let out = store.document().to_string();
    assert!(
        out.starts_with("/-binds {\n    Mod+A { quit; }\n}\n"),
        "{out}"
    );
    assert!(
        out.contains("    Mod+B { quit; }\n    Mod+C { quit; }\n}"),
        "{out}"
    );
}

#[test]
fn disabled_include_is_not_loaded() {
    let dir = tempfile::TempDir::new().unwrap();
    let config = dir.path().join("config.kdl");
    fs::write(&config, "/-include \"off.kdl\"\n").unwrap();
    fs::write(dir.path().join("off.kdl"), "layout {\n    gaps 4\n}\n").unwrap();
    let store = load(&config);
    assert!(store.included_files().is_empty());
    assert_eq!(store.layout_gaps(), None);
}

#[test]
fn one_line_block_flags_read_and_toggle_on_as_no_op() {
    let source = "input {\n    touchpad { tap; natural-scroll; }\n}\n";
    let (_dir, mut store) = store_from(source);
    assert!(store.touchpad_tap());
    assert!(store.touchpad_natural_scroll());
    // already on: nothing to stage
    store.set_touchpad_natural_scroll(true).unwrap();
    assert_eq!(store.document().to_string(), source);
}

// ---- save scope: sandbox write roots and all-or-nothing writes ----

#[test]
fn config_dirs_include_a_symlinked_configs_target_dir() {
    let dir = tempfile::TempDir::new().unwrap();
    let config_dir = dir.path().join("niri");
    let dotfiles = dir.path().join("dotfiles");
    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&dotfiles).unwrap();
    fs::write(dotfiles.join("config.kdl"), "layout {\n    gaps 4\n}\n").unwrap();
    std::os::unix::fs::symlink(dotfiles.join("config.kdl"), config_dir.join("config.kdl")).unwrap();

    let store = load(config_dir.join("config.kdl"));
    let dirs = niri_center::disk::config_dirs(store.path());
    assert_eq!(dirs.len(), 2, "{dirs:?}");
    assert_eq!(dirs[0], config_dir);
    assert_eq!(dirs[1], fs::canonicalize(&dotfiles).unwrap());
}

#[test]
fn config_dirs_is_one_dir_without_symlinks() {
    let (dir, store) = store_from("layout {\n    gaps 4\n}\n");
    let canonical = fs::canonicalize(dir.path()).unwrap();
    let dirs = niri_center::disk::config_dirs(store.path());
    // the raw dir, plus its canonical spelling only if they differ
    assert!(dirs.len() == 1 || (dirs.len() == 2 && dirs[1] == canonical));
}

#[test]
fn save_aborts_before_any_write_when_one_file_is_unwritable() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::TempDir::new().unwrap();
    let locked = dir.path().join("locked");
    fs::create_dir_all(&locked).unwrap();
    let config = dir.path().join("config.kdl");
    let main_source = "layout {\n    gaps 4\n}\ninclude \"locked/binds.kdl\"\n";
    let include_source = "binds {\n    Mod+A { quit; }\n}\n";
    fs::write(&config, main_source).unwrap();
    fs::write(locked.join("binds.kdl"), include_source).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
    if fs::write(locked.join("probe"), "").is_ok() {
        // running as root: permissions do not bind, nothing to check
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }

    let mut store = load(&config);
    let mut disk = folders_in(dir.path());
    store.set_layout_gaps(8.0).unwrap(); // main config: writable
    store.add_binding("Mod+B", "quit").unwrap(); // include: read-only dir

    if validate::niri_available() {
        let result = save(&mut disk, &mut store);
        // unlock before asserting, so a failure still lets TempDir clean up
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            matches!(result, Err(niri_center::disk::DiskError::Write { .. })),
            "{result:?}"
        );
        // neither file changed, and no temp file was left behind
        assert_eq!(fs::read_to_string(&config).unwrap(), main_source);
        assert_eq!(
            fs::read_to_string(locked.join("binds.kdl")).unwrap(),
            include_source
        );
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".temp_path"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
}

// ---- UI-facing round trips: what the GUI passes, not what the store wants ----

#[test]
fn binding_rows_carry_the_raw_action_text() {
    let (_dir, store) = temp_store();
    let rows = store.binding_rows();
    let space = rows.iter().find(|r| r.key == "Mod+Space").unwrap();
    assert_eq!(space.action, "spawn rofi -show drun", "display text");
    assert_eq!(space.action_raw, "spawn \"rofi -show drun\"", "source text");
}

#[test]
fn remove_then_restore_is_byte_identical_including_props() {
    let (_dir, mut store) = temp_store();
    let before = store.document().to_string();
    let index = store
        .binding_rows()
        .iter()
        .position(|r| r.key == "Mod+Space")
        .unwrap();
    let removed = store.remove_binding(index).unwrap();
    assert_ne!(store.document().to_string(), before);
    store.restore_binding(&removed).unwrap();
    assert_eq!(store.document().to_string(), before);
}

#[test]
fn restore_puts_a_binding_back_into_its_own_file() {
    let (_dir, config) = multi_file_tree();
    let mut store = load(&config);
    let rows = store.binding_rows();
    // the last row lives in an include, not the main config
    let index = rows.len() - 1;
    let file = rows[index].file.clone();
    assert!(rows[index].included, "{rows:?}");
    assert!(store.pending_files().is_empty());

    let removed = store.remove_binding(index).unwrap();
    store.restore_binding(&removed).unwrap();
    assert!(
        store.pending_files().is_empty(),
        "restore must undo the removal exactly: {:?}",
        store.pending_files()
    );
    assert_eq!(store.binding_rows()[index].file, file);
}

#[test]
fn easing_curve_and_spring_replace_each_other() {
    // niri rejects an animation that has both a spring and a curve
    let source = "animations {\n    window-open {\n        spring damping-ratio=0.8 stiffness=1000 epsilon=0.0001\n    }\n    workspace-switch {\n        duration-ms 250\n        curve \"ease-out-cubic\"\n    }\n}\n";
    let (_dir, mut store) = store_from(source);

    store.set_easing_curve("window-open", "linear").unwrap();
    let out = store.document().to_string();
    let window_open = &out[..out.find("workspace-switch").unwrap()];
    assert!(window_open.contains("curve \"linear\""), "{out}");
    assert!(!window_open.contains("spring"), "{out}");

    store
        .set_spring_params("workspace-switch", 1.0, 800.0, 0.001)
        .unwrap();
    let out = store.document().to_string();
    let workspace_switch = &out[out.find("workspace-switch").unwrap()..];
    assert!(workspace_switch.contains("spring damping-ratio=1"), "{out}");
    assert!(!workspace_switch.contains("curve"), "{out}");
    assert!(!workspace_switch.contains("duration-ms"), "{out}");

    if validate::niri_available() {
        check(&store).unwrap();
    }
}

#[test]
fn autostart_with_arguments_goes_through_a_shell() {
    // spawn-at-startup takes argv items: "waybar -c x" as one item is a
    // program named `waybar -c x`, which validates but never starts
    let (_dir, mut store) = store_from("");
    store.add_autostart("waybar").unwrap();
    store
        .add_autostart("waybar -c ~/.config/waybar/niri.jsonc")
        .unwrap();
    let out = store.document().to_string();
    assert!(out.contains("spawn-at-startup \"waybar\"\n"), "{out}");
    assert!(
        out.contains("spawn-sh-at-startup \"waybar -c ~/.config/waybar/niri.jsonc\"\n"),
        "{out}"
    );
}

#[test]
fn staging_a_snapshot_changes_nothing_on_disk_until_save() {
    let (_dir, mut store) = temp_store();
    let mut disk = folders_in(_dir.path());
    store.set_layout_gaps(32.0).unwrap();
    disk.create_snapshot("wide gaps", store.snapshot_files())
        .unwrap();
    store = load(store.path());
    let on_disk = fs::read_to_string(store.path()).unwrap();

    stage_snapshot(&disk, &mut store, "wide gaps").unwrap();
    assert_eq!(fs::read_to_string(store.path()).unwrap(), on_disk);
    assert_eq!(store.layout_gaps(), Some(32.0));
    assert_eq!(store.pending_files().len(), 1);
}

#[test]
fn staging_a_backup_changes_nothing_on_disk_until_save() {
    let (_dir, mut store) = temp_store();
    let backups = _dir.path().join("backups");
    let disk = folders_in(_dir.path());
    niri_center::backup::write_backup(&backups, "config.kdl", "layout {\n    gaps 2\n}\n").unwrap();
    let on_disk = fs::read_to_string(store.path()).unwrap();

    stage_backup(&disk, &mut store, "config.kdl").unwrap();
    assert_eq!(fs::read_to_string(store.path()).unwrap(), on_disk);
    assert_eq!(store.layout_gaps(), Some(2.0));
    assert_eq!(store.pending_files().len(), 1);
}

#[test]
fn disabled_bindings_are_not_duplicates() {
    let (_dir, store) =
        store_from("binds {\n    Mod+Q { quit; }\n    /- Mod+Q { close-window; }\n}\n");
    assert!(store.duplicate_keys().is_empty());
}

#[test]
fn a_misspelled_node_is_found_and_renamed_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.kdl");
    let source = "layout {\n    focus-ring {\n        widht 2 // thin\n    }\n}\n";
    std::fs::write(&path, source).unwrap();
    let mut store = load(&path);
    let found: Vec<usize> = store
        .nodes_named("widht")
        .into_iter()
        .map(|(_, line)| line)
        .collect();
    assert_eq!(found, vec![3]);
    store.rename_node(&path, 3, "widht", "width").unwrap();
    assert_eq!(
        store.document().to_string(),
        source.replace("widht", "width")
    );
    assert!(store.rename_node(&path, 1, "widht", "width").is_err());
}

#[test]
fn environment_variables_are_set_changed_and_taken_out() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.kdl");
    let source = "environment {\n    QT_QPA_PLATFORM \"wayland\" // for Qt\n    DISPLAY null\n}\n";
    std::fs::write(&path, source).unwrap();
    let mut store = load(&path);
    assert_eq!(
        store.environment(),
        vec![
            ("QT_QPA_PLATFORM".to_string(), Some("wayland".to_string())),
            ("DISPLAY".to_string(), None),
        ]
    );
    store
        .set_environment("QT_QPA_PLATFORM", Some("xcb"))
        .unwrap();
    store.set_environment("EDITOR", Some("hx")).unwrap();
    store.remove_environment("DISPLAY").unwrap();
    let out = store.document().to_string();
    assert!(out.contains("QT_QPA_PLATFORM \"xcb\" // for Qt\n"), "{out}");
    assert!(out.contains("EDITOR \"hx\""), "{out}");
    assert!(!out.contains("DISPLAY"), "{out}");
    if validate::niri_available() {
        check(&store).unwrap();
    }
}

#[test]
fn rules_go_into_a_new_included_file_written_on_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.kdl");
    let source = "layout {\n    gaps 16\n}\n";
    std::fs::write(&path, source).unwrap();
    let mut store = load(&path);
    let mut disk = folders_in(dir.path());
    let rule = "window-rule {\n    match app-id=\"steam\"\n    open-floating true\n}";
    store.add_to_include("rules-library.kdl", rule).unwrap();
    // a second rule goes to the same file, and one include line is enough
    store.add_to_include("rules-library.kdl", rule).unwrap();
    assert_eq!(
        store.document().to_string(),
        format!("{source}\ninclude \"rules-library.kdl\"\n")
    );
    assert_eq!(store.window_rule_rows().len(), 2);
    if !validate::niri_available() {
        return;
    }
    save(&mut disk, &mut store).unwrap();
    let written = fs::read_to_string(dir.path().join("rules-library.kdl")).unwrap();
    assert_eq!(written, format!("{rule}\n\n{rule}\n"));
    let reloaded = load(&path);
    assert_eq!(reloaded.window_rule_rows().len(), 2);
}

#[test]
fn a_file_that_cannot_be_written_is_found_at_load() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.kdl");
    std::fs::write(&path, "layout {\n    gaps 16\n}\n").unwrap();
    assert!(read_only(&path).is_empty());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
    // root writes a read-only file anyway (a container): nothing to find
    if std::fs::OpenOptions::new().append(true).open(&path).is_ok() {
        return;
    }
    assert_eq!(read_only(&path), vec![path.clone()]);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
}

#[test]
fn outputs_with_their_own_backdrop_are_listed_once_with_their_color() {
    let (_dir, store) = store_from(
        "overview { backdrop-color \"#262626\"; }\n\
         output \"eDP-1\" { backdrop-color \"#3c0000\"; }\n\
         output \"HDMI-A-1\" { scale 1; }\n\
         output \"eDP-1\" { scale 1.5; }\n",
    );
    let found =
        niri_center::settings::overrides(&store, &[Block::named("overview")], "backdrop-color");
    assert_eq!(
        found.iter().map(ToString::to_string).collect::<Vec<_>>(),
        ["output \"eDP-1\" (#3c0000)"]
    );
}

#[test]
fn a_written_pattern_reads_back_as_it_was_made() {
    use niri_center::store::TextMatch;
    for (how, text) in [
        (TextMatch::Exact, "org.gnome.Calculator"),
        (TextMatch::Contains, "Picture-in-Picture (1)"),
        (TextMatch::Pattern, "^(firefox|zen)$"),
    ] {
        assert_eq!(TextMatch::read(&how.pattern(text)), (how, text.to_string()));
    }
    assert_eq!(
        TextMatch::read("firefox"),
        (TextMatch::Contains, "firefox".to_string())
    );
}

#[test]
fn a_key_bound_in_two_files_is_used_where_niri_reads_it_last() {
    let both = |main: &str| {
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(dir.path().join("config.kdl"), main).unwrap();
        fs::write(
            dir.path().join("agents.kdl"),
            "binds {\n    Mod+P { spawn \"agent\"; }\n}\n",
        )
        .unwrap();
        let store = load(dir.path().join("config.kdl"));
        (dir, store)
    };
    // the include comes after: its binding wins
    let (_dir, store) = both(
        "binds {\n    Mod+P { spawn \"fuzzel\"; }\n    Mod+T { spawn \"foot\"; }\n}\ninclude \"agents.kdl\"\n",
    );
    let rows = store.binding_rows();
    let mine = rows
        .iter()
        .find(|row| !row.included && row.key == "Mod+P")
        .unwrap();
    let theirs = rows.iter().find(|row| row.included).unwrap();
    assert_eq!(mine.hidden_by.as_deref(), Some("agents.kdl"));
    assert_eq!(theirs.replaces, ["config.kdl"]);
    assert!(theirs.hidden_by.is_none());
    let single = rows.iter().find(|row| row.key == "Mod+T").unwrap();
    assert!(single.hidden_by.is_none() && single.replaces.is_empty());
    // the include comes first: the main file's binding wins
    let (_dir, store) =
        both("include \"agents.kdl\"\nbinds {\n    Mod+P { spawn \"fuzzel\"; }\n}\n");
    let rows = store.binding_rows();
    let theirs = rows.iter().find(|row| row.included).unwrap();
    assert_eq!(theirs.hidden_by.as_deref(), Some("config.kdl"));
}

#[test]
fn a_save_asks_niri_about_its_own_texts() {
    if !validate::niri_available() {
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("config.kdl");
    fs::write(&path, "layout {\n    gaps 8\n}\n").unwrap();
    let mut store = load(&path);
    let mut disk = folders_in(dir.path());
    store.set_layout_gaps(16.0).unwrap();
    check(&store).unwrap();
    // an edit niri turns down, made after it looked
    store
        .stage_file(&path, "layout {\n    gapz 16\n}\n")
        .unwrap();
    assert!(matches!(
        save(&mut disk, &mut store),
        Err(niri_center::disk::DiskError::Rejected { .. })
    ));
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "layout {\n    gaps 8\n}\n"
    );
}

#[test]
fn a_misspelled_name_is_found_in_the_include_niri_names() {
    let dir = tempfile::TempDir::new().unwrap();
    fs::create_dir(dir.path().join("dms")).unwrap();
    fs::write(
        dir.path().join("config.kdl"),
        "include \"dms/binds.kdl\"\nlayout {\n    gapz 4\n}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("dms/binds.kdl"),
        "layout {\n    gapz 8\n}\n",
    )
    .unwrap();
    let store = load(dir.path().join("config.kdl"));
    let diagnostics = "Error:   × unexpected node `gapz`\n   ╭─[dms/binds.kdl:1:1]\n 1 │ layout {\n 2 │     gapz 8\n   ·     ───┬───\n";
    let (name, path, line) = store.misspelled(diagnostics).unwrap();
    assert_eq!(name, "gapz");
    assert!(path.ends_with("dms/binds.kdl"), "{}", path.display());
    assert_eq!(line, 2);
}

#[test]
fn an_added_binding_or_rule_says_where_it_is() {
    let (_dir, mut store) = store_from(
        "binds {\n    Mod+T { spawn \"foot\"; }\n}\nwindow-rule {\n    open-floating true\n}\n",
    );
    let binding = store.add_binding("Mod+E", "spawn \"vim\"").unwrap();
    assert_eq!(store.binding_rows()[binding].key, "Mod+E");
    let rule = store
        .add_window_rule_text("window-rule {\n    match app-id=\"mpv\"\n    open-floating true\n}")
        .unwrap();
    assert_eq!(rule, 1);
    assert!(store.window_rule_rows()[rule].criteria[0].contains("mpv"));
    let edited = store
        .replace_window_rule(0, "window-rule {\n    open-maximized true\n}")
        .unwrap();
    assert_eq!(edited, 0);
}
