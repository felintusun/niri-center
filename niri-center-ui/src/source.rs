//! The config source beside a page: the part of the file the page edits,
//! colored, with the latest change marked.
//!
//! Read-only. The app changes the file through its controls, and you
//! through your editor; this column shows what either did, in the file's
//! own words.

use iced::widget::{button, column, container, row, scrollable, space, text, tooltip};
use iced::{Alignment, Length};
use niri_center::diff::{self, Line};
use niri_center::store::Store;

use crate::highlight::Mark;
use crate::icons::Icon;
use crate::{App, El, Message, Page, highlight, theme};

/// Lines (1-based) of `path` that the latest change wrote, in order.
fn changed_lines(state: &App, path: &std::path::Path, now: &str) -> Vec<usize> {
    let Some(before) = state
        .session
        .saved
        .as_ref()
        .and_then(|saved| saved.text_of(path))
    else {
        return Vec::new();
    };
    diff::diff(before, now)
        .iter()
        .flat_map(|hunk| {
            hunk.numbered()
                .filter(|(_, _, line)| matches!(line, Line::Added(_)))
                .filter_map(|(_, new, _)| new)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The source column, or its fold strip.
pub fn view(state: &App) -> El<'_> {
    let Some(store) = state.store() else {
        return space().into();
    };
    let Some(excerpt) = state.page.excerpt(store) else {
        return column![
            text("Nothing on this page is in your config yet. niri uses its defaults.")
                .size(13)
                .style(theme::muted)
        ]
        .padding([16, 20])
        .into();
    };
    let source = store.text_of(excerpt.path).unwrap_or_default();
    let changed = changed_lines(state, excerpt.path, source);
    let colors = state.colors();
    let mut lines =
        highlight::numbered_lines(source, excerpt.first, excerpt.last, &changed, colors);
    // on Rules, the rule the open form would add, where it would go
    let draft =
        (state.page == Page::Rules && state.rules.dialog && state.rules.draft.editing.is_none())
            .then(|| state.rules.draft.rule())
            .filter(|rule| rule.app_id.is_some() || rule.title.is_some() || !rule.flags.is_empty());
    let total = source.lines().count();
    if let Some(rule) = &draft {
        let kdl = rule.to_kdl();
        for (offset, line) in std::iter::once("").chain(kdl.lines()).enumerate() {
            lines = lines.push(highlight::numbered(
                total + 1 + offset,
                line,
                Mark::Added,
                highlight::Gutter::Number,
                colors,
            ));
        }
    }
    let name = text(store.file_label(excerpt.path))
        .size(14)
        .font(theme::MONO)
        .width(Length::Fill);
    let pop_out = tooltip(
        button(text("Open as a window").size(12))
            .padding([4, 10])
            .style(theme::secondary)
            .on_press(Message::PopOut(Page::Source)),
        crate::controls::tip("Ctrl+Shift+N"),
        tooltip::Position::Bottom,
    );
    let fold = crate::controls::icon_button(
        Icon::Fold,
        14.0,
        "Hide the config, Ctrl+.".to_string(),
        Message::ShowSource(false),
    );
    // one line when the column has room, the controls under the name when not
    let controls: El<'_> = if state.source_width() >= 520.0 {
        row![name, pop_out, fold]
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
    } else {
        column![
            row![name, fold].spacing(8).align_y(Alignment::Center),
            row![pop_out].spacing(8).align_y(Alignment::Center),
        ]
        .spacing(6)
        .into()
    };
    let header = column![
        controls,
        text(if draft.is_some() {
            format!("The new rule goes after line {total}")
        } else if excerpt.first == excerpt.last {
            format!("Line {}", excerpt.first)
        } else {
            format!("Lines {}–{}", excerpt.first, excerpt.last)
        })
        .size(12)
        .style(theme::muted),
    ]
    .spacing(6)
    .padding(iced::Padding {
        top: 12.0,
        right: 12.0,
        bottom: 12.0,
        left: 20.0,
    });
    let lines = container(lines).width(Length::Fill).clip(true);
    column![
        header,
        // long lines run past the edge rather than wrap: wrapped KDL no
        // longer lines up with its line numbers
        scrollable(lines)
            .style(theme::scrollbar)
            .direction(scrollable::Direction::Vertical(theme::thin_scrollbar(),))
            .width(Length::Fill)
            .height(Length::Fill),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// The scrolling list of files on the Config file page, to jump in.
pub const FILES_SCROLL: &str = "config-files";

/// A file's section on the Config file page, by its place in the list.
pub fn section_id(index: usize) -> String {
    format!("config-file-{index}")
}

/// The whole config, on its page or popped out into a window of its own:
/// config.kdl, then each file it includes, in the order niri reads them,
/// each under a heading that folds it. The file names stay above the code
/// and jump to their file. It follows every save, the latest change marked.
pub fn whole<'app>(state: &'app App, store: &'app Store) -> El<'app> {
    let colors = state.colors();
    let paths: Vec<&std::path::Path> = std::iter::once(store.path())
        .chain(
            store
                .included_files()
                .iter()
                .map(|file| file.path.as_path()),
        )
        .collect();
    let mut jump = row![].spacing(6).align_y(Alignment::Center);
    for (index, path) in paths.iter().enumerate() {
        jump = jump.push(
            button(text(store.file_label(path)).size(12).font(theme::MONO))
                .padding([3, 8])
                .style(theme::secondary)
                .on_press(Message::JumpToFile(index, path.to_path_buf())),
        );
    }
    let mut top = column![].spacing(8);
    // only the window of its own goes back; in the app it is a page
    if state.pane == Some(Page::Source) {
        top = top.push(
            row![
                space().width(Length::Fill),
                tooltip(
                    button(text("Put back").size(12))
                        .padding([4, 10])
                        .style(theme::secondary)
                        .on_press(Message::PutBack),
                    crate::controls::tip("Ctrl+W"),
                    tooltip::Position::Left,
                ),
            ]
            .align_y(Alignment::Center),
        );
    }
    // a lone config.kdl has nowhere to jump
    if paths.len() > 1 {
        top = top.push(jump.wrap());
    }
    // includes niri cannot follow (a missing file, a loop) say so first
    for warning in store.include_warnings() {
        top = top.push(text(warning.clone()).size(12).style(theme::pending));
    }
    let mut sections = column![].spacing(20).width(Length::Fill);
    for (index, path) in paths.into_iter().enumerate() {
        let source = store.text_of(path).unwrap_or_default();
        let total = source.lines().count();
        let folded = state.folded_files.contains(path);
        let heading = button(
            row![
                crate::icons::icon(
                    if folded {
                        Icon::Chevron
                    } else {
                        Icon::ChevronDown
                    },
                    12.0
                ),
                text(store.file_label(path)).size(14).font(theme::MONO),
                text(niri_center::words::count(total, "line", "lines"))
                    .size(12)
                    .style(theme::muted),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([4, 6])
        .style(theme::list_row)
        .on_press(Message::FoldFile(path.to_path_buf(), !folded));
        let section: El<'_> = if folded {
            heading.into()
        } else {
            let changed = changed_lines(state, path, source);
            let lines = highlight::numbered_lines(source, 1, total, &changed, colors);
            column![heading, container(lines).width(Length::Fill).clip(true)]
                .spacing(6)
                .into()
        };
        sections = sections.push(container(section).id(section_id(index)));
    }
    column![
        top,
        scrollable(container(sections).padding(iced::Padding {
            right: 10.0,
            ..iced::Padding::ZERO
        }))
        .id(FILES_SCROLL)
        .style(theme::scrollbar)
        .height(Length::Fill),
    ]
    .spacing(12)
    .height(Length::Fill)
    .into()
}
