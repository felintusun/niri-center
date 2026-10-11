//! The config source beside a page: the part of the file the page edits,
//! colored, with the latest change marked.
//!
//! Read-only. The app changes the file through its controls, and you
//! through your editor; this column shows what either did, in the file's
//! own words.

use iced::widget::{button, column, container, row, scrollable, space, text, tooltip};
use iced::{Alignment, Length};
use niri_center::diff::{self, Line};

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
    let Some(store) = &state.session.store else {
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
                None,
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
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new().width(6).scroller_width(6),
            ))
            .width(Length::Fill)
            .height(Length::Fill),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// The config popped out into a window of its own: all of it, following
/// every save, the latest change marked.
pub fn whole(state: &App) -> El<'_> {
    let Some(store) = &state.session.store else {
        return space().into();
    };
    let path = store.path();
    let source = store.text_of(path).unwrap_or_default();
    let changed = changed_lines(state, path, source);
    let colors = state.colors();
    let total = source.lines().count();
    let lines = highlight::numbered_lines(source, 1, total, &changed, colors);
    let mut header = row![
        text(store.file_label(path))
            .size(14)
            .font(theme::MONO)
            .width(Length::Fill),
    ]
    .align_y(Alignment::Center);
    // only the window of its own goes back; in the app it is a page
    if state.pane == Some(Page::Source) {
        header = header.push(tooltip(
            button(text("Put back").size(12))
                .padding([4, 10])
                .style(theme::secondary)
                .on_press(Message::PutBack),
            crate::controls::tip("Ctrl+W"),
            tooltip::Position::Left,
        ));
    }
    column![header, container(lines).width(Length::Fill).clip(true)]
        .spacing(10)
        .into()
}
