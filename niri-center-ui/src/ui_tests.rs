//! Headless UI tests: the real `view`/`update` driven through iced_test's
//! simulator: no window, no compositor, no IPC.
//!
//! Assertions are behavioural (what a click stages, what text a page shows)
//! or computed properties (contrast ratios, copy rules); rendered PNGs are
//! for human review only and never compared as truth.
//!
//! These tests run inside the developer's niri session, so they never take a
//! path that talks to niri synchronously: `update` returns IPC work as Tasks,
//! which the simulator does not run, and no test ends a preview through
//! Apply, Discard or closing.

use std::path::PathBuf;

use iced::Size;
use iced_test::Error;
use iced_test::simulator::Simulator;
use niri_center::animations::{Animation, Curve};
use niri_center::disk::actor::{Actor, Command, Reply};
use niri_center::disk::{Disk, Folders};
use niri_center::session::{SaveState, SaveWhen};
use niri_center::settings::{Coordinate, DraftKey, OutputSetting, Setting};
use niri_center::store::{RuleProperty, Store, TextMatch};

use niri_center::niri::{OpenWindow, Running};
use niri_center::outputs::{MonitorInfo, OutputInfo};
use niri_center::page::Page;
use niri_center::session::{Platform, Problem, TRIAL_SECONDS, Version};
use niri_center::settings::Edit;

use crate::app::App;
use crate::message::Message;
use crate::pages::{animations, displays, environment, files, keybinds, rules};
use crate::theme;
use crate::update::step;
use crate::view::view;

const UI_FIXTURE: &str = include_str!("../tests/fixtures/ui.kdl");

/// A desktop-ish window, a half-width column on a 1920 px output and a
/// third-width one: niri users tile at all three.
const WIDE: Size = Size::new(1280.0, 800.0);
const NARROW: Size = Size::new(900.0, 760.0);
const THIRD: Size = Size::new(620.0, 900.0);
/// Clicks happen in a tall desktop window, so a row far down a long page
/// is on screen without scrolling.
const TALL: Size = Size::new(1280.0, 4000.0);

fn mode(value: &str, width: u32, height: u32) -> niri_center::outputs::Mode {
    niri_center::outputs::Mode {
        value: value.to_string(),
        label: format!("{width} × {height}"),
        choice: format!("{width} × {height}"),
        width,
        height,
    }
}

/// An app with `source` loaded as its config (from a temp file), the way
/// `Message::Loaded` would deliver it, minus the sandbox.
fn loaded_app(source: &str) -> (tempfile::TempDir, App) {
    loaded_app_with(source, &[])
}

/// [`loaded_app`] with more files beside config.kdl, for its includes.
fn loaded_app_with(source: &str, files: &[(&str, &str)]) -> (tempfile::TempDir, App) {
    let dir = tempfile::TempDir::new().unwrap();
    for (name, text) in files {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    let path = dir.path().join("config.kdl");
    std::fs::write(&path, source).unwrap();
    let actor = test_actor(dir.path());
    let store = load(&path);
    let outputs = vec![
        OutputInfo {
            name: "DP-1".into(),
            description: "Dell Inc. U2720Q".into(),
            modes: vec![
                mode("3840x2160@59.997", 3840, 2160),
                mode("2560x1440@165.001", 2560, 1440),
            ],
            preferred: Some(0),
            current: Some(1),
            scale: 1.0,
            vrr_supported: true,
            vrr_enabled: true,
            rect: Some(MonitorInfo {
                x: 1707.0,
                y: 0.0,
                width: 2560.0,
                height: 1440.0,
            }),
        },
        OutputInfo {
            name: "eDP-1".into(),
            description: "BOE 0x0BCA".into(),
            modes: vec![mode("2560x1600@60.002", 2560, 1600)],
            preferred: Some(0),
            current: Some(0),
            scale: 1.5,
            vrr_supported: false,
            vrr_enabled: false,
            rect: Some(MonitorInfo {
                x: 0.0,
                y: 0.0,
                width: 1707.0,
                height: 1067.0,
            }),
        },
    ];
    let mut app = App {
        displays: displays::State {
            monitors: niri_center::outputs::rects(&outputs),
            ..displays::State::default()
        },
        outputs: outputs.clone(),
        running: Some(Running {
            outputs,
            windows: vec![
                OpenWindow {
                    id: 1,
                    app_id: "kitty".into(),
                    title: "~/code".into(),
                    floating: false,
                    focused: true,
                },
                OpenWindow {
                    id: 2,
                    app_id: "org.gnome.Calculator".into(),
                    title: "Calculator".into(),
                    floating: true,
                    focused: false,
                },
            ],
            layers: vec!["waybar on DP-1 (Top)".into()],
        }),
        test_actor: Some(actor),
        ..App::default()
    };
    load_into(&mut app, store);
    (dir, app)
}

fn ui(app: &App, size: Size) -> Simulator<'_, Message> {
    Simulator::with_size(crate::settings(), size, view(app))
}

/// Run every message a simulated interaction produced through `update`,
/// then hand the disk actor what the session asked of it and feed its
/// answers back, until both are quiet.
fn apply(app: &mut App, messages: impl IntoIterator<Item = Message>) {
    for message in messages {
        let _ = step(app, message);
        settle(app);
    }
}

/// The disk actor's work, done now: commands in, replies back as messages.
fn settle(app: &mut App) {
    loop {
        let commands = app.session.take_commands();
        if commands.is_empty() {
            return;
        }
        let Some(actor) = app.test_actor.as_mut() else {
            return;
        };
        let replies: Vec<Reply> = commands
            .into_iter()
            .flat_map(|command| actor.handle(command))
            .collect();
        for reply in replies {
            let _ = step(app, Message::Disk(Box::new(reply)));
        }
    }
}

/// The real files under `dir`, with backups, snapshots and nothing else
/// there; the history kept in memory.
fn test_actor(dir: &std::path::Path) -> Actor {
    Actor::new(Disk::files(Folders::under(dir)), None)
}

/// Hand `app` a config, as the disk actor's answer at start does.
fn load_into(app: &mut App, store: Store) {
    app.session.loaded(store);
    app.refresh_rows();
}

/// A disk that only reads: it writes nothing, so its folders need not
/// exist.
fn reader() -> Disk {
    Disk::files(Folders::under(std::path::Path::new("/nonexistent")))
}

/// The config at `path`, read from disk.
fn load(path: impl AsRef<std::path::Path>) -> Store {
    reader().load(path.as_ref()).unwrap().store
}

fn goto(app: &mut App, page: Page) {
    apply(app, [Message::Page(page)]);
}

fn store(app: &App) -> &Store {
    app.session.store.as_ref().unwrap()
}

fn document(app: &App) -> String {
    store(app).document().to_string()
}

/// Click the widget showing `label` and run the resulting messages.
fn click(app: &mut App, label: &str) {
    let messages: Vec<Message> = {
        let mut ui = ui(app, TALL);
        if let Err(error) = ui.click(label) {
            panic!("cannot click {label:?}: {error}");
        }
        ui.into_messages().collect()
    };
    apply(app, messages);
}

fn shows(app: &App, size: Size, label: &str) -> bool {
    ui(app, size).find(label).is_ok()
}

fn binding_index(app: &App, key: &str) -> usize {
    store(app)
        .binding_rows()
        .iter()
        .position(|row| row.key == key)
        .unwrap()
}

/// Let the wait before saving pass: what is staged is saved, as the app
/// does a moment after the last edit.
fn wait_for_save(app: &mut App) {
    app.session.hurry();
    apply(app, [Message::SaveTick]);
}

/// The open binding form.
fn binding_form(app: &mut App) -> &mut niri_center::forms::BindingForm {
    app.keybinds
        .form
        .as_mut()
        .expect("the binding form is open")
}

fn status_text(app: &App) -> String {
    app.status
        .as_ref()
        .map(|status| status.text.clone())
        .unwrap_or_default()
}

fn assert_niri_accepts(app: &App) {
    let probes = tempfile::tempdir().unwrap();
    if niri_center::validate::niri_available()
        && let Err(e) = Disk::files(Folders::under(probes.path())).check(&store(app).job())
    {
        panic!("niri rejects:\n{e}\n{}", document(app));
    }
}

/// Take away the pictures an earlier run rendered: iced only writes a
/// picture where none is, so old ones would stay.
fn clear_shots(out: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(out) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with("-tiny-skia.png") || name.ends_with("-wgpu.png") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Render every screen to `$NIRI_CENTER_UI_SHOTS` (default: target/ui-shots)
/// for a human to look at. Not an assertion: run with `--ignored`.
#[test]
#[ignore = "renders PNGs for human review; run with --ignored"]
fn render_screens_for_review() -> Result<(), Error> {
    let out = std::env::var_os("NIRI_CENTER_UI_SHOTS")
        .map_or_else(|| PathBuf::from("../target/ui-shots"), PathBuf::from);
    clear_shots(&out);
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    for (mode_name, mode) in [
        ("dark", iced::theme::Mode::Dark),
        ("light", iced::theme::Mode::Light),
    ] {
        app.mode = Some(mode);
        let theme = theme::for_mode(app.mode, false);
        let sizes: &[(&str, Size)] = if mode_name == "dark" {
            &[("wide", WIDE), ("narrow", NARROW), ("third", THIRD)]
        } else {
            &[("wide", WIDE)]
        };
        for (size_name, size) in sizes {
            app.window_width = size.width;
            for page in Page::ALL {
                goto(&mut app, page);
                let shot = ui(&app, *size).snapshot(&theme)?;
                let _ = shot.matches_image(
                    out.join(format!("{mode_name}-{size_name}-{}.png", page.slug())),
                )?;
            }
        }
    }
    // the high-contrast variants, for the pages with the most controls
    for (mode_name, mode) in [
        ("dark", iced::theme::Mode::Dark),
        ("light", iced::theme::Mode::Light),
    ] {
        app.window_width = WIDE.width;
        let theme = theme::for_mode(Some(mode), true);
        for page in [Page::Keybinds, Page::Layout, Page::Windows] {
            goto(&mut app, page);
            let shot = ui(&app, WIDE).snapshot(&theme)?;
            let _ = shot.matches_image(
                out.join(format!("{mode_name}-contrast-wide-{}.png", page.slug())),
            )?;
        }
    }
    app.mode = Some(iced::theme::Mode::Dark);
    app.window_width = WIDE.width;
    let theme = theme::for_mode(app.mode, false);
    let shoot = |app: &App, name: &str| -> Result<(), Error> {
        let _ = ui(app, WIDE)
            .snapshot(&theme)?
            .matches_image(out.join(format!("dark-wide-{name}.png")))?;
        Ok(())
    };
    // the README's Layout screen: gaps changed and saved, marked in the source
    goto(&mut app, Page::Layout);
    apply(
        &mut app,
        [
            Message::Setting(Setting::LayoutGaps, Edit::Draft("24".into())),
            Message::SaveNow,
        ],
    );
    app.source_shown = true;
    shoot(&app, "hero-layout")?;
    app.source_shown = false;
    goto(&mut app, Page::Keybinds);
    apply(
        &mut app,
        [Message::Keybinds(keybinds::Message::KeyFound("L".into()))],
    );
    shoot(&app, "keybinds-key")?;
    apply(
        &mut app,
        [Message::Keybinds(keybinds::Message::KeyClicked("L".into()))],
    );
    // a change still waiting: markers on the row
    goto(&mut app, Page::Layout);
    apply(
        &mut app,
        [Message::Setting(
            Setting::LayoutPresetColumnWidths,
            Edit::Draft(
                "proportion 0.33333; proportion 0.5; proportion 0.66667; proportion 1.0".into(),
            ),
        )],
    );
    apply(
        &mut app,
        [Message::Setting(
            Setting::LayoutFocusRingWidth,
            Edit::Draft("x".into()),
        )],
    );
    shoot(&app, "layout-changed")?;
    goto(&mut app, Page::Keybinds);
    app.header_search = Some(("focus".into(), 0));
    shoot(&app, "palette")?;
    app.header_search = None;
    app.help_open = true;
    shoot(&app, "help")?;
    app.help_open = false;
    app.closing = Some(crate::app::Closing::Asking);
    shoot(&app, "close")?;
    app.closing = None;
    app.session.niri_reloaded(true);
    app.session.diagnosed(Err(
        "Error:   × unexpected node\n   ╭─[config.kdl:3:5]".into()
    ));
    shoot(&app, "health")?;
    app.session.health_fixed();
    // a change niri turns down: not saved, with niri's words
    app.session.problem = Some(Problem::Rejected(
        "Error:   × duplicate keybind later defined here\n    ╭─[config.kdl:57:1]\n 57 │     Mod+Q { quit; }\n    ·     ──┬──\n    ·       ╰── unexpected keybind\n    ╰────".into(),
    ));
    shoot(&app, "not-saved")?;
    app.session.problem = None;
    // a rule made from an open window
    goto(&mut app, Page::Rules);
    apply(
        &mut app,
        [
            Message::Rules(rules::Message::FromWindow("kitty".into(), "~/code".into())),
            Message::Rules(rules::Message::Property(RuleProperty::OpenFloating, true)),
        ],
    );
    shoot(&app, "rules-window")?;
    if let Some(data) = &mut app.running {
        data.windows.push(OpenWindow {
            id: 20,
            app_id: "firefox".into(),
            title: "Picture-in-Picture".into(),
            floating: true,
            focused: false,
        });
    }
    apply(
        &mut app,
        [Message::Rules(rules::Message::FromWindow(
            "firefox".into(),
            "Picture-in-Picture".into(),
        ))],
    );
    shoot(&app, "rules-window-applying")?;
    apply(&mut app, [Message::Rules(rules::Message::Edit(0))]);
    shoot(&app, "rules-dialog")?;
    apply(&mut app, [Message::Rules(rules::Message::CancelEdit)]);
    // the search on Window rules: its apps and rules first
    apply(&mut app, [Message::SearchQuery("fire".into())]);
    shoot(&app, "rules-search")?;
    apply(&mut app, [Message::SearchQuery(String::new())]);
    // a third of the screen: an app's card, then one of its rules, alone
    app.window_width = THIRD.width;
    app.window_height = THIRD.height;
    let _ = ui(&app, THIRD)
        .snapshot(&theme)?
        .matches_image(out.join("dark-third-rules-app.png"))?;
    apply(&mut app, [Message::Rules(rules::Message::Open(0))]);
    let _ = ui(&app, THIRD)
        .snapshot(&theme)?
        .matches_image(out.join("dark-third-rules-rule.png"))?;
    apply(&mut app, [Message::Rules(rules::Message::Show(None))]);
    app.window_width = WIDE.width;
    app.window_height = WIDE.height;
    // a display change on trial
    goto(&mut app, Page::Displays);
    apply(
        &mut app,
        [Message::Output(
            "DP-1".into(),
            OutputSetting::Transform,
            Edit::Choice(Some("90".into())),
        )],
    );
    shoot(&app, "displays-trying")?;
    apply(&mut app, [Message::KeepTrial]);
    // the rule library
    apply(
        &mut app,
        [
            Message::Rules(rules::Message::OpenLibrary(true)),
            Message::Rules(rules::Message::LibraryAllApps(true)),
            Message::Rules(rules::Message::LibraryApp(Some(
                niri_center::library::KnownApp::Steam,
            ))),
            Message::Rules(rules::Message::LibraryMark(8, true)),
        ],
    );
    goto(&mut app, Page::Rules);
    shoot(&app, "rules-library")?;
    apply(
        &mut app,
        [Message::Rules(rules::Message::OpenLibrary(false))],
    );
    // niri cannot read a misspelled name
    goto(&mut app, Page::Layout);
    app.session
        .store
        .as_mut()
        .unwrap()
        .set_layout_gaps(12.0)
        .unwrap();
    let path = store(&app).path().to_path_buf();
    let line = store(&app)
        .setting_line(&[niri_center::store::Block::named("layout")], "gaps")
        .map(|(_, line)| line)
        .unwrap();
    let _ = store(&app).text_of(&path);
    app.session
        .store
        .as_mut()
        .unwrap()
        .rename_node(&path, line, "gaps", "gpas")
        .unwrap();
    // niri's words arrive once the name is misspelled on disk
    app.session.niri_reloaded(true);
    app.session.diagnosed(Err(
        "Error:   × error loading config\n  ╰─▶ unexpected node `gpas`".into(),
    ));
    app.broken_line_shown = true;
    shoot(&app, "repair")?;
    app.session.health_fixed();
    app.session
        .store
        .as_mut()
        .unwrap()
        .rename_node(&path, line, "gpas", "gaps")
        .unwrap();
    goto(&mut app, Page::Environment);
    apply(
        &mut app,
        [
            Message::Environment(environment::Message::NewName("QT_QPA_PLATFORM".into())),
            Message::Environment(environment::Message::NewValue("wayland".into())),
            Message::Environment(environment::Message::Add),
        ],
    );
    shoot(&app, "environment-added")?;
    // a toast for something that worked, and one for an error
    app.status = crate::app::info("Imported 1 rule.");
    shoot(&app, "toast")?;
    app.status = crate::app::error(
        "A variable name is letters, digits and underscores, and does not start with a digit.",
    );
    shoot(&app, "toast-error")?;
    app.status = None;
    // saving on request: the diff over the page
    app.session.save_when = niri_center::session::SaveWhen::OnRequest;
    apply(
        &mut app,
        [Message::Setting(
            Setting::LayoutGaps,
            Edit::Draft("28".into()),
        )],
    );
    goto(&mut app, Page::Layout);
    apply(&mut app, [Message::ShowReview(true)]);
    shoot(&app, "review-on-request")?;
    apply(&mut app, [Message::ShowReview(false), Message::Undo]);
    app.session.save_when = niri_center::session::SaveWhen::AfterChange;
    // the history after those changes are saved
    apply(&mut app, [Message::SaveNow]);
    goto(&mut app, Page::History);
    shoot(&app, "history-saved")?;
    app.window_width = THIRD.width;
    let _ = ui(&app, THIRD)
        .snapshot(&theme)?
        .matches_image(out.join("dark-third-history-list.png"))?;
    // half a 1920 px screen: the sidebar folded to icons
    app.window_width = NARROW.width;
    let _ = ui(&app, NARROW)
        .snapshot(&theme)?
        .matches_image(out.join("dark-narrow-sidebar-folded.png"))?;
    Ok(())
}

/// Every page from top to bottom at a desktop, half and third width, to
/// look over whole panes. Not an assertion: run with `--ignored`.
#[test]
#[ignore = "renders PNGs for human review; run with --ignored"]
fn render_whole_pages_for_review() -> Result<(), Error> {
    let out = PathBuf::from("../target/ui-pages");
    clear_shots(&out);
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    app.mode = Some(iced::theme::Mode::Dark);
    let theme = theme::for_mode(app.mode, false);
    for (name, width) in [("wide", 1280.0), ("half", 940.0), ("third", 620.0)] {
        app.window_width = width;
        for page in Page::ALL {
            // Layout is the longest page by far
            let height = if page == Page::Layout { 4000.0 } else { 2400.0 };
            app.window_height = height;
            goto(&mut app, page);
            let _ = ui(&app, Size::new(width, height))
                .snapshot(&theme)?
                .matches_image(out.join(format!("{name}-{}.png", page.slug())))?;
        }
    }
    Ok(())
}

// ---- keybinds ----

#[test]
fn saving_an_untouched_edit_keeps_the_config_identical() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    let before = document(&app);
    // three argv items, one with a space: its display text
    // (`spawn sh -c notify-send hello`) cannot be re-quoted back
    let index = binding_index(&app, "Mod+N");
    apply(
        &mut app,
        [Message::Keybinds(keybinds::Message::Edit(index))],
    );
    click(&mut app, "Save");
    assert_eq!(document(&app), before, "{}", status_text(&app));
}

#[test]
fn undo_puts_a_removed_binding_back_byte_for_byte() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    let before = document(&app);
    // Mod+T carries a hotkey-overlay-title prop the old undo dropped
    let index = binding_index(&app, "Mod+T");
    apply(
        &mut app,
        [Message::Keybinds(keybinds::Message::Remove(index))],
    );
    assert_ne!(document(&app), before);
    click(&mut app, "Undo");
    assert_eq!(document(&app), before);
}

#[test]
fn plain_words_typed_as_arguments_become_a_quoted_string() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    apply(&mut app, [Message::Keybinds(keybinds::Message::New(None))]);
    let form = binding_form(&mut app);
    form.key = "Mod+B".into();
    form.action = Some("spawn-sh".into());
    form.args = "firefox --new-window".into();
    click(&mut app, "Add binding");
    let out = document(&app);
    assert!(
        out.contains("Mod+B { spawn-sh \"firefox --new-window\"; }"),
        "{out}"
    );
    assert_niri_accepts(&app);
}

#[test]
fn the_action_list_is_niris_own() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    apply(&mut app, [Message::Keybinds(keybinds::Message::New(None))]);
    binding_form(&mut app).action = Some("move-column-to-workspace".into());
    // niri's description and the argument syntax appear under the form
    assert!(shows(
        &app,
        WIDE,
        "move-column-to-workspace <reference> focus=<value>"
    ));
}

#[test]
fn binding_a_taken_key_in_the_same_file_is_refused() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    apply(&mut app, [Message::Keybinds(keybinds::Message::New(None))]);
    let before = document(&app);
    let form = binding_form(&mut app);
    form.key = "Mod+Q".into();
    form.action = Some("quit".into());
    click(&mut app, "Add binding");
    assert_eq!(document(&app), before);
    assert!(
        status_text(&app).contains("close-window"),
        "{}",
        status_text(&app)
    );
}

#[test]
fn a_key_bound_in_another_file_may_be_overridden() {
    // niri: later binds override conflicting keys from earlier files
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("config.kdl"),
        "binds {\n    Mod+T { spawn \"kitty\"; }\n}\ninclude \"more.kdl\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("more.kdl"),
        "binds {\n    Mod+Q { quit; }\n}\n",
    )
    .unwrap();
    let mut app = App { ..App::default() };
    load_into(&mut app, load(dir.path().join("config.kdl")));
    apply(&mut app, [Message::Keybinds(keybinds::Message::New(None))]);
    // a new binding goes to config.kdl; Mod+Q lives in more.kdl, which
    // config.kdl includes after its binds, so niri keeps that one
    let form = binding_form(&mut app);
    form.key = "Mod+Q".into();
    form.action = Some("close-window".into());
    click(&mut app, "Add binding");
    assert!(document(&app).contains("Mod+Q { close-window; }"));
    assert_eq!(
        status_text(&app),
        "Added Mod+Q. more.kdl binds it too and is read later, so niri uses that one."
    );
}

#[test]
fn a_pressed_key_shows_its_bindings_and_offers_one_more() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    click(&mut app, "Find by pressing keys");
    assert!(app.keybinds.finding_keys);
    // the window saw L pressed (without Mod: niri keeps Mod combos)
    apply(
        &mut app,
        [Message::Keybinds(keybinds::Message::KeyFound("L".into()))],
    );
    assert!(!app.keybinds.finding_keys);
    assert!(shows(&app, WIDE, "2 bindings on L"));
    click(&mut app, "Bind something to L");
    assert_eq!(binding_form(&mut app).key, "Mod+L");
}

#[test]
fn the_keyboard_picture_can_be_hidden() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    click(&mut app, "Hide keyboard");
    assert!(app.keybinds.keyboard_hidden);
    assert!(shows(&app, WIDE, "Show keyboard"));
}

#[test]
fn each_page_keeps_its_own_search() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    apply(
        &mut app,
        [Message::Keybinds(keybinds::Message::Search("kitty".into()))],
    );
    goto(&mut app, Page::Startup);
    assert!(shows(&app, WIDE, "waybar"));
}

// ---- autostart ----

#[test]
fn an_autostart_command_with_arguments_runs_through_a_shell() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Startup);
    app.startup.command = "mako --config ~/.config/mako/niri".into();
    click(&mut app, "Add");
    assert!(
        document(&app).contains("spawn-sh-at-startup \"mako --config ~/.config/mako/niri\""),
        "{}",
        document(&app)
    );
}

// ---- registry settings ----

#[test]
fn every_registry_setting_stages_a_value_niri_accepts() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    for &setting in Setting::ALL {
        let spec = setting.spec();
        spec.write(app.session.store.as_mut().unwrap(), &spec.sample())
            .unwrap_or_else(|e| panic!("{setting:?}: {e}"));
        assert_eq!(
            spec.read(store(&app)),
            spec.sample(),
            "{setting:?} reads back"
        );
    }
    assert_niri_accepts(&app);
}

#[test]
fn every_display_setting_stages_a_value_niri_accepts() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    let chain = [niri_center::store::Block::with_arg("output", "DP-1")];
    for &setting in OutputSetting::ALL {
        // turning the display off is checked on its own
        if setting == OutputSetting::Off {
            continue;
        }
        let spec = setting.spec();
        let sample = spec.sample();
        niri_center::settings::write(
            app.session.store.as_mut().unwrap(),
            &chain,
            spec.child,
            spec.kind,
            &sample,
        )
        .unwrap_or_else(|e| panic!("{setting:?}: {e}"));
        assert_eq!(
            niri_center::settings::read(store(&app), &chain, spec.child, spec.kind),
            sample,
            "{setting:?} reads back"
        );
    }
    assert_niri_accepts(&app);
}

#[test]
fn a_display_change_reads_the_same_in_its_row_and_in_the_history() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Displays);
    let before = app.session.store.clone().unwrap();
    apply(
        &mut app,
        [Message::Output(
            "DP-1".into(),
            OutputSetting::Transform,
            Edit::Choice(Some("90".into())),
        )],
    );
    let summary = niri_center::summary::describe(&before, store(&app));
    assert_eq!(summary, ["DP-1 rotation Normal → Turned 90 degrees"]);
    app.session.saved = Some(before);
    assert!(shows(&app, WIDE, "Was Normal"));
}

#[test]
fn the_browser_demo_says_what_only_the_desktop_app_does() {
    let banner = "Running in your browser on niri's default config. Changes stay in this page.";
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    assert!(!shows(&app, WIDE, banner));
    app.session.platform = Platform::Browser;
    assert!(shows(&app, WIDE, banner));
    app.files.snapshot_name = "before".into();
    apply(&mut app, [Message::Files(files::Message::CreateSnapshot)]);
    assert!(
        status_text(&app).contains("desktop app"),
        "{}",
        status_text(&app)
    );
    let listing = app.test_actor.as_mut().unwrap().handle(Command::Listing);
    assert!(
        matches!(listing.as_slice(), [Reply::Listing { snapshots, .. }] if snapshots.is_empty())
    );
}

#[test]
fn every_setting_has_a_row() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    for page in Page::ALL.into_iter().filter(|page| page.only_settings()) {
        goto(&mut app, page);
        for setting in Setting::ALL.iter().filter(|s| s.spec().page == page) {
            assert!(
                shows(&app, TALL, setting.spec().label),
                "{setting:?} on {page:?}"
            );
        }
    }
}

#[test]
fn a_set_value_links_to_its_line_in_the_config() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Layout);
    let line = 1 + UI_FIXTURE[..UI_FIXTURE.find("    gaps ").unwrap()]
        .matches('\n')
        .count();
    assert!(shows(&app, TALL, &format!("config.kdl:{line}")));
}

#[test]
fn a_saved_change_says_what_it_was_and_undo_restores_it() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    let before = document(&app);
    goto(&mut app, Page::Input);
    // the fixture has `tap`: turning it off is a change
    apply(
        &mut app,
        [
            Message::Setting(Setting::InputTouchpadTap, Edit::Switch(false)),
            Message::SaveNow,
        ],
    );
    assert!(!document(&app).contains("        tap\n"));
    assert_eq!(app.session.pending(), 0, "saved");
    assert!(shows(&app, TALL, "Was on"));
    assert!(shows(&app, WIDE, "Tap to click on → off saved"));
    click(&mut app, "Undo");
    assert_eq!(document(&app), before);
    assert_eq!(
        app.session.history.len(),
        2,
        "the undo is a change of its own"
    );
}

#[test]
fn an_invalid_number_is_explained_and_not_staged() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    let before = document(&app);
    goto(&mut app, Page::Layout);
    apply(
        &mut app,
        [Message::Setting(
            Setting::LayoutFocusRingWidth,
            Edit::Draft("wide".into()),
        )],
    );
    assert_eq!(document(&app), before);
    assert!(shows(&app, WIDE, "Enter a whole number from 0 to 64."));
    apply(
        &mut app,
        [Message::Setting(
            Setting::LayoutFocusRingWidth,
            Edit::Draft("6".into()),
        )],
    );
    assert!(
        document(&app).contains("focus-ring {\n        width 6\n    }"),
        "{}",
        document(&app)
    );
}

#[test]
fn clearing_a_field_returns_the_setting_to_niris_default() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    apply(
        &mut app,
        [Message::Setting(
            Setting::LayoutGaps,
            Edit::Draft(String::new()),
        )],
    );
    assert!(!document(&app).contains("gaps"));
    assert_niri_accepts(&app);
}

#[test]
fn search_finds_a_setting_and_lands_on_its_row() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    let entry = app
        .search_index
        .matches("repeat rate")
        .find(|e| e.setting == Some(Setting::InputKeyboardRepeatRate))
        .cloned()
        .expect("repeat rate is searchable");
    apply(&mut app, [Message::SearchPick(entry)]);
    assert_eq!(app.page, Page::Input);
    assert_eq!(app.highlight, Some(Setting::InputKeyboardRepeatRate));
}

// ---- displays ----

#[test]
fn display_rows_edit_the_outputs_block() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Displays);
    assert!(shows(&app, TALL, "Variable refresh rate"));
    // eDP-1 has no VRR, so alone it gets no VRR row
    let both = app.outputs.clone();
    app.outputs.retain(|output| output.name == "eDP-1");
    assert!(!shows(&app, TALL, "Variable refresh rate"));
    app.outputs = both;
    apply(
        &mut app,
        [
            Message::Output(
                "eDP-1".into(),
                OutputSetting::Scale,
                Edit::Draft("1.25".into()),
            ),
            Message::Output(
                "DP-1".into(),
                OutputSetting::Mode,
                Edit::Choice(Some("3840x2160@59.997".into())),
            ),
            Message::Output(
                "DP-1".into(),
                OutputSetting::VariableRefreshRate,
                Edit::Switch(true),
            ),
        ],
    );
    let out = document(&app);
    assert!(out.contains("    scale 1.25\n"), "{out}");
    assert!(out.contains("mode \"3840x2160@59.997\""), "{out}");
    assert!(out.contains("variable-refresh-rate"), "{out}");
    assert_niri_accepts(&app);
}

#[test]
fn a_display_change_goes_back_unless_kept() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    let original = document(&app);
    goto(&mut app, Page::Displays);
    apply(
        &mut app,
        [Message::Output(
            "DP-1".into(),
            OutputSetting::Transform,
            Edit::Choice(Some("90".into())),
        )],
    );
    assert_eq!(app.session.pending(), 0, "saved at once");
    assert!(shows(&app, TALL, "Keep"));
    for _ in 0..TRIAL_SECONDS {
        apply(&mut app, [Message::TrialTick]);
    }
    assert!(app.session.trial.is_none());
    assert_eq!(document(&app), original, "it went back on its own");
    // after an edit that was still waiting: that one is saved first, and
    // the trial is the display change alone
    apply(
        &mut app,
        [Message::Setting(
            Setting::LayoutGaps,
            Edit::Draft("20".into()),
        )],
    );
    apply(
        &mut app,
        [Message::Output(
            "DP-1".into(),
            OutputSetting::Transform,
            Edit::Choice(Some("180".into())),
        )],
    );
    assert!(
        app.session
            .trial
            .as_ref()
            .is_some_and(|t| t.what.starts_with("DP-1 rotation")),
        "{:?}",
        app.session.trial
    );
    apply(&mut app, [Message::EndTrial]);
    // kept, it stays
    apply(
        &mut app,
        [
            Message::Output(
                "DP-1".into(),
                OutputSetting::Transform,
                Edit::Choice(Some("90".into())),
            ),
            Message::Shortcut(crate::shortcuts::Shortcut::Confirm),
        ],
    );
    assert!(app.session.trial.is_none());
    assert!(
        document(&app).contains("transform \"90\""),
        "{}",
        document(&app)
    );
    assert!(
        app.session.history.last().unwrap().summary[0].contains("DP-1 rotation"),
        "{:?}",
        app.session.history.last().unwrap().summary
    );
}

// ---- animations ----

#[test]
fn choosing_an_easing_for_a_spring_animation_replaces_the_spring() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Animations);
    apply(
        &mut app,
        [Message::Animations(animations::Message::Select(
            Animation::WindowOpen,
        ))],
    );
    assert!(shows(
        &app,
        TALL,
        "A spring in your config: damping 0.8, stiffness 1000."
    ));
    apply(
        &mut app,
        [
            Message::Animations(animations::Message::Spring(false)),
            Message::Animations(animations::Message::Curve(Curve::EaseOutQuad)),
        ],
    );
    click(&mut app, "Use this curve");
    let out = document(&app);
    let window_open = &out[out.find("window-open").unwrap()..out.find("workspace-switch").unwrap()];
    assert!(
        window_open.contains("curve \"ease-out-quad\""),
        "{}\n{out}",
        status_text(&app)
    );
    assert!(window_open.contains("duration-ms 150"), "{out}");
    assert!(!window_open.contains("spring"), "{out}");
    assert_niri_accepts(&app);
}

#[test]
fn an_animation_the_config_leaves_alone_can_take_a_custom_curve() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Animations);
    // every animation niri has is offered, not just the configured ones
    apply(
        &mut app,
        [Message::Animations(animations::Message::Select(
            Animation::OverviewOpenClose,
        ))],
    );
    assert!(shows(
        &app,
        TALL,
        "niri's default: a spring, damping 1, stiffness 800."
    ));
    apply(
        &mut app,
        [
            Message::Animations(animations::Message::Spring(false)),
            Message::Animations(animations::Message::Curve(Curve::CubicBezier)),
            Message::Animations(animations::Message::Bezier(crate::curves::Bezier([
                0.05, 0.7, 0.1, 1.0,
            ]))),
            Message::Animations(animations::Message::Duration("300".into())),
        ],
    );
    click(&mut app, "Use this curve");
    let out = document(&app);
    assert!(
        out.contains("curve \"cubic-bezier\" 0.05 0.7 0.1 1.0"),
        "{}\n{out}",
        status_text(&app)
    );
    assert_niri_accepts(&app);
    click(&mut app, "Back to niri's default");
    assert!(!document(&app).contains("overview-open-close"));
}

#[test]
fn only_curves_niri_accepts_are_offered() {
    let names: Vec<&str> = Curve::ALL.into_iter().map(Curve::name).collect();
    assert_eq!(
        names,
        [
            "linear",
            "ease-out-quad",
            "ease-out-cubic",
            "ease-out-expo",
            "cubic-bezier"
        ]
    );
}

// ---- window rules ----

#[test]
fn your_rules_and_the_rule_library_are_two_tabs_of_window_rules() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Rules);
    assert!(shows(&app, WIDE, "Apps"));
    click(&mut app, "Rule library");
    assert!(app.rules.library.open);
    assert!(!shows(&app, WIDE, "Apps"));
    assert!(shows(
        &app,
        WIDE,
        "From niri's docs and issues, and from configs people share."
    ));
    click(&mut app, "Your rules");
    assert!(!app.rules.library.open);
    assert!(shows(&app, WIDE, "Apps"));
}

#[test]
fn a_rule_is_changed_from_its_apps_page_in_one_click() {
    use niri_center::by_app::Who;
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Rules);
    apply(
        &mut app,
        [Message::Rules(rules::Message::Show(Some(Who::App(
            "firefox".into(),
        ))))],
    );
    // the rule shows on the app's page with what can be done to it
    assert!(shows(&app, WIDE, "title Picture-in-Picture, config.kdl:69"));
    assert!(shows(&app, WIDE, "Turn off"));
    click(&mut app, "Edit");
    assert!(app.rules.dialog);
    assert_eq!(app.rules.draft.editing, Some(firefox_rule(&app)));
    // saved, the app's page is still the one shown
    click(&mut app, "Save changes");
    assert!(!app.rules.dialog);
    assert_eq!(app.rules.app, Some(Who::App("firefox".into())));
    assert_eq!(app.rules.open, None);
}

/// The fixture's rule for Firefox's picture-in-picture window.
fn firefox_rule(app: &App) -> usize {
    store(app)
        .window_rule_rows()
        .iter()
        .position(|rule| niri_center::rules::name(rule).starts_with("firefox"))
        .unwrap()
}

#[test]
fn a_narrow_rules_page_zooms_in_and_escape_goes_back_up() {
    use niri_center::by_app::Who;
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Rules);
    app.window_width = THIRD.width;
    app.window_height = THIRD.height;
    assert!(!app.rules_beside_list());
    // the index alone, an app by its name
    assert!(shows(&app, THIRD, "kitty"));
    assert!(shows(&app, THIRD, "Firefox"));
    let firefox = Some(Who::App("firefox".into()));
    apply(
        &mut app,
        [Message::Rules(rules::Message::Show(firefox.clone()))],
    );
    // its card alone: within Firefox, a rule says what else picks it
    assert!(!shows(&app, THIRD, "kitty"));
    assert!(shows(
        &app,
        THIRD,
        "title Picture-in-Picture, config.kdl:69"
    ));
    let rule = firefox_rule(&app);
    apply(&mut app, [Message::Rules(rules::Message::Open(rule))]);
    assert!(shows(&app, THIRD, "firefox: open floating"));
    // Escape goes up to the app, Alt+Left to the index
    apply(
        &mut app,
        [Message::Shortcut(crate::shortcuts::Shortcut::Close)],
    );
    assert_eq!(app.rules.open, None);
    assert_eq!(app.rules.app, firefox);
    apply(
        &mut app,
        [Message::Shortcut(crate::shortcuts::Shortcut::Back)],
    );
    assert_eq!(app.rules.app, None);
    assert!(shows(&app, THIRD, "kitty"));
    // at the index there is nowhere further up
    apply(
        &mut app,
        [Message::Shortcut(crate::shortcuts::Shortcut::Close)],
    );
    assert_eq!(app.page, Page::Rules);
}

#[test]
fn a_rule_opened_in_a_narrow_window_is_still_open_when_it_widens() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Rules);
    app.window_width = THIRD.width;
    app.window_height = THIRD.height;
    let rule = firefox_rule(&app);
    apply(&mut app, [Message::Rules(rules::Message::Open(rule))]);
    app.window_width = WIDE.width;
    app.window_height = WIDE.height;
    assert!(app.rules_beside_list());
    // the index beside the same rule
    assert!(shows(&app, WIDE, "kitty"));
    assert!(shows(&app, WIDE, "firefox: open floating"));
    assert_eq!(app.rules.open, Some(rule));
}

#[test]
fn up_and_down_move_through_the_index_beside_the_app_shown() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Rules);
    app.window_width = WIDE.width;
    app.window_height = WIDE.height;
    let windows = app
        .running
        .as_ref()
        .map(|data| data.windows.clone())
        .unwrap_or_default();
    let index = niri_center::by_app::index(
        store(&app).window_rule_rows().as_slice(),
        &windows,
        &app.system,
    );
    assert!(index.len() >= 2, "{index:?}");
    // the first app shows beside the index until another is picked
    assert!(shows(&app, WIDE, "Rules"));
    let key = |shortcut| Message::Shortcut(shortcut);
    apply(&mut app, [key(crate::shortcuts::Shortcut::Down)]);
    assert_eq!(app.rules.app.as_ref(), Some(&index[1].who));
    apply(&mut app, [key(crate::shortcuts::Shortcut::Up)]);
    assert_eq!(app.rules.app.as_ref(), Some(&index[0].who));
}

#[test]
fn a_toast_or_a_dialog_leaves_the_page_where_it_was_scrolled() {
    // iced keeps how far a list is scrolled by the widget's place in the
    // tree: the page must keep its place when something floats over it
    let place = |app: &App| {
        let element = view(app);
        let root = element.as_widget();
        let page = root.children().first().map(|tree| tree.tag);
        (root.tag(), root.children().len(), page)
    };
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Rules);
    let plain = place(&app);
    app.status = crate::app::info("Added: Firefox picture-in-picture.");
    assert_eq!(place(&app), plain, "a toast");
    apply(&mut app, [Message::Rules(rules::Message::New(None))]);
    assert!(app.rules.dialog);
    assert_eq!(place(&app), plain, "a dialog over the toast");
}

#[test]
fn the_rules_page_scrolls_its_own_columns_only_when_wide_and_tall() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Rules);
    app.window_width = 1280.0;
    app.window_height = 800.0;
    // each column scrolls on its own: the rule picked low in the list
    // shows at the top of its column, not where the page was scrolled to
    assert!(app.page_scrolls_itself());
    app.window_height = 500.0;
    assert!(!app.page_scrolls_itself(), "too short: the page scrolls");
    app.window_height = 800.0;
    app.window_width = 620.0;
    assert!(
        !app.page_scrolls_itself(),
        "a third of the screen stacks them"
    );
}

#[test]
fn a_rule_built_from_an_open_window_is_staged_exactly() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    app.source_shown = true;
    goto(&mut app, Page::Windows);
    apply(
        &mut app,
        [Message::Rules(rules::Message::NewForWindow(
            "org.gnome.Calculator".into(),
            "Calculator".into(),
        ))],
    );
    assert_eq!(app.page, Page::Rules);
    apply(
        &mut app,
        [Message::Rules(rules::Message::Property(
            RuleProperty::OpenFloating,
            true,
        ))],
    ); // open floating
    // the window it is for, and where the rule goes in the source
    assert!(shows(&app, WIDE, "New rule"));
    assert!(shows(&app, WIDE, "1 open window matches it now"));
    let lines = document(&app).lines().count();
    assert!(shows(
        &app,
        WIDE,
        &format!("The new rule goes after line {lines}")
    ));
    let preview = app.rules.draft.rule().to_kdl();
    click(&mut app, "Add rule");
    assert!(document(&app).contains(&preview));
    let out = document(&app);
    assert!(
        out.contains("match app-id=r#\"^org\\.gnome\\.Calculator$\"#\n    open-floating true\n}"),
        "{out}"
    );
    assert_niri_accepts(&app);
}

#[test]
fn the_windows_of_one_app_are_one_entry_in_the_index() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    let terminal = OpenWindow {
        id: 10,
        app_id: "Alacritty".into(),
        title: "Alacritty".into(),
        floating: false,
        focused: false,
    };
    if let Some(data) = &mut app.running {
        for id in 10..15 {
            data.windows.push(OpenWindow {
                id,
                ..terminal.clone()
            });
        }
    }
    goto(&mut app, Page::Rules);
    assert!(shows(&app, WIDE, "5 open windows"));
    click(&mut app, "Alacritty");
    assert!(shows(&app, WIDE, "Alacritty, tiled, 5 windows"));
    assert_eq!(
        app.rules
            .window
            .as_ref()
            .map(|window| window.app_id.as_str()),
        Some("Alacritty")
    );
}

#[test]
fn a_rule_needs_a_match_and_something_to_do() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Rules);
    let before = document(&app);
    apply(&mut app, [Message::Rules(rules::Message::New(None))]);
    click(&mut app, "Add rule");
    assert!(status_text(&app).contains("app ID"));
    apply(
        &mut app,
        [Message::Rules(rules::Message::App("kitty".into()))],
    );
    click(&mut app, "Add rule");
    assert!(status_text(&app).contains("at least one"));
    assert_eq!(document(&app), before);
}

// ---- environment ----

#[test]
fn an_environment_variable_is_added_and_saved_with_its_name() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Environment);
    apply(
        &mut app,
        [
            Message::Environment(environment::Message::NewName("1BAD".into())),
            Message::Environment(environment::Message::Add),
        ],
    );
    assert!(status_text(&app).contains("does not start with a digit"));
    apply(
        &mut app,
        [
            Message::Environment(environment::Message::NewName("QT_QPA_PLATFORM".into())),
            Message::Environment(environment::Message::NewValue("wayland".into())),
            Message::Environment(environment::Message::Add),
            Message::SaveNow,
        ],
    );
    assert!(document(&app).contains("QT_QPA_PLATFORM \"wayland\""));
    assert!(shows(&app, TALL, "QT_QPA_PLATFORM"));
    assert_eq!(
        app.session.history.last().unwrap().summary,
        vec!["Sets QT_QPA_PLATFORM to wayland".to_string()]
    );
    assert_niri_accepts(&app);
}

// ---- toasts ----

#[test]
fn a_toast_says_what_happened_and_clears_itself_unless_it_is_an_error() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Environment);
    apply(
        &mut app,
        [
            Message::Environment(environment::Message::NewName("EDITOR".into())),
            Message::Environment(environment::Message::NewValue("hx".into())),
            Message::Environment(environment::Message::Add),
            Message::Environment(environment::Message::NewName("1BAD".into())),
            Message::Environment(environment::Message::Add),
        ],
    );
    // an error stays until dismissed
    let error = status_text(&app);
    assert!(shows(&app, WIDE, &error));
    if let Some(status) = &mut app.status {
        status.since = iced::time::Instant::now() - std::time::Duration::from_secs(60);
    }
    apply(&mut app, [Message::ToastTick]);
    assert_eq!(status_text(&app), error);
    apply(&mut app, [Message::DismissToast]);
    assert!(app.status.is_none());
    // what worked goes on its own
    app.status = crate::app::info("Imported 1 rule.");
    if let Some(status) = &mut app.status {
        status.since = iced::time::Instant::now() - std::time::Duration::from_secs(60);
    }
    apply(&mut app, [Message::ToastTick]);
    assert!(app.status.is_none());
}

#[test]
fn text_on_changed_and_selected_rows_meets_its_contrast_floor() {
    for colors in theme::ALL {
        let floor = if matches!(
            colors.palette,
            crate::theme::Palette::HighContrastDark | crate::theme::Palette::HighContrastLight
        ) {
            7.0
        } else {
            4.5
        };
        let alpha = if colors.dark { 0.12 } else { 0.14 };
        for tint in [colors.pending, colors.accent] {
            let fill = theme::mix(colors.surface, tint, alpha);
            for ink in [colors.text, colors.muted] {
                let ratio = theme::contrast(ink, fill);
                assert!(ratio >= floor, "{:?}: {ratio}", colors.palette);
            }
        }
    }
}

#[test]
fn toast_text_meets_its_contrast_floor() {
    for colors in theme::ALL {
        let floor = if matches!(
            colors.palette,
            crate::theme::Palette::HighContrastDark | crate::theme::Palette::HighContrastLight
        ) {
            7.0
        } else {
            4.5
        };
        for tint in [colors.success, colors.error] {
            let fill = theme::mix(colors.surface, tint, 0.12);
            let ratio = theme::contrast(colors.text, fill);
            assert!(ratio >= floor, "{:?}: {ratio}", colors.palette);
        }
    }
}

// ---- search and shortcuts ----

#[test]
fn typing_with_no_field_focused_starts_a_search() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Layout);
    for typed in ["g", "a", "p"] {
        apply(
            &mut app,
            [Message::Shortcut(crate::shortcuts::Shortcut::Type(
                typed.into(),
            ))],
        );
    }
    assert_eq!(
        app.header_search.as_ref().map(|(q, _)| q.as_str()),
        Some("gap")
    );
    assert!(shows(&app, WIDE, "Spacing: Gaps"));
    apply(
        &mut app,
        [Message::Shortcut(crate::shortcuts::Shortcut::Close)],
    );
    assert!(app.header_search.is_none(), "Esc goes back to the page");
}

#[test]
fn the_shortcut_list_says_when_a_bind_takes_an_app_key() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    app.help_open = true;
    // no clash: the list says nothing about one
    assert!(!shows(&app, WIDE, "Bound in your config"));
    app.session
        .store
        .as_mut()
        .unwrap()
        .add_binding("Ctrl+Z", "spawn \"kitty\";")
        .unwrap();
    app.refresh_rows();
    assert!(shows(
        &app,
        WIDE,
        "Bound in your config, so niri takes Ctrl+Z before the app sees it."
    ));
}

#[test]
fn the_config_column_pops_out_and_folds_here() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Layout);
    assert!(app.shows_source());
    // spawning the window is a Task for niri, which the simulator skips
    apply(&mut app, [Message::PopOut(Page::Source)]);
    assert!(!app.source_shown);
    // the window itself: the whole file
    app.pane = Some(Page::Source);
    goto(&mut app, Page::Source);
    assert!(shows(&app, WIDE, "Put back"));
}

#[test]
fn the_sidebar_stays_and_folds_to_icons_in_a_narrow_window() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    app.window_width = WIDE.width;
    assert!(!app.sidebar_folded());
    assert!(
        shows(&app, WIDE, "Keyboard and mouse"),
        "headings when open"
    );
    // a page far down the list is one click away, at any width
    click(&mut app, "History");
    assert_eq!(app.page, Page::History);
    app.window_width = THIRD.width;
    assert!(app.sidebar_folded(), "a narrow window folds it");
    assert!(!shows(&app, THIRD, "Keyboard and mouse"));
    // asked for, it stays as asked
    apply(
        &mut app,
        [Message::Shortcut(crate::shortcuts::Shortcut::ToggleSidebar)],
    );
    assert!(!app.sidebar_folded());
}

#[test]
fn the_config_shows_only_when_asked_for() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Layout);
    for width in [1280.0, 2560.0] {
        app.window_width = width;
        assert!(!app.source_open(), "not by itself at {width}");
    }
    app.window_width = WIDE.width;
    click(&mut app, "Show config.kdl");
    assert!(app.source_open());
    click(&mut app, "Hide config.kdl");
    assert!(!app.source_open());
}

#[test]
fn putting_the_config_window_back_brings_the_column_back_as_it_was() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Layout);
    apply(
        &mut app,
        [Message::ShowSource(true), Message::PopOut(Page::Source)],
    );
    assert!(!app.source_shown);
    // niri reports the new window, then other windows come and go
    apply(
        &mut app,
        [Message::SourceWindow(7), Message::WindowClosed(3)],
    );
    assert!(!app.source_shown);
    // Put back closes it
    apply(&mut app, [Message::WindowClosed(7)]);
    assert!(app.source_shown);
    assert!(app.source_popped.is_none());
}

#[test]
fn the_source_shows_only_blocks_that_hold_what_the_page_edits() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    app.source_shown = true;
    // the fixture sets nothing on Workspaces, though both of its settings
    // live in blocks it has
    goto(&mut app, Page::Workspaces);
    assert!(shows(
        &app,
        WIDE,
        "Nothing on this page is in your config yet. niri uses its defaults."
    ));
    goto(&mut app, Page::Animations);
    assert!(!shows(
        &app,
        WIDE,
        "Nothing on this page is in your config yet. niri uses its defaults."
    ));
}

// ---- saving, undo, the history ----

#[test]
fn an_edit_is_saved_after_a_pause() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    let _ = step(
        &mut app,
        Message::Setting(Setting::LayoutGaps, Edit::Draft("20".into())),
    );
    assert!(app.session.due().is_some(), "the wait starts");
    assert_eq!(app.session.pending(), 1);
    // a message that changes nothing keeps the wait as it is
    let due = app.session.due();
    let _ = step(&mut app, Message::RunningTick);
    assert_eq!(app.session.due(), due);
    // a tick before the wait is over saves nothing
    let _ = step(&mut app, Message::SaveTick);
    assert_eq!(app.session.pending(), 1);
    app.session.hurry();
    apply(&mut app, [Message::SaveTick]);
    assert_eq!(app.session.pending(), 0);
    assert_eq!(app.session.history.len(), 1);
    assert_eq!(
        app.session.history[0].summary,
        vec!["Gaps 12 → 20".to_string()]
    );
}

#[test]
fn restoring_a_snapshot_is_one_change() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    app.files.snapshot_name = "roomy".into();
    apply(
        &mut app,
        [
            Message::Setting(Setting::LayoutGaps, Edit::Draft("40".into())),
            Message::SaveNow,
            Message::Files(files::Message::CreateSnapshot),
            Message::Setting(Setting::LayoutGaps, Edit::Draft("8".into())),
            Message::SaveNow,
            Message::Files(files::Message::RestoreSnapshot("roomy".into())),
        ],
    );
    wait_for_save(&mut app);
    assert!(document(&app).contains("gaps 40"), "{}", document(&app));
    assert_eq!(app.session.pending(), 0);
    assert_eq!(app.session.history.len(), 3);
}

#[test]
fn any_version_in_the_history_can_be_restored_and_undo_takes_it_back() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    let original = document(&app);
    apply(
        &mut app,
        [
            Message::Setting(Setting::LayoutGaps, Edit::Draft("20".into())),
            Message::SaveNow,
            Message::Setting(Setting::LayoutGaps, Edit::Draft("24".into())),
            Message::SaveNow,
        ],
    );
    let first = app.session.history[0].time;
    goto(&mut app, Page::History);
    assert!(shows(&app, WIDE, "Current"));
    assert!(shows(&app, WIDE, "Before these changes"));
    apply(
        &mut app,
        [Message::History(crate::history::Message::Restore(
            Version::After(first),
        ))],
    );
    assert!(document(&app).contains("gaps 20"), "{}", document(&app));
    assert_eq!(app.session.history.len(), 3);
    let restored = niri_center::summary::title(app.session.history.last().unwrap());
    assert!(
        restored.starts_with("Restored the version from "),
        "{restored}"
    );
    assert_eq!(
        app.history_view.selected, None,
        "the new current version shows"
    );
    assert_eq!(
        app.session.history.last().unwrap().source,
        niri_center::history::Source::Restored
    );
    apply(&mut app, [Message::Undo]);
    assert!(document(&app).contains("gaps 24"));
    apply(
        &mut app,
        [Message::History(crate::history::Message::Restore(
            Version::Start,
        ))],
    );
    assert_eq!(document(&app), original);
}

#[test]
fn an_edit_in_another_program_lands_in_the_history() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    let original = document(&app);
    let path = store(&app).path().to_path_buf();
    std::fs::write(&path, original.replace("gaps 12", "gaps 30")).unwrap();
    // niri noticed the file and loaded it
    apply(&mut app, [Message::NiriConfigLoaded(false)]);
    assert!(document(&app).contains("gaps 30"));
    let entry = app.session.history.last().unwrap();
    assert_eq!(entry.source, niri_center::history::Source::Outside);
    goto(&mut app, Page::History);
    assert!(shows(&app, WIDE, "Edited in another program"));
    assert!(shows(&app, WIDE, "Gaps 12 → 30"));
    apply(&mut app, [Message::Undo]);
    assert_eq!(document(&app), original);
}

#[test]
fn the_history_shows_one_change_at_a_time_in_a_narrow_column() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    apply(
        &mut app,
        [
            Message::Setting(Setting::LayoutGaps, Edit::Draft("20".into())),
            Message::SaveNow,
        ],
    );
    goto(&mut app, Page::History);
    app.window_width = THIRD.width;
    let time = app.session.history[0].time;
    apply(
        &mut app,
        [Message::History(crate::history::Message::Pick(Some(
            Version::After(time),
        )))],
    );
    assert!(shows(&app, THIRD, "All versions"));
    apply(
        &mut app,
        [Message::Shortcut(crate::shortcuts::Shortcut::Close)],
    );
    assert_eq!(app.history_view.selected, None, "Esc goes back to the list");
}

#[test]
fn a_change_niri_turns_down_is_not_written_and_undo_drops_it() {
    if !niri_center::validate::niri_available() {
        return;
    }
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    app.session.platform = Platform::Desktop;
    let path = store(&app).path().to_path_buf();
    let on_disk = std::fs::read_to_string(&path).unwrap();
    // niri rejects a key bound twice in one file
    let key = store(&app).binding_rows()[0].key.clone();
    app.session
        .store
        .as_mut()
        .unwrap()
        .add_binding(&key, "quit;")
        .unwrap();
    apply(&mut app, [Message::SaveNow]);
    assert!(
        matches!(app.session.problem, Some(Problem::Rejected(_))),
        "{:?}",
        app.session.problem
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), on_disk);
    assert!(shows(&app, WIDE, "Not saved"));
    // closing asks, since the change would be lost
    apply(&mut app, [Message::CloseRequested]);
    assert_eq!(app.closing, Some(crate::app::Closing::Asking));
    assert!(shows(
        &app,
        WIDE,
        "niri does not accept it, so it is not saved. Everything before it is."
    ));
    click(&mut app, "Keep editing");
    click(&mut app, "Undo");
    assert_eq!(app.session.pending(), 0);
    assert_eq!(app.session.problem, None);
    assert!(app.session.history.is_empty(), "nothing was saved");
}

// ---- the review findings ----

#[test]
fn ctrl_z_and_enter_in_a_text_field_stay_with_the_field() {
    use iced::keyboard::{Event, Key, Location, Modifiers, key};
    let press = |key: Key, modifiers: Modifiers| Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: key::Physical::Unidentified(key::NativeCode::Unidentified),
        location: Location::Standard,
        modifiers,
        text: None,
        repeat: false,
    };
    let undo = press(Key::Character("z".into()), Modifiers::CTRL);
    let enter = press(Key::Named(key::Named::Enter), Modifiers::empty());
    // a focused field took the key: not the app's
    assert!(crate::shortcuts::from_event(&undo, true).is_none());
    assert!(crate::shortcuts::from_event(&enter, true).is_none());
    assert!(matches!(
        crate::shortcuts::from_event(&undo, false),
        Some(crate::shortcuts::Shortcut::Undo)
    ));
}

#[test]
fn saving_waits_while_niri_cannot_read_the_config_and_blames_nobody() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    app.session.platform = Platform::Desktop;
    // found at start, before any change
    apply(
        &mut app,
        [Message::Disk(Box::new(Reply::Checked(Err(
            "unexpected node `gpas`".into(),
        ))))],
    );
    let _ = step(
        &mut app,
        Message::Setting(Setting::LayoutGaps, Edit::Draft("20".into())),
    );
    apply(&mut app, [Message::SaveNow]);
    assert_eq!(
        app.session.pending(),
        1,
        "nothing written while niri can't read it"
    );
    assert!(app.session.problem.is_none(), "the change is not blamed");
    assert!(shows(&app, WIDE, "Waiting until niri can read the config"));
    // niri reads it again: what waited is saved by the next save
    app.session.platform = Platform::InMemory;
    apply(
        &mut app,
        [Message::NiriConfigLoaded(false), Message::SaveNow],
    );
    assert_eq!(app.session.pending(), 0);
}

#[test]
fn an_edit_made_while_a_save_is_written_gets_its_own_save() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    let path = store(&app).path().to_path_buf();
    apply(
        &mut app,
        [Message::Setting(
            Setting::LayoutGaps,
            Edit::Draft("20".into()),
        )],
    );
    app.session.hurry();
    // the tick sends the save; the disk actor has not answered yet
    let _ = step(&mut app, Message::SaveTick);
    assert!(app.session.saving());
    let _ = step(
        &mut app,
        Message::Setting(Setting::LayoutGaps, Edit::Draft("22".into())),
    );
    settle(&mut app);
    assert!(std::fs::read_to_string(&path).unwrap().contains("gaps 20"));
    assert_eq!(app.session.pending(), 1, "the later edit waits for its own");
    wait_for_save(&mut app);
    assert!(std::fs::read_to_string(&path).unwrap().contains("gaps 22"));
    assert_eq!(app.session.pending(), 0);
}

#[test]
fn a_change_niri_turns_down_does_not_hold_back_the_ones_before_it() {
    if !niri_center::validate::niri_available() {
        return;
    }
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    app.session.platform = Platform::Desktop;
    let path = store(&app).path().to_path_buf();
    // a good change, then a binding niri rejects (a key bound twice)
    let _ = step(
        &mut app,
        Message::Setting(Setting::LayoutGaps, Edit::Draft("20".into())),
    );
    let key = store(&app).binding_rows()[0].key.clone();
    let _ = step(
        &mut app,
        Message::Environment(environment::Message::NewName(String::new())),
    );
    app.session
        .store
        .as_mut()
        .unwrap()
        .add_binding(&key, "quit;")
        .unwrap();
    // the edit is noticed as any is, after a message
    apply(
        &mut app,
        [
            Message::Environment(environment::Message::NewName(String::new())),
            Message::SaveNow,
        ],
    );
    assert!(matches!(app.session.problem, Some(Problem::Rejected(_))));
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(on_disk.contains("gaps 20"), "the good change is saved");
    // Undo drops only the rejected one
    apply(&mut app, [Message::Undo]);
    assert_eq!(app.session.pending(), 0);
    assert!(document(&app).contains("gaps 20"));
}

#[test]
fn a_read_only_config_says_so_and_offers_the_change_as_a_diff() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    let path = store(&app).path().to_path_buf();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
    // root writes a read-only file anyway (a container): nothing to find
    if std::fs::OpenOptions::new().append(true).open(&path).is_ok() {
        return;
    }
    app.session.read_only = reader().load(&path).unwrap().read_only;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(app.session.read_only, vec![path]);
    assert!(shows(
        &app,
        WIDE,
        "config.kdl is read-only here, for example a link into the Nix store, so changes are not saved. Try them out, then copy them as a diff."
    ));
    apply(
        &mut app,
        [
            Message::Setting(Setting::LayoutGaps, Edit::Draft("20".into())),
            Message::SaveNow,
        ],
    );
    assert!(matches!(app.session.problem, Some(Problem::ReadOnly(_))));
    click(&mut app, "Copy as a diff");
    assert_eq!(status_text(&app), "Copied what is not saved as a diff.");
    assert!(app.session.pending_patch().contains("+    gaps 20"));
}

#[test]
fn a_typed_scale_is_tried_and_a_second_change_joins_the_trial() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    let original = document(&app);
    goto(&mut app, Page::Displays);
    apply(
        &mut app,
        [
            Message::Output(
                "DP-1".into(),
                OutputSetting::Scale,
                Edit::Draft("1.25".into()),
            ),
            Message::SaveNow,
        ],
    );
    assert!(app.session.trial.is_some(), "typed, then saved: on trial");
    apply(
        &mut app,
        [Message::Output(
            "DP-1".into(),
            OutputSetting::Transform,
            Edit::Choice(Some("90".into())),
        )],
    );
    // the countdown is on every page
    goto(&mut app, Page::Layout);
    assert!(shows(&app, WIDE, "Keep"));
    // going back takes both
    apply(&mut app, [Message::EndTrial]);
    assert_eq!(document(&app), original);
}

#[test]
fn closing_saves_what_waits_and_closes_without_asking() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    let path = store(&app).path().to_path_buf();
    let _ = step(
        &mut app,
        Message::Setting(Setting::LayoutGaps, Edit::Draft("20".into())),
    );
    apply(&mut app, [Message::CloseRequested]);
    assert!(std::fs::read_to_string(&path).unwrap().contains("gaps 20"));
    assert_eq!(app.session.save_state(), SaveState::Saved);
    assert_eq!(
        app.closing,
        Some(crate::app::Closing::AfterSave),
        "no dialog"
    );
}

#[test]
fn closing_with_changes_that_wait_for_save_asks_and_can_save_them() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    let path = store(&app).path().to_path_buf();
    apply(
        &mut app,
        [Message::Files(files::Message::SaveWhen(
            SaveWhen::OnRequest,
        ))],
    );
    let _ = step(
        &mut app,
        Message::Setting(Setting::LayoutGaps, Edit::Draft("20".into())),
    );
    apply(&mut app, [Message::CloseRequested]);
    assert_eq!(app.closing, Some(crate::app::Closing::Asking));
    assert!(shows(&app, WIDE, "Save before closing?"));
    assert!(shows(
        &app,
        WIDE,
        "1 file not saved. Saving waits for Save, as the Files page sets it."
    ));
    click(&mut app, "Save and close");
    assert!(std::fs::read_to_string(&path).unwrap().contains("gaps 20"));
    assert_eq!(app.closing, Some(crate::app::Closing::AfterSave));
}

#[test]
fn closing_after_another_program_wrote_the_file_says_so() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    let path = store(&app).path().to_path_buf();
    let _ = step(
        &mut app,
        Message::Setting(Setting::LayoutGaps, Edit::Draft("20".into())),
    );
    let theirs = std::fs::read_to_string(&path).unwrap() + "// theirs\n";
    std::fs::write(&path, &theirs).unwrap();
    apply(&mut app, [Message::CloseRequested]);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), theirs);
    assert_eq!(app.closing, Some(crate::app::Closing::Asking));
    assert!(shows(
        &app,
        WIDE,
        "Another program changed config.kdl meanwhile, so it is not saved."
    ));
    assert!(shows(&app, WIDE, "Close without it"));
    assert!(!shows(&app, WIDE, "Save and close"));
}

#[test]
fn closing_while_niri_cannot_read_the_config_says_what_is_dropped() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    app.session.platform = Platform::Desktop;
    apply(
        &mut app,
        [Message::Disk(Box::new(Reply::Checked(Err(
            "unexpected node `gpas`".into(),
        ))))],
    );
    let _ = step(
        &mut app,
        Message::Setting(Setting::LayoutGaps, Edit::Draft("20".into())),
    );
    apply(&mut app, [Message::CloseRequested]);
    assert_eq!(app.closing, Some(crate::app::Closing::Asking));
    assert!(shows(
        &app,
        WIDE,
        "niri cannot read the config on disk, so saving waits until it can. Closing drops what waits."
    ));
    click(&mut app, "Keep editing");
    assert_eq!(app.closing, None);
}

#[test]
fn closing_during_a_trial_goes_back_first() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    let original = document(&app);
    apply(
        &mut app,
        [
            Message::Output(
                "DP-1".into(),
                OutputSetting::Transform,
                Edit::Choice(Some("90".into())),
            ),
            Message::CloseRequested,
        ],
    );
    assert!(app.session.trial.is_none());
    assert_eq!(document(&app), original);
}

#[test]
fn a_display_position_can_be_typed_left_of_the_others() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Displays);
    apply(
        &mut app,
        [
            Message::Displays(displays::Message::Position(
                "DP-1".into(),
                Coordinate::X,
                "-2560".into(),
            )),
            Message::SaveNow,
        ],
    );
    assert!(
        document(&app).contains("position x=-2560 y=0"),
        "{}",
        document(&app)
    );
    assert!(app.session.trial.is_some());
    assert_niri_accepts(&app);
}

#[test]
fn a_rule_from_a_window_leaves_the_title_out_until_asked() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    apply(
        &mut app,
        [Message::Rules(rules::Message::NewForWindow(
            "kitty".into(),
            "~/code".into(),
        ))],
    );
    assert!(app.rules.draft.rule().title.is_none());
    click(&mut app, "Also match the title \"~/code\"");
    let rule = app.rules.draft.rule();
    assert_eq!(rule.title.as_deref(), Some("~/code"));
    assert_eq!(rule.title_how, niri_center::store::TextMatch::Contains);
}

#[test]
fn a_rule_reads_as_which_windows_and_when() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Rules);
    apply(
        &mut app,
        [
            Message::Rules(rules::Message::NewForWindow(
                "kitty".into(),
                "~/code".into(),
            )),
            Message::Rules(rules::Message::TitleFromWindow),
            // the title goes again: empty is any title
            Message::Rules(rules::Message::Title(String::new())),
            Message::Rules(rules::Message::When(Some(
                niri_center::rules::When::NotFocused,
            ))),
            Message::Rules(rules::Message::Property(RuleProperty::Transparent, true)), // slightly transparent
        ],
    );
    let kdl = app.rules.draft.rule().to_kdl();
    assert!(
        kdl.contains("match app-id=r#\"^kitty$\"# is-focused=false"),
        "{kdl}"
    );
    assert!(shows(&app, WIDE, "When it opens"));
    assert!(shows(&app, WIDE, "For as long as it matches"));
    click(&mut app, "Add rule");
    assert_niri_accepts(&app);
}

#[test]
fn a_known_app_gets_its_suggested_rules_beside_it() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    if let Some(data) = &mut app.running {
        data.windows.push(OpenWindow {
            id: 9,
            app_id: "niri-center.displays".into(),
            title: "niri-center: Displays".into(),
            floating: true,
            focused: false,
        });
    }
    goto(&mut app, Page::Rules);
    // the index says the library knows the app
    assert_eq!(
        niri_center::library::missing_for(crate::library::in_config(&app), "niri-center.displays"),
        1
    );
    assert!(shows(&app, TALL, "1 suggested"));
    apply(
        &mut app,
        [Message::Rules(rules::Message::FromWindow(
            "niri-center.displays".into(),
            "niri-center: Displays".into(),
        ))],
    );
    assert!(shows(&app, TALL, "Suggested for niri-center"));
    click(&mut app, "Add this rule");
    assert!(store(&app).has_niri_center_rules());
    // what the config has is not suggested again
    assert!(!shows(&app, TALL, "Suggested for niri-center"));
    assert!(!shows(&app, TALL, "Add this rule"));
    assert_eq!(
        niri_center::library::missing_for(crate::library::in_config(&app), "niri-center.displays"),
        0,
        "nothing left to suggest"
    );
}

#[test]
fn saving_on_request_waits_for_save_and_shows_the_diff_first() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    apply(
        &mut app,
        [Message::Files(files::Message::SaveWhen(
            niri_center::session::SaveWhen::OnRequest,
        ))],
    );
    let _ = step(
        &mut app,
        Message::Setting(Setting::LayoutGaps, Edit::Draft("20".into())),
    );
    assert!(app.session.due().is_none(), "no save on its own");
    assert!(shows(&app, WIDE, "1 file not saved"));
    click(&mut app, "Review");
    assert!(shows(&app, WIDE, "Not saved yet"));
    click(&mut app, "Save these changes");
    assert_eq!(app.session.pending(), 0);
    assert!(!app.review_open);
}

// ---- niri's reloads ----

#[test]
fn a_name_niri_does_not_know_gets_a_one_click_fix() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    let original = document(&app);
    let path = store(&app).path().to_path_buf();
    std::fs::write(&path, original.replace("gaps 12", "gpas 12")).unwrap();
    let line = 1 + original[..original.find("gaps 12").unwrap()]
        .matches('\n')
        .count();
    apply(
        &mut app,
        [
            Message::NiriConfigLoaded(true),
            Message::Disk(Box::new(Reply::Diagnosed(Err(
                "Error:   × error loading config\n  ╰─▶ unexpected node `gpas`".into(),
            )))),
        ],
    );
    assert!(shows(
        &app,
        WIDE,
        &format!("Line {line}: niri doesn't know gpas. Did you mean gaps?")
    ));
    click(&mut app, &format!("Show line {line} here"));
    click(&mut app, "Change to gaps");
    wait_for_save(&mut app);
    assert_eq!(document(&app), original);
    assert!(app.session.health().is_none());
    assert_eq!(
        app.session.history.len(),
        2,
        "the edit elsewhere, then the fix: both undoable"
    );
}

#[test]
fn a_failed_reload_in_niri_raises_a_banner_with_the_reason() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    apply(&mut app, [Message::NiriConfigLoaded(true)]);
    assert!(shows(
        &app,
        WIDE,
        "niri could not load your config after a change on disk. It keeps running the last version that loaded."
    ));
    apply(
        &mut app,
        [Message::Disk(Box::new(Reply::Diagnosed(Err(
            "× expected a value".into(),
        ))))],
    );
    assert!(shows(&app, WIDE, "× expected a value"));
    click(&mut app, "Dismiss");
    assert!(app.session.health().is_none());
}

// ---- controls ----

#[test]
fn a_color_field_takes_every_syntax_niri_reads() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    for color in [
        "rgb(255, 127, 0)",
        "hsl(200, 80%, 70%)",
        "rebeccapurple",
        "#7fc8ff80",
    ] {
        apply(
            &mut app,
            [Message::Setting(
                Setting::LayoutFocusRingActiveColor,
                Edit::Draft(color.into()),
            )],
        );
        assert!(
            app.session
                .drafts
                .error(&DraftKey::Setting(Setting::LayoutFocusRingActiveColor))
                .is_none(),
            "{color} rejected"
        );
        assert!(document(&app).contains(&format!("active-color \"{color}\"")));
        assert_niri_accepts(&app);
    }
}

#[test]
fn the_picker_writes_the_color_it_shows() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Layout);
    let target = Message::Setting(
        Setting::LayoutBorderActiveColor,
        Edit::Draft(crate::controls::HOLE.to_string()),
    );
    apply(
        &mut app,
        [Message::OpenColor(
            crate::app::ColorField {
                key: DraftKey::Setting(Setting::LayoutBorderActiveColor),
                end: None,
            },
            crate::color::parse("#ffc87f"),
            Box::new(target),
        )],
    );
    assert!(shows(&app, TALL, "Pick from screen"));
    let red = crate::color::Hsva {
        hue: 0.0,
        saturation: 1.0,
        value: 1.0,
        alpha: 0.5,
    };
    apply(&mut app, [Message::ColorPicked(red)]);
    assert!(
        document(&app).contains("active-color \"#ff000080\""),
        "{}",
        document(&app)
    );
    click(&mut app, "Done");
    assert!(app.color.is_none());
}

#[test]
fn a_setting_that_depends_on_an_off_section_says_so() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Layout);
    // niri leaves the border off unless the config turns it on
    assert!(shows(&app, TALL, "Only used while Border is on."));
    apply(
        &mut app,
        [Message::Setting(
            Setting::LayoutBorder,
            Edit::OnOff(niri_center::settings::OnOff::On),
        )],
    );
    assert!(!shows(&app, TALL, "Only used while Border is on."));
}

#[test]
fn a_gradient_is_staged_and_the_color_it_replaces_says_so() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Layout);
    click(&mut app, "Add a gradient");
    let out = document(&app);
    assert!(
        out.contains("active-gradient from=\"#80c8ff\" to=\"#bbddff\" angle=45"),
        "{out}"
    );
    assert!(shows(&app, TALL, "Active gradient is used instead."));
    assert_niri_accepts(&app);
}

#[test]
fn column_widths_are_written_as_one_block() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Layout);
    // niri's thirds show until the config sets its own
    assert!(shows(&app, TALL, "0.66667"));
    click(&mut app, "Full");
    let out = document(&app);
    assert!(
        out.contains(
            "preset-column-widths { proportion 0.33333; proportion 0.5; proportion 0.66667; proportion 1.0; }"
        ),
        "{out}"
    );
    apply(
        &mut app,
        [Message::Setting(
            Setting::LayoutDefaultColumnWidth,
            Edit::Choice(Some(String::new())),
        )],
    );
    assert!(document(&app).contains("default-column-width { }"));
    assert_niri_accepts(&app);
}

#[test]
fn not_saving_screenshots_writes_null() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    apply(
        &mut app,
        [Message::Setting(
            Setting::ScreenshotPath,
            Edit::Draft(niri_center::settings::NO_FILE.into()),
        )],
    );
    assert!(document(&app).contains("screenshot-path null"));
    assert_niri_accepts(&app);
    goto(&mut app, Page::Screenshots);
    assert!(shows(&app, TALL, "Screenshots still go to the clipboard."));
}

#[test]
fn a_chosen_folder_keeps_the_file_name() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    let target = Message::Setting(
        Setting::ScreenshotPath,
        Edit::Draft(format!("{}/Shot %F.png", crate::controls::HOLE)),
    );
    apply(
        &mut app,
        [Message::Browsed(
            Box::new(target),
            Ok(Some("~/Shots".into())),
        )],
    );
    assert!(document(&app).contains("screenshot-path \"~/Shots/Shot %F.png\""));
}

#[test]
fn a_per_output_layout_override_is_named_on_the_global_row() {
    let source =
        format!("{UI_FIXTURE}\noutput \"DP-1\" {{\n    layout {{\n        gaps 32\n    }}\n}}\n");
    let (_dir, mut app) = loaded_app(&source);
    goto(&mut app, Page::Layout);
    assert!(shows(
        &app,
        TALL,
        "output \"DP-1\" (32) sets its own value, used there instead."
    ));
}

#[test]
fn a_value_in_a_file_dankmaterialshell_rewrites_is_flagged() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("dms")).unwrap();
    std::fs::write(
        dir.path().join("config.kdl"),
        "layout {\n    gaps 8\n}\ninclude \"dms/colors.kdl\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("dms/colors.kdl"),
        "layout {\n    focus-ring {\n        active-color \"#d0bcff\"\n    }\n}\n",
    )
    .unwrap();
    let mut app = App::default();
    load_into(&mut app, load(dir.path().join("config.kdl")));
    goto(&mut app, Page::Layout);
    assert!(shows(
        &app,
        TALL,
        "This value is in dms/colors.kdl, which DankMaterialShell rewrites, so a change here may not last."
    ));
    assert!(shows(&app, TALL, "dms/colors.kdl:3"));
}

// ---- displays ----

#[test]
fn a_staged_scale_that_makes_displays_overlap_is_pointed_out() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Displays);
    assert!(!shows(
        &app,
        TALL,
        "eDP-1 and DP-1 overlap. niri moves overlapping displays apart on its own, so drag them edge to edge."
    ));
    // 2560 px at 1.25 is 2048 logical: past DP-1, which starts at 1707
    apply(
        &mut app,
        [Message::Output(
            "eDP-1".into(),
            OutputSetting::Scale,
            Edit::Draft("1.25".into()),
        )],
    );
    assert!(shows(
        &app,
        TALL,
        "DP-1 and eDP-1 overlap. niri moves overlapping displays apart on its own, so drag them edge to edge."
    ));
    apply(
        &mut app,
        [Message::Displays(displays::Message::Pick("eDP-1".into()))],
    );
    assert!(shows(
        &app,
        TALL,
        "The desktop on it is 2048 × 1280 logical pixels."
    ));
}

#[test]
fn the_only_display_that_is_on_cannot_be_turned_off() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    app.outputs.retain(|output| output.name == "eDP-1");
    goto(&mut app, Page::Displays);
    assert!(shows(
        &app,
        TALL,
        "This is the only display that is on, so it stays on."
    ));
}

// ---- window rules ----

#[test]
fn every_library_rule_is_one_niri_accepts() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Rules);
    apply(
        &mut app,
        [
            Message::Rules(rules::Message::OpenLibrary(true)),
            Message::Rules(rules::Message::LibraryAllApps(true)),
            Message::Rules(rules::Message::LibraryIntoConfig(true)),
        ],
    );
    for index in 0..niri_center::library::LIBRARY.len() {
        apply(
            &mut app,
            [Message::Rules(rules::Message::LibraryMark(index, true))],
        );
    }
    apply(&mut app, [Message::Rules(rules::Message::ImportLibrary)]);
    assert_eq!(
        status_text(&app),
        format!("Imported {} rules.", niri_center::library::LIBRARY.len())
    );
    assert_niri_accepts(&app);
    apply(
        &mut app,
        [Message::Rules(rules::Message::OpenLibrary(false))],
    );
    let rows = store(&app).window_rule_rows();
    let last = rows.len() - 1;
    apply(&mut app, [Message::Rules(rules::Message::Open(last))]);
    assert!(shows(&app, TALL, &niri_center::rules::name(&rows[last])));
}

#[test]
fn a_library_rule_added_while_saving_waits_for_save_is_not_saved_yet() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    apply(
        &mut app,
        [
            Message::Files(files::Message::SaveWhen(
                niri_center::session::SaveWhen::OnRequest,
            )),
            Message::Rules(rules::Message::AddLibraryRule(0)),
        ],
    );
    wait_for_save(&mut app);
    assert_eq!(app.session.pending(), 1, "waits for Save");
    apply(&mut app, [Message::SaveNow]);
    assert_eq!(app.session.pending(), 0);
}

#[test]
fn the_library_lists_installed_apps_and_imports_into_its_own_file() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    app.system = niri_center::system::SystemData {
        desktop_ids: ["steam".to_string()].into(),
        ..niri_center::system::SystemData::default()
    };
    goto(&mut app, Page::Rules);
    click(&mut app, "Rule library");
    assert!(shows(&app, TALL, "Steam"));
    assert!(!shows(&app, TALL, "KeePassXC"), "not installed here");
    apply(
        &mut app,
        [Message::Rules(rules::Message::LibraryApp(Some(
            niri_center::library::KnownApp::Steam,
        )))],
    );
    click(&mut app, "Notifications float in the bottom-right corner");
    click(&mut app, "Import 1 rule");
    wait_for_save(&mut app);
    assert!(document(&app).ends_with("include \"rules-library.kdl\"\n"));
    assert_eq!(app.session.pending(), 0, "saved");
    assert_eq!(store(&app).window_rule_rows().len(), 2);
}

#[test]
fn a_rule_edited_as_text_replaces_only_that_rule() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Rules);
    apply(&mut app, [Message::Rules(rules::Message::EditText(0))]);
    let edited = "window-rule {\n    match app-id=\"firefox$\" title=\"^Picture-in-Picture$\"\n    open-floating true\n    opacity 0.8\n}";
    app.rules.editor = Some((0, iced::widget::text_editor::Content::with_text(edited)));
    click(&mut app, "Save changes");
    let out = document(&app);
    assert!(out.ends_with(&format!("{edited}\n")), "{out}");
    assert_eq!(
        out.strip_suffix(&format!("{edited}\n")),
        UI_FIXTURE.strip_suffix(&format!("{}\n", store_rule(UI_FIXTURE)))
    );
    assert_niri_accepts(&app);
}

#[test]
fn nested_rule_lines_show_evenly_indented() {
    assert_eq!(
        niri_center::words::dedent("background-effect {\n            blur true\n        }"),
        "background-effect {\n    blur true\n}"
    );
}

/// The fixture's window rule block, as written.
fn store_rule(source: &str) -> &str {
    &source[source.find("window-rule").unwrap()..source.len() - 1]
}

// ---- keybinds ----

#[test]
fn a_combo_can_be_built_on_the_keyboard_picture() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    apply(&mut app, [Message::Keybinds(keybinds::Message::New(None))]);
    click(&mut app, "Choose keys on the keyboard");
    apply(
        &mut app,
        [
            Message::Keybinds(keybinds::Message::ComposeModifier(
                niri_center::bindings::Modifier::Mod,
            )),
            // modifiers on the picture toggle too
            Message::Keybinds(keybinds::Message::KeyClicked("Shift".into())),
            Message::Keybinds(keybinds::Message::KeyClicked("T".into())),
        ],
    );
    assert_eq!(binding_form(&mut app).key, "Mod+Shift+T");
    assert!(app.keybinds.composing.is_none());
}

#[test]
fn clicking_a_binding_opens_it_with_turn_off_and_remove() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    click(&mut app, "spawn fuzzel");
    let index = binding_index(&app, "Mod+D");
    assert_eq!(binding_form(&mut app).editing, Some(index));
    click(&mut app, "Turn off");
    assert!(document(&app).contains("/- Mod+D"), "{}", document(&app));
}

// ---- every page, every size ----

#[test]
fn every_page_renders_and_names_itself_at_desktop_and_tile_widths() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    for page in Page::ALL {
        goto(&mut app, page);
        for size in [WIDE, NARROW, THIRD] {
            app.window_width = size.width;
            assert!(shows(&app, size, page.label()), "{page:?} at {size:?}");
            assert!(!shows(&app, size, "No config loaded yet."), "{page:?}");
        }
    }
}

#[test]
fn the_browser_demo_renders_every_page_on_its_own_data() {
    let actions = niri_center::bindings::parse_catalog(niri_center::bindings::CATALOG);
    let mut app = App {
        keybinds: keybinds::State {
            action_box: keybinds::action_box(&actions),
            actions,
            ..keybinds::State::default()
        },
        outputs: niri_center::demo::outputs(),
        running: Some(niri_center::demo::running()),
        live_events: niri_center::demo::events().into(),
        ..App::default()
    };
    load_into(&mut app, niri_center::demo::store().unwrap());
    app.displays.monitors = niri_center::outputs::rects(&app.outputs);
    apply(
        &mut app,
        [Message::SystemLoaded(Box::new(niri_center::demo::system()))],
    );
    for page in Page::ALL {
        goto(&mut app, page);
        for size in [WIDE, THIRD] {
            app.window_width = size.width;
            assert!(shows(&app, size, page.label()), "{page:?} at {size:?}");
        }
    }
}

#[test]
fn in_a_third_width_column_actions_keep_a_readable_width() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    app.window_width = THIRD.width;
    // the config source leaves the page, so the content gets the width
    assert!(!app.shows_source());
    let mut ui = ui(&app, THIRD);
    for action in ["show-hotkey-overlay", "spawn kitty", "close-window"] {
        let bounds = ui.find(action).unwrap().bounds();
        assert!(
            bounds.width > 3.0 * bounds.height,
            "{action} squeezed to {}x{}",
            bounds.width,
            bounds.height
        );
    }
}

#[test]
fn in_a_third_width_column_setting_controls_sit_under_their_labels() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    app.window_width = THIRD.width;
    goto(&mut app, Page::Layout);
    let mut ui = ui(&app, THIRD);
    let label = ui.find("Gaps").unwrap().bounds();
    let field = ui.find("12").unwrap().bounds();
    assert!(
        field.y > label.y + label.height,
        "the field is below the label"
    );
}

#[test]
fn old_pane_names_still_open_their_page() {
    for (old, page) in [
        ("monitors", Page::Displays),
        ("curves", Page::Animations),
        ("toolbox", Page::Windows),
        ("live", Page::Events),
        ("config", Page::Files),
        ("keybinds", Page::Keybinds),
    ] {
        assert_eq!(Page::from_slug(old), Some(page), "{old}");
    }
}

// ---- legibility and copy ----

use crate::theme::contrast;

#[test]
fn every_text_color_meets_wcag_in_every_theme() {
    // 4.5:1 is AA for normal-size text (most hint text here is 11-13 px);
    // the high-contrast themes hold text to AAA, 7:1
    for colors in theme::ALL {
        let needed = if matches!(
            colors.palette,
            crate::theme::Palette::HighContrastDark | crate::theme::Palette::HighContrastLight
        ) {
            7.0
        } else {
            4.5
        };
        let text_roles = [
            ("text", colors.text),
            ("muted", colors.muted),
            ("code", colors.code),
            ("accent", colors.accent),
            ("pending", colors.pending),
            ("error", colors.error),
            ("success", colors.success),
        ];
        let grounds = [
            ("page", colors.background),
            ("surface", colors.surface),
            ("canvas", colors.canvas),
        ];
        for (role, color) in text_roles {
            for (ground, background) in grounds {
                let ratio = contrast(color, background);
                assert!(
                    ratio >= needed,
                    "{}: {role} on {ground} is {ratio:.2}:1",
                    colors.palette.name()
                );
            }
        }
        // button labels on every fill a button can have
        for (fill, background) in [
            ("control", colors.control),
            ("hovered control", colors.control_hover),
            ("pressed control", colors.control_pressed),
        ] {
            let ratio = contrast(colors.text, background);
            assert!(
                ratio >= needed,
                "{}: text on {fill} is {ratio:.2}:1",
                colors.palette.name()
            );
        }
        for (role, color, fill) in [
            ("on_accent", colors.on_accent, colors.accent),
            (
                "on_accent",
                colors.on_accent,
                theme::mix(colors.accent, colors.text, 0.25),
            ),
            ("on_error", colors.on_error, colors.error),
            (
                "on_error",
                colors.on_error,
                theme::mix(colors.error, colors.text, 0.2),
            ),
        ] {
            let ratio = contrast(color, fill);
            assert!(
                ratio >= 4.5,
                "{}: {role} is {ratio:.2}:1",
                colors.palette.name()
            );
        }
        // placeholders: iced draws them in the palette's secondary color, on
        // a field filled with the page color or a pick list on `control`
        let placeholder = theme::extended(colors).secondary.base.color;
        for (ground, background) in [("field", colors.background), ("pick list", colors.control)] {
            let ratio = contrast(placeholder, background);
            assert!(
                ratio >= 4.5,
                "{}: placeholder in a {ground} is {ratio:.2}:1",
                colors.palette.name()
            );
        }
    }
}

#[test]
fn key_labels_read_on_every_shade_of_the_keyboard_picture() {
    for colors in theme::ALL {
        let [off, on, hot] = crate::keyboard::heat(colors);
        assert!(
            contrast(colors.muted, off) >= 4.5,
            "{}: unused key",
            colors.palette.name()
        );
        for fill in [on, hot] {
            let label = theme::readable_on(colors, fill);
            let ratio = contrast(label, fill);
            assert!(
                ratio >= 4.5,
                "{}: used key label is {ratio:.2}:1",
                colors.palette.name()
            );
        }
    }
}

#[test]
fn every_control_outline_meets_wcag_non_text_contrast() {
    // WCAG 1.4.11: what you need to find a control clears 3:1 against
    // whatever it sits on
    for colors in theme::ALL {
        for (ground, background) in [("page", colors.background), ("surface", colors.surface)] {
            for (role, color) in [("edge", colors.edge), ("accent", colors.accent)] {
                let ratio = contrast(color, background);
                assert!(
                    ratio >= 3.0,
                    "{}: {role} on {ground} is {ratio:.2}:1",
                    colors.palette.name()
                );
            }
        }
    }
}

/// The app's sources: every widget and every word on screen.
const UI_SOURCES: [(&str, &str); 19] = [
    ("main.rs", include_str!("main.rs")),
    ("app.rs", include_str!("app.rs")),
    ("update.rs", include_str!("update.rs")),
    ("view.rs", include_str!("view.rs")),
    ("rows.rs", include_str!("rows.rs")),
    ("controls.rs", include_str!("controls.rs")),
    ("history.rs", include_str!("history.rs")),
    ("library.rs", include_str!("library.rs")),
    ("repair.rs", include_str!("repair.rs")),
    ("source.rs", include_str!("source.rs")),
    ("shortcuts.rs", include_str!("shortcuts.rs")),
    ("pages/mod.rs", include_str!("pages/mod.rs")),
    ("pages/keybinds.rs", include_str!("pages/keybinds.rs")),
    ("pages/startup.rs", include_str!("pages/startup.rs")),
    ("pages/displays.rs", include_str!("pages/displays.rs")),
    ("pages/animations.rs", include_str!("pages/animations.rs")),
    ("pages/rules.rs", include_str!("pages/rules.rs")),
    ("pages/windows.rs", include_str!("pages/windows.rs")),
    ("pages/files.rs", include_str!("pages/files.rs")),
];

/// The library's sources that hold words the app shows; not the change
/// summaries, whose "12 → 20" is what the arrow is for.
const LIBRARY_COPY: [(&str, &str); 10] = [
    (
        "niri-center/src/settings.rs",
        include_str!("../../niri-center/src/settings.rs"),
    ),
    (
        "niri-center/src/session.rs",
        include_str!("../../niri-center/src/session.rs"),
    ),
    (
        "niri-center/src/words.rs",
        include_str!("../../niri-center/src/words.rs"),
    ),
    (
        "niri-center/src/search.rs",
        include_str!("../../niri-center/src/search.rs"),
    ),
    (
        "niri-center/src/page.rs",
        include_str!("../../niri-center/src/page.rs"),
    ),
    (
        "niri-center/src/typo.rs",
        include_str!("../../niri-center/src/typo.rs"),
    ),
    (
        "niri-center/src/animations.rs",
        include_str!("../../niri-center/src/animations.rs"),
    ),
    (
        "niri-center/src/library.rs",
        include_str!("../../niri-center/src/library.rs"),
    ),
    (
        "niri-center/src/rules.rs",
        include_str!("../../niri-center/src/rules.rs"),
    ),
    (
        "niri-center/src/outputs.rs",
        include_str!("../../niri-center/src/outputs.rs"),
    ),
];

#[test]
fn buttons_use_the_audited_styles() {
    // iced's built-in button styles derive their colors and fade disabled
    // labels to half opacity: they are what made buttons hard to read
    for (file, source) in UI_SOURCES {
        for style in [
            "button::primary",
            "button::secondary",
            "button::danger",
            "button::success",
        ] {
            assert!(!source.contains(style), "{file} uses {style}; use theme::");
        }
    }
}

#[test]
fn ui_copy_has_no_em_dashes_or_decorative_glyphs() {
    // the commonest tells of generated copy (docs/ADR-design-rules.md)
    let banned = ['\u{2014}', '✓', '✔', '✨', '⏸', '☰', '→', '🚀'];
    for (file, source) in UI_SOURCES.iter().chain(&LIBRARY_COPY) {
        for (number, line) in source.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if let Some(glyph) = line.chars().find(|c| banned.contains(c)) {
                panic!("{file}:{}: {glyph:?} in {line:?}", number + 1);
            }
        }
    }
}

#[test]
fn the_overview_backdrop_says_when_a_display_sets_its_own() {
    let (_dir, mut app) = loaded_app("output \"eDP-1\" {\n    backdrop-color \"#3c0000\"\n}\n");
    goto(&mut app, Page::Overview);
    assert!(shows(
        &app,
        WIDE,
        "output \"eDP-1\" (#3c0000) sets its own value, used there instead."
    ));
}

#[test]
fn a_rule_opens_in_the_form_and_saves_in_its_place() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Rules);
    apply(
        &mut app,
        [
            Message::Rules(rules::Message::Open(0)),
            Message::Rules(rules::Message::EditInForm(0)),
        ],
    );
    // a pattern stays a pattern, an exact title reads as the title
    assert_eq!(app.rules.draft.app_how, TextMatch::Pattern);
    assert_eq!(app.rules.draft.app, "firefox$");
    assert_eq!(app.rules.draft.title_how, TextMatch::Exact);
    assert_eq!(app.rules.draft.title, "Picture-in-Picture");
    assert!(shows(&app, WIDE, "Change the rule"));
    apply(
        &mut app,
        [Message::Rules(rules::Message::Property(
            RuleProperty::Transparent,
            true,
        ))],
    ); // slightly transparent
    click(&mut app, "Save changes");
    let text = document(&app);
    assert_eq!(store(&app).window_rule_rows().len(), 1, "{text}");
    assert!(text.contains("opacity 0.92"), "{text}");
    assert!(!text.contains("match app-id=\"firefox$\""), "{text}");
    assert_niri_accepts(&app);
    let (_before_dir, before) = loaded_app(UI_FIXTURE);
    let summary = niri_center::summary::describe(store(&before), store(&app));
    assert_eq!(summary, ["Changed rule: firefox: open floating"]);
}

#[test]
fn a_rule_the_form_cannot_say_stays_text() {
    let (_dir, mut app) = loaded_app(
        "window-rule {\n    match app-id=\"kitty\"\n    // mine\n    min-width 400\n}\n",
    );
    goto(&mut app, Page::Rules);
    apply(&mut app, [Message::Rules(rules::Message::Open(0))]);
    // Edit opens its text: the form cannot say the comment
    assert!(shows(&app, WIDE, "Edit"));
    assert!(!shows(&app, WIDE, "Edit as text"));
    apply(&mut app, [Message::Rules(rules::Message::EditInForm(0))]);
    assert_eq!(app.rules.draft.editing, None);
    click(&mut app, "Edit");
    assert!(app.rules.editor.is_some());
    assert!(!app.rules.dialog);
}

#[test]
fn a_rule_is_removed_from_the_list_and_undo_brings_it_back() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Rules);
    let before = document(&app);
    apply(
        &mut app,
        [
            Message::Rules(rules::Message::Open(0)),
            Message::Rules(rules::Message::Remove(0)),
        ],
    );
    assert!(store(&app).window_rule_rows().is_empty());
    assert_eq!(app.rules.open, None);
    apply(&mut app, [Message::Undo]);
    assert_eq!(document(&app), before);
}

#[test]
fn an_open_rule_and_binding_follow_their_place_through_undo() {
    let (_dir, mut app) = loaded_app(
        "binds {\n    Mod+A { quit; }\n    Mod+B { close-window; }\n}\nwindow-rule {\n    match app-id=\"a\"\n    open-floating true\n}\nwindow-rule {\n    match app-id=\"b\"\n    open-maximized true\n}\n",
    );
    goto(&mut app, Page::Rules);
    apply(
        &mut app,
        [
            Message::Rules(rules::Message::Open(1)),
            Message::Rules(rules::Message::Remove(0)),
        ],
    );
    assert_eq!(app.rules.open, Some(0), "the open rule moved up");
    apply(&mut app, [Message::Undo]);
    assert_eq!(app.rules.open, Some(1), "and back down");
    let index = binding_index(&app, "Mod+B");
    apply(
        &mut app,
        [
            Message::Keybinds(keybinds::Message::Edit(index)),
            Message::Keybinds(keybinds::Message::Remove(0)),
        ],
    );
    assert_eq!(binding_form(&mut app).editing, Some(index - 1));
    apply(&mut app, [Message::Undo]);
    assert_eq!(binding_form(&mut app).editing, Some(index));
    // gone: the form lets go instead of editing what took its place
    apply(
        &mut app,
        [Message::Keybinds(keybinds::Message::Remove(index))],
    );
    assert!(app.keybinds.form.is_none());
}

#[test]
fn a_first_change_does_not_move_the_rows_below() {
    // a setting not in the config yet, with no help line, above another
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Overview);
    let heading_at = |app: &App| ui(app, WIDE).find("Window switcher").unwrap().bounds().y;
    let before = heading_at(&app);
    apply(
        &mut app,
        [Message::Setting(
            Setting::OverviewWorkspaceShadow,
            Edit::Switch(false),
        )],
    );
    assert!(
        app.session.pending() > 0 || !app.session.history.is_empty(),
        "nothing changed"
    );
    assert!(
        (heading_at(&app) - before).abs() < 0.5,
        "moved from {before} to {}",
        heading_at(&app)
    );
}

#[test]
fn a_key_bound_again_in_a_later_file_says_which_one_niri_uses() {
    let (_dir, mut app) = loaded_app_with(
        "binds {\n    Mod+P { spawn \"fuzzel\"; }\n}\ninclude \"agents.kdl\"\n",
        &[("agents.kdl", "binds {\n    Mod+P { spawn \"agent\"; }\n}\n")],
    );
    goto(&mut app, Page::Keybinds);
    assert!(shows(
        &app,
        WIDE,
        "Not used: agents.kdl binds this key later."
    ));
    assert!(shows(
        &app,
        WIDE,
        "Used instead of the binding in config.kdl."
    ));
    assert!(shows(
        &app,
        WIDE,
        "1 binding is not used: its key is bound again in a file niri reads later."
    ));
    assert_niri_accepts(&app);
}

#[test]
fn the_layout_page_says_when_attention_requests_draw_nothing() {
    let note = "With the focus ring and the border off, a window asking for attention looks like any other. A window rule for windows that ask for attention can mark them.";
    let both_off =
        "layout {\n    focus-ring {\n        off\n    }\n    border {\n        off\n    }\n}\n";
    let (_dir, mut app) = loaded_app(both_off);
    goto(&mut app, Page::Layout);
    assert!(shows(&app, WIDE, note));
    // a rule for urgent windows marks them
    let (_dir, mut app) = loaded_app(&format!(
        "{both_off}window-rule {{\n    match is-urgent=true\n    opacity 1.0\n}}\n"
    ));
    goto(&mut app, Page::Layout);
    assert!(!shows(&app, WIDE, note));
    // so does either one being on, as in niri's own default
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Layout);
    assert!(!shows(&app, WIDE, note));
}

#[test]
fn a_rule_in_an_included_file_can_be_removed() {
    let (dir, mut app) = loaded_app_with(
        "include \"window_rules.kdl\"\n",
        &[(
            "window_rules.kdl",
            "window-rule {\n    geometry-corner-radius 15\n}\n\nwindow-rule {\n    match app-id=\"^dev\\\\.zed\\\\.Zed$\"\n    opacity 0.92\n}\n",
        )],
    );
    goto(&mut app, Page::Rules);
    apply(&mut app, [Message::Rules(rules::Message::Remove(1))]);
    assert!(app.session.problem.is_none(), "{:?}", app.session.problem);
    wait_for_save(&mut app);
    let written = std::fs::read_to_string(dir.path().join("window_rules.kdl")).unwrap();
    assert!(!written.contains("dev"), "{written}");
    assert!(written.contains("geometry-corner-radius 15"));
    assert!(app.session.problem.is_none(), "{:?}", app.session.problem);
}

#[test]
fn a_rule_that_applies_to_a_window_is_changed_in_a_dialog_and_the_window_stays() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    if let Some(data) = &mut app.running {
        data.windows.push(OpenWindow {
            id: 20,
            app_id: "firefox".into(),
            title: "Picture-in-Picture".into(),
            floating: true,
            focused: false,
        });
    }
    goto(&mut app, Page::Rules);
    apply(
        &mut app,
        [Message::Rules(rules::Message::FromWindow(
            "firefox".into(),
            "Picture-in-Picture".into(),
        ))],
    );
    // within Firefox, its rule says what it does
    assert!(shows(&app, WIDE, "open floating"));
    // Edit opens the form over the page, filled in
    apply(&mut app, [Message::Rules(rules::Message::Edit(0))]);
    assert!(shows(&app, WIDE, "Change the rule"));
    assert_eq!(app.rules.draft.title, "Picture-in-Picture");
    // Esc closes it and drops what was changed
    apply(
        &mut app,
        [
            Message::Rules(rules::Message::Property(RuleProperty::Transparent, true)),
            Message::Shortcut(crate::shortcuts::Shortcut::Close),
        ],
    );
    assert!(!shows(&app, WIDE, "Change the rule"));
    assert!(!document(&app).contains("opacity 0.92"));
    // saved, the window is still the one shown
    apply(
        &mut app,
        [
            Message::Rules(rules::Message::Edit(0)),
            Message::Rules(rules::Message::Property(RuleProperty::Transparent, true)),
        ],
    );
    click(&mut app, "Save changes");
    assert!(document(&app).contains("opacity 0.92"));
    assert!(!app.rules.dialog);
    assert_eq!(app.rules.open, None);
    assert_eq!(
        app.rules
            .window
            .as_ref()
            .map(|window| window.app_id.as_str()),
        Some("firefox")
    );
}

#[test]
fn the_search_finds_apps_and_rules_on_window_rules_first() {
    use niri_center::by_app::Who;
    let rules: String = (0..10)
        .map(|n| format!("window-rule {{\n    match app-id=\"app{n}\"\n    opacity 0.9\n}}\n"))
        .collect();
    let (_dir, mut app) = loaded_app(&rules);
    goto(&mut app, Page::Rules);
    let search = |query: &str| Message::SearchQuery(query.into());
    apply(&mut app, [search("app3")]);
    // the app and its rule, before settings and pages
    assert!(shows(&app, WIDE, "Window rules"));
    assert!(shows(&app, WIDE, "app3: opacity 0.9"));
    assert!(!shows(&app, WIDE, "app7"));
    apply(
        &mut app,
        [
            Message::Shortcut(crate::shortcuts::Shortcut::Down),
            Message::SearchSubmit,
        ],
    );
    assert!(app.header_search.is_none());
    assert_eq!(app.rules.open, Some(3));
    // a rule's own words find it too
    apply(&mut app, [search("opacity"), Message::SearchSubmit]);
    assert_eq!(app.rules.open, Some(0));
    // the app picked shows its page
    apply(&mut app, [search("app7"), Message::SearchSubmit]);
    assert_eq!(app.rules.app, Some(Who::App("app7".into())));
    assert_eq!(app.rules.open, None);
    // elsewhere the search lists settings and pages only
    goto(&mut app, Page::Layout);
    apply(&mut app, [search("app3")]);
    assert!(!shows(&app, WIDE, "app3: opacity 0.9"));
}

#[test]
fn an_apps_entry_says_how_many_of_its_rules_are_off() {
    let (_dir, mut app) =
        loaded_app("window-rule {\n    match app-id=\"app3\"\n    opacity 0.9\n}\n");
    goto(&mut app, Page::Rules);
    apply(&mut app, [Message::Rules(rules::Message::Open(0))]);
    click(&mut app, "Turn off");
    assert!(document(&app).contains("/- window-rule {\n    match app-id=\"app3\""));
    assert!(shows(&app, WIDE, "1 rule, 1 off"));
}

#[test]
fn a_rule_that_acts_as_a_window_opens_says_when_it_takes_effect() {
    let (_dir, mut app) = loaded_app(UI_FIXTURE);
    goto(&mut app, Page::Rules);
    apply(
        &mut app,
        [
            Message::Rules(rules::Message::New(Some("kitty".into()))),
            Message::Rules(rules::Message::Property(RuleProperty::OpenFloating, true)),
            Message::Rules(rules::Message::Add),
        ],
    );
    assert!(
        status_text(&app).contains("starts with the next one that opens"),
        "{}",
        status_text(&app)
    );
    apply(
        &mut app,
        [
            Message::Rules(rules::Message::New(Some("kitty".into()))),
            Message::Rules(rules::Message::Property(RuleProperty::Transparent, true)),
            Message::Rules(rules::Message::Add),
        ],
    );
    assert!(
        !status_text(&app).contains("next one"),
        "{}",
        status_text(&app)
    );
}
