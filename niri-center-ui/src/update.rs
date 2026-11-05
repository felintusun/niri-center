//! What each message does to the app.

use iced::{Color, Task};
use niri_center::clock::Instant;
use niri_center::diff;
use niri_center::disk::actor::{Purpose, Reply};
use niri_center::niri;
use niri_center::outputs;
use niri_center::page::Page;
use niri_center::session::{SaveState, SaveWhen};
use niri_center::settings;
use num_traits::ToPrimitive;

use crate::app::{App, Closing, ColorEdit, TOAST_SECONDS, error, info};
use crate::message::Message;
use crate::{color, pages, shortcuts, tasks, view};

/// One message, then the wait to save: a message that changed the config
/// (re)starts it, and a tick comes when it is over.
pub fn step(state: &mut App, message: Message) -> Task<Message> {
    let due = state.session.due();
    let latest = state.session.history.last().map(|entry| entry.time);
    let lists_files = matches!(
        message,
        Message::Page(Page::Files)
            | Message::Files(
                pages::files::Message::CreateSnapshot | pages::files::Message::DeleteSnapshot(_)
            )
    );
    let from_editor = matches!(message, Message::Animations(_));
    let revision = state.store().map(niri_center::store::Store::revision);
    let task = update(state, message);
    // the config changed under the Animations editor (Undo, a restore,
    // another program): it shows the config again
    if !from_editor && state.store().map(niri_center::store::Store::revision) != revision {
        state.reload_animation_editor();
    }
    // a save makes backups: the Files page lists them again
    if state.page == Page::Files
        && (lists_files || state.session.history.last().map(|entry| entry.time) != latest)
    {
        state.list_files();
    }
    // most messages (ticks, the pointer, typing in search) leave the
    // config alone; only an edit costs a copy
    state.session.edited();
    state.refresh_rows();
    state.refresh_motions();
    pages::keybinds::refresh_picture(state);
    if let Some(failure) = state.session.error.take() {
        state.status = error(failure.to_string());
    }
    crate::history::want_texts(state);
    state.send_commands();
    // closing waits for the save it started; what cannot be saved asks
    if state.closing == Some(Closing::AfterSave) {
        match state.session.save_state() {
            SaveState::Saved => return iced::exit(),
            SaveState::Saving => {}
            SaveState::WaitingForNiri | SaveState::NotSaved(_) | SaveState::OnRequest { .. } => {
                state.closing = Some(Closing::Asking);
            }
        }
    }
    // the event stream's lines only while the Events page shows them
    let watching = state.page == Page::Events;
    if watching != state.watching {
        state.watching = watching;
        state.tell_niri(niri::Command::Watch(watching));
    }
    let now_due = state.session.due();
    if now_due == due {
        return task;
    }
    // one timer for the wait: a new wait, or none, drops the one before
    state.save_timer = None;
    let Some(new) = now_due else {
        return task;
    };
    let (timer, handle) = tasks::after(
        new.saturating_duration_since(Instant::now()),
        Message::SaveTick,
    )
    .abortable();
    state.save_timer = Some(handle.abort_on_drop());
    Task::batch([task, timer])
}

/// What the disk actor answers: the session's business first, then the
/// pages'.
fn disk_reply(state: &mut App, reply: Reply) -> Task<Message> {
    if let Reply::Loaded(Ok(loaded)) = &reply {
        state.files.sandbox = niri_center::start::sandbox(state.session.platform, &loaded.store);
    }
    let started = matches!(reply, Reply::Loaded(Ok(_)));
    let unread = matches!(reply, Reply::Checked(Err(_)));
    let history_changed = matches!(reply, Reply::Saved { .. } | Reply::Reloaded(Some(_)));
    let Some(rest) = state.session.reply(reply) else {
        if started {
            // an editor filled before the config came reads it now
            state.reload_animation_editor();
        }
        if history_changed {
            // the start of the history may read differently now
            state.history_view.start_preview = None;
            if state.page == Page::Files {
                state.list_files();
            }
        }
        // the open windows, to find the editor the config is open in
        if unread {
            return state.ask_niri(niri::Command::Running);
        }
        return Task::none();
    };
    match rest {
        Reply::Loaded(Err(failure)) => state.load_error = Some(failure.to_string()),
        Reply::Texts {
            purpose: Purpose::Copy,
            files,
            ..
        } => {
            state.status = info("Copied the change as a diff.");
            return iced::clipboard::write(state.session.patch(&files));
        }
        Reply::Texts { time, files, .. } => {
            let diffs = files
                .iter()
                .map(|file| diff::FileDiff {
                    path: state.session.file_label(&file.path),
                    hunks: diff::diff(&file.before, &file.after),
                })
                .collect();
            state.history_view.diff = Some((time, diffs));
        }
        Reply::Before { files, .. } => {
            // what restoring changes: from each file now to as it was
            let diffs = state.store().map(|store| {
                files
                    .iter()
                    .filter_map(|(path, before)| {
                        let (_, now) = store.saved_texts().find(|(saved, _)| saved == path)?;
                        (now != before).then(|| diff::FileDiff {
                            path: store.file_label(path),
                            hunks: diff::diff(now, before),
                        })
                    })
                    .collect()
            });
            state.history_view.start_preview = diffs;
        }
        Reply::Listing { backups, snapshots } => {
            state.files.listing = pages::files::Listing { backups, snapshots };
        }
        Reply::Backup { name, text } => match text {
            Ok(text) => {
                if let Some(store) = &mut state.session.store
                    && let Err(failure) = store.stage_named(&name, &text)
                {
                    state.status = error(failure.to_string());
                }
            }
            Err(failure) => state.status = error(failure.to_string()),
        },
        Reply::Snapshot { files, .. } => match files {
            Ok(files) => {
                if let Some(store) = &mut state.session.store
                    && let Err(failure) = store.stage_snapshot(&files)
                {
                    state.status = error(failure.to_string());
                }
            }
            Err(failure) => state.status = error(failure.to_string()),
        },
        Reply::Failed(failure) => state.status = error(failure.to_string()),
        Reply::Loaded(Ok(_))
        | Reply::History(_)
        | Reply::Checked(_)
        | Reply::Saved { .. }
        | Reply::Reloaded(_)
        | Reply::Diagnosed(_) => {}
    }
    Task::none()
}

pub fn update(state: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::Disk(reply) => return disk_reply(state, reply),
        Message::Page(page) => {
            state.page = page;
            if state
                .highlight
                .is_some_and(|setting| setting.spec().page != page)
            {
                state.highlight = None;
            }
            match page {
                Page::Windows | Page::Rules => {
                    return state.ask_niri(niri::Command::Running);
                }
                Page::Displays => {
                    return state.ask_niri(niri::Command::Outputs);
                }
                // the editor opens on the animation as the config has it now
                Page::Animations => {
                    state.reload_animation_editor();
                }
                Page::Keybinds
                | Page::Layout
                | Page::Workspaces
                | Page::Overview
                | Page::Input
                | Page::Gestures
                | Page::Startup
                | Page::Environment
                | Page::Cursor
                | Page::Screenshots
                | Page::History
                | Page::Files
                | Page::Events
                | Page::Source => {}
            }
        }
        Message::Keybinds(message) => return crate::pages::keybinds::update(state, message),
        Message::Environment(message) => return crate::pages::environment::update(state, message),
        Message::Startup(message) => return crate::pages::startup::update(state, message),
        // ---- registry and outputs ----
        Message::Setting(id, edit) => {
            follow_picker(state, &settings::DraftKey::Setting(id), &edit);
            if let Some(store) = &mut state.session.store
                && let Err(failure) = settings::edit(store, &mut state.session.drafts, id, edit)
            {
                state.status = error(failure.to_string());
            }
        }
        Message::Output(output, child, edit) => {
            follow_picker(
                state,
                &settings::DraftKey::Output(output.clone(), child),
                &edit,
            );
            // a picked value is tried at once; typing waits for its save.
            // Edits waiting for theirs go first, so the trial is this alone
            let typed = matches!(edit, settings::Edit::Draft(_));
            let connected = &state.outputs;
            if let Err(failure) = state.session.change_display(typed, |store, drafts| {
                settings::edit_output(store, drafts, connected, &output, child, edit)
            }) {
                state.status = error(failure.to_string());
            }
        }
        Message::OpenColor(field, shown, target) => {
            if state.color.as_ref().is_some_and(|open| open.field == field) {
                state.color = None;
            } else {
                state.color = Some(ColorEdit {
                    field,
                    target,
                    hsva: color::hsva(shown.unwrap_or(Color::WHITE), color::Hsva::default()),
                });
            }
        }
        Message::ColorPicked(hsva) => {
            let Some(open) = &mut state.color else {
                return Task::none();
            };
            open.hsva = hsva;
            let filled = open.target.filled(&hsva.hex());
            return update(state, filled);
        }
        Message::PickScreenColor => {
            state.status = info("Click anywhere on the screen to take its color. Escape cancels.");
            return state.ask_niri(niri::Command::PickColor);
        }
        Message::ScreenColor(result) => match result {
            Ok(Some([red, green, blue])) => {
                state.status = None;
                // the portal's channels run from 0 to 1, well inside f32
                if let Some(open) = &state.color
                    && let (Some(red), Some(green), Some(blue)) =
                        (red.to_f32(), green.to_f32(), blue.to_f32())
                {
                    let picked = Color::from_rgba(red, green, blue, open.hsva.alpha);
                    let hsva = color::hsva(picked, open.hsva);
                    return update(state, Message::ColorPicked(hsva));
                }
            }
            Ok(None) => state.status = info("No color picked."),
            Err(failure) => state.status = error(format!("niri could not pick a color: {failure}")),
        },
        Message::CloseColor => state.color = None,
        Message::Browse { to, folder } => {
            return Task::perform(niri_center::portal::choose(folder), move |result| {
                Message::Browsed(to, result)
            });
        }
        Message::Browsed(to, result) => match result {
            Ok(Some(path)) => return update(state, to.filled(&path)),
            Ok(None) => {}
            Err(failure) => state.status = error(failure),
        },
        Message::SystemLoaded(data) => {
            state.layout_box = iced::widget::combo_box::State::new(data.layouts.clone());
            state.option_box = iced::widget::combo_box::State::new(data.options.clone());
            state.startup.app_box = iced::widget::combo_box::State::new(data.apps.clone());
            state.system = data;
            // apps are named by what is installed
            state.rows.by_app_from = None;
        }
        Message::Animations(message) => return crate::pages::animations::update(state, message),
        Message::Rules(message) => return crate::pages::rules::update(state, message),
        Message::OutputsLoaded(result) => match result {
            Ok(outputs) => {
                state.displays.monitors = outputs::rects(&outputs);
                state.outputs = outputs;
                state.displays.error = None;
            }
            Err(failure) => state.displays.error = Some(failure),
        },
        Message::Displays(message) => return crate::pages::displays::update(state, message),
        Message::TrialTick => state.session.trial_tick(),
        Message::KeepTrial => state.session.keep_trial(),
        Message::EndTrial => state.session.end_trial(),
        Message::RunningTick => {
            return state.ask_niri(niri::Command::Running);
        }
        Message::RunningLoaded(result) => match result {
            Ok(data) => {
                state.running = Some(data);
                state.running_error = None;
            }
            Err(failure) => state.running_error = Some(failure),
        },
        Message::LiveEvent(kind, text) => {
            if state.live_events.len() >= 120 {
                state.live_events.pop_back();
            }
            state.live_events.push_front((kind, text));
        }
        Message::LiveError(e) => state.status = error(format!("Event stream: {e}")),
        Message::ClearLive => state.live_events.clear(),
        Message::Files(message) => return crate::pages::files::update(state, message),
        // ---- chrome ----
        Message::ModeLoaded(mode) => state.mode = Some(mode),
        Message::ContrastLoaded(high) => state.high_contrast = high,
        Message::WindowResized(size) => {
            state.window_width = size.width;
            state.window_height = size.height;
        }
        Message::Shortcut(shortcut) => return on_shortcut(state, shortcut),
        Message::OpenSearch => {
            state.help_open = false;
            return iced::widget::operation::focus(view::SEARCH_INPUT);
        }
        Message::SearchQuery(query) => {
            state.header_search = (!query.is_empty()).then_some((query, 0));
        }
        Message::SearchSubmit => {
            let picked = state
                .header_search
                .as_ref()
                .and_then(|(query, selected)| view::Hits::of(state, query).nth(*selected));
            if let Some(hit) = picked {
                return update(state, hit.message);
            }
        }
        Message::SearchPickHere(message) => {
            state.header_search = None;
            return update(state, Message::Rules(message));
        }
        Message::SearchPick(entry) => {
            state.header_search = None;
            let task = update(state, Message::Page(entry.page));
            state.highlight = entry.setting;
            return task;
        }
        Message::ShowHelp(open) => {
            state.help_open = open;
            if open {
                state.header_search = None;
            }
        }
        Message::PopOut(page) => {
            // the config column is in its own window now
            if page == Page::Source {
                state.source_popped = Some((None, state.source_shown));
                state.source_shown = false;
            }
            let Ok(exe) = std::env::current_exe() else {
                state.status = error("Could not find niri-center's own executable.");
                return Task::none();
            };
            let command = vec![
                exe.display().to_string(),
                "--pane".to_string(),
                page.slug().to_string(),
            ];
            // niri starts it, so the pane runs outside this window's sandbox
            return state.ask_niri(niri::Command::Spawn(command));
        }
        Message::OpenDocs(page) => {
            if let Some(url) = page.docs() {
                return state.ask_niri(niri::Command::OpenLink(url.to_string()));
            }
        }
        Message::LinkOpened(result) => {
            if let Err(failure) = result {
                state.status = error(format!("Could not open the page: {failure}"));
            }
        }
        Message::PoppedOut(result) => {
            state.status = match result {
                Ok(()) => info("Opened in its own window."),
                Err(failure) => error(format!("niri could not open the window: {failure}")),
            };
        }
        // ---- niri's config reloads ----
        Message::NiriConfigLoaded(failed) => {
            // what is on disk is loaded, so the panel can point into it
            state.session.niri_reloaded(failed);
            if failed {
                state.broken_line_shown = false;
                // the open windows, to find the editor
                return state.ask_niri(niri::Command::Running);
            }
        }
        Message::DismissHealth => state.session.health_fixed(),
        Message::FixTypo {
            path,
            line,
            name,
            meant,
        } => {
            let Some(store) = &mut state.session.store else {
                return Task::none();
            };
            match store.rename_node(&path, line, &name, &meant) {
                // the fix is what lets niri read it again: saved as any edit
                Ok(()) => state.session.health_fixed(),
                Err(failure) => state.status = error(failure.to_string()),
            }
        }
        Message::FocusWindow(id) => {
            return state.ask_niri(niri::Command::Focus(id));
        }
        Message::WindowFocused(result) => {
            if let Err(failure) = result {
                state.status = error(format!("niri did not switch to the window: {failure}"));
            }
        }
        Message::ShowBrokenLine(shown) => state.broken_line_shown = shown,
        // ---- closing ----
        Message::CloseRequested => {
            // a display change nobody kept goes back, as it would have
            state.session.end_trial();
            // an edit still waiting for its save goes out now, unless saving
            // waits for the user
            if state.session.save_when == SaveWhen::AfterChange {
                state.session.save();
            }
            state.closing = Some(Closing::AfterSave);
            state.header_search = None;
            state.help_open = false;
        }
        Message::SaveAndClose => {
            state.session.save();
            state.closing = Some(Closing::AfterSave);
        }
        Message::CloseCancel => state.closing = None,
        // ---- saving and the history ----
        Message::SaveTick => state.session.tick(),
        Message::SaveNow => {
            state.session.save();
            // the header says how the save went
            state.review_open = false;
        }
        Message::Undo => state.session.undo_latest(),
        Message::History(message) => return crate::history::update(state, message),
        Message::ShowSource(shown) => state.source_shown = shown,
        Message::JumpToFile(index, path) => {
            state.folded_files.remove(&path);
            return jump_to_file(index);
        }
        Message::FoldFile(path, folded) => {
            if folded {
                state.folded_files.insert(path);
            } else {
                state.folded_files.remove(&path);
            }
        }
        Message::PageDefaults(page) => {
            if let Some(store) = &mut state.session.store {
                let set: Vec<settings::Setting> = settings::set_on(store, page).collect();
                for setting in set {
                    if let Err(failure) = settings::edit(
                        store,
                        &mut state.session.drafts,
                        setting,
                        settings::Edit::Unset,
                    ) {
                        state.status = error(failure.to_string());
                        break;
                    }
                }
            }
        }
        Message::ToggleSidebar => state.sidebar_folded = Some(!state.sidebar_folded()),
        Message::PutBack | Message::CloseDiscard => return iced::exit(),
        Message::CopyPending => {
            if state.store().is_some() {
                let patch = state.session.pending_patch();
                state.status = info("Copied what is not saved as a diff.");
                return iced::clipboard::write(patch);
            }
        }
        Message::ShowReview(open) => state.review_open = open,
        Message::ToastTick => {
            let expired = state.status.as_ref().is_some_and(|status| {
                !status.error && status.since.elapsed().as_secs() >= TOAST_SECONDS
            });
            if expired {
                state.status = None;
            }
        }
        Message::DismissToast => state.status = None,
        Message::SourceWindow(id) => {
            if let Some((window, _)) = &mut state.source_popped {
                *window = Some(id);
            }
        }
        Message::NiriKeyboardLayout(index) => {
            // the picture follows a switch made in niri
            state.keybinds.niri_layout = Some(index);
            state.keybinds.layout = None;
        }
        Message::WindowClosed(id) => {
            if let Some((Some(window), was_shown)) = state.source_popped
                && window == id
            {
                state.source_popped = None;
                // unless the column was opened here meanwhile
                if !state.source_shown {
                    state.source_shown = was_shown;
                }
            }
        }
    }
    Task::none()
}

/// Keep an open picker on the color its field now holds, when the field
/// (or the gradient end it edits) was typed into or given a swatch.
fn follow_picker(state: &mut App, key: &settings::DraftKey, edit: &settings::Edit) {
    let (Some(open), settings::Edit::Draft(text)) = (&mut state.color, edit) else {
        return;
    };
    if open.field.key != *key {
        return;
    }
    let typed = match open.field.end {
        None => Some(text.clone()),
        Some(end) => {
            niri_center::color::Gradient::parse(text).map(|gradient| gradient.end(end).clone())
        }
    };
    if let Some(picked) = typed.as_deref().and_then(color::parse)
        && color::hex(picked) != open.hsva.hex()
    {
        open.hsva = color::hsva(picked, open.hsva);
    }
}

/// Route a keyboard shortcut; navigation ones are inert in single-pane mode.
fn on_shortcut(state: &mut App, shortcut: shortcuts::Shortcut) -> Task<Message> {
    use shortcuts::Shortcut;
    let navigable = state.pane.is_none() && state.closing.is_none();
    let pages: Vec<Page> = Page::in_sidebar().collect();
    match shortcut {
        Shortcut::Search if navigable => return update(state, Message::OpenSearch),
        Shortcut::Goto(i) if navigable => {
            state.header_search = None;
            if let Some(page) = pages.get(i) {
                return update(state, Message::Page(*page));
            }
        }
        Shortcut::Previous | Shortcut::Next if navigable => {
            let current = pages
                .iter()
                .position(|page| *page == state.page)
                .unwrap_or(0);
            let count = pages.len();
            let next = if matches!(shortcut, Shortcut::Next) {
                (current + 1) % count
            } else {
                (current + count - 1) % count
            };
            if let Some(&page) = pages.get(next) {
                return update(state, Message::Page(page));
            }
        }
        Shortcut::ToggleSource if navigable => {
            state.source_shown = !state.source_shown;
        }
        Shortcut::Help => return update(state, Message::ShowHelp(!state.help_open)),
        Shortcut::ToggleSidebar if navigable => return update(state, Message::ToggleSidebar),
        Shortcut::Type(typed) if navigable && !state.keybinds.finding_keys && !state.help_open => {
            let query = state
                .header_search
                .take()
                .map_or_else(|| typed.clone(), |(query, _)| query + &typed);
            state.header_search = Some((query, 0));
            return iced::widget::operation::focus(view::SEARCH_INPUT).chain(
                iced::widget::operation::move_cursor_to_end(view::SEARCH_INPUT),
            );
        }
        Shortcut::PopOutSource
            if navigable
                && state.store().is_some()
                && state.page.has_source()
                && cfg!(not(target_arch = "wasm32")) =>
        {
            return update(state, Message::PopOut(Page::Source));
        }
        Shortcut::PutBack if state.pane == Some(Page::Source) => return iced::exit(),
        Shortcut::Save => return update(state, Message::SaveNow),
        Shortcut::Undo if state.header_search.is_none() => return update(state, Message::Undo),
        Shortcut::Close | Shortcut::Confirm
            if state.session.trial.is_some() && state.header_search.is_none() =>
        {
            return update(state, Message::EndTrial);
        }
        Shortcut::Close if state.rules.dialog || state.rules.editor.is_some() => {
            return update(state, Message::Rules(pages::rules::Message::CancelEdit));
        }
        // Escape on Window rules goes back up a level once nothing floats
        // over the page
        Shortcut::Close | Shortcut::Back
            if state.page == Page::Rules && !state.floating() && pages::rules::can_go_up(state) =>
        {
            return update(state, Message::Rules(pages::rules::Message::Up));
        }
        Shortcut::Up | Shortcut::Down
            if state.page == Page::Rules && state.rules_beside_list() && !state.floating() =>
        {
            let down = matches!(shortcut, Shortcut::Down);
            return update(state, Message::Rules(pages::rules::Message::Step(down)));
        }
        Shortcut::Close => {
            // Esc closes the frontmost thing: search or a card, else an open
            // change on a narrow History page
            if !state.floating()
                && state.page == Page::History
                && state.history_view.selected.is_some()
            {
                state.history_view.selected = None;
            }
            state.header_search = None;
            state.help_open = false;
            state.closing = None;
            state.keybinds.finding_keys = false;
            state.review_open = false;
        }
        Shortcut::Up | Shortcut::Down => {
            let count = state
                .header_search
                .as_ref()
                .map_or(0, |(query, _)| view::Hits::of(state, query).len());
            if let Some((_, selected)) = &mut state.header_search
                && count > 0
            {
                *selected = if matches!(shortcut, Shortcut::Down) {
                    (*selected + 1) % count
                } else {
                    (*selected + count - 1) % count
                };
            }
        }
        Shortcut::Back
        | Shortcut::Search
        | Shortcut::Goto(_)
        | Shortcut::Previous
        | Shortcut::Next
        | Shortcut::ToggleSource
        | Shortcut::ToggleSidebar
        | Shortcut::PopOutSource
        | Shortcut::PutBack
        | Shortcut::Type(_)
        | Shortcut::Confirm
        | Shortcut::Undo => {}
    }
    Task::none()
}

/// Scroll the Config file page's list so file `index` starts at its top:
/// where its section is laid out, less where the list's content starts.
fn jump_to_file(index: usize) -> Task<Message> {
    use iced::widget::scrollable::AbsoluteOffset;
    use iced::widget::selector::{self, Target};
    selector::find(selector::id(crate::source::section_id(index))).then(|section| {
        selector::find(selector::id(crate::source::FILES_SCROLL)).then(move |list| {
            match (&section, list) {
                (Some(section), Some(Target::Scrollable { content_bounds, .. })) => {
                    iced::widget::operation::scroll_to(
                        crate::source::FILES_SCROLL,
                        AbsoluteOffset {
                            x: None,
                            y: Some(section.bounds().y - content_bounds.y),
                        },
                    )
                }
                _ => Task::none(),
            }
        })
    })
}
