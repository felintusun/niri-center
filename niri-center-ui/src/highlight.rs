//! KDL lines colored by `niri_center::highlight`'s scanner, for the source
//! column and the history's diffs.

use iced::widget::text::Span;
use iced::widget::{Column, column, container, rich_text, row, space, span, text};
use iced::{Alignment, Color, Length};

use niri_center::highlight::{Token, tokens};

use crate::message::Message;
use crate::{El, theme};

const fn color(colors: &theme::Colors, token: Token) -> Color {
    match token {
        Token::Plain => colors.text,
        Token::Node => colors.syntax_node,
        Token::Property => colors.syntax_property,
        Token::String => colors.syntax_string,
        Token::Number => colors.syntax_number,
        Token::Punctuation | Token::Comment => colors.muted,
    }
}

/// Lines of KDL, colored, in a code block.
pub fn block<'a>(source: &str, colors: &theme::Colors, size: f32) -> El<'a> {
    let mut lines = iced::widget::column![];
    for text in source.lines() {
        lines = lines.push(line(text, colors, size));
    }
    iced::widget::container(lines)
        .padding(8)
        .width(iced::Length::Fill)
        .clip(true)
        .style(theme::code_block)
        .into()
}

/// One line of KDL, colored, in the monospace face.
pub fn line<'a>(source: &str, colors: &theme::Colors, size: f32) -> El<'a> {
    let spans: Vec<Span<'a, (), iced::Font>> = tokens(source)
        .into_iter()
        .map(|(range, token)| {
            let piece = span(source[range].to_string()).color(color(colors, token));
            if token == Token::Comment {
                piece.font(iced::Font {
                    style: iced::font::Style::Italic,
                    ..theme::MONO
                })
            } else {
                piece
            }
        })
        .collect();
    rich_text(spans)
        .font(theme::MONO)
        .size(size)
        .wrapping(iced::widget::text::Wrapping::None)
        .into()
}

/// How a numbered line stands out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Plain,
    /// Changed and not saved yet: amber.
    Changed,
    Added,
    Removed,
}

/// Lines `first` to `last` (1-based) of `source`, numbered and colored,
/// with the lines in `changed` marked as not saved yet.
pub fn numbered_lines<'a>(
    source: &str,
    first: usize,
    last: usize,
    changed: &[usize],
    colors: &theme::Colors,
) -> Column<'a, Message> {
    let mut lines = column![].width(Length::Fill);
    for (index, line) in source
        .lines()
        .enumerate()
        .skip(first.saturating_sub(1))
        .take((last + 1).saturating_sub(first))
    {
        let number = index + 1;
        let mark = if changed.binary_search(&number).is_ok() {
            Mark::Changed
        } else {
            Mark::Plain
        };
        lines = lines.push(numbered(number, line, mark, None, colors));
    }
    lines
}

/// Numbered lines on a surface of their own. Long lines run past the
/// edge: wrapped, KDL would lose its line numbers.
pub fn lines_block<'a>(lines: Column<'a, Message>) -> El<'a> {
    container(lines)
        .padding([8, 0])
        .clip(true)
        .width(Length::Fill)
        .style(theme::group)
        .into()
}

/// One line of KDL behind its number, the way the source column, the
/// repair panel and History show code. `sign` is a diff's `+` or `-`.
pub fn numbered<'a>(
    number: usize,
    source: &str,
    mark: Mark,
    sign: Option<&'static str>,
    colors: &theme::Colors,
) -> El<'a> {
    let number_style = match mark {
        Mark::Changed => theme::pending,
        Mark::Added if sign.is_none() => theme::success,
        Mark::Plain | Mark::Added | Mark::Removed => theme::muted,
    };
    let mut shown = row![
        text(number.to_string())
            .size(13)
            .font(theme::MONO)
            .style(number_style)
            .width(44)
            .align_x(iced::alignment::Horizontal::Right),
        space().width(10),
    ];
    if let Some(sign) = sign {
        let tint = match mark {
            Mark::Added => colors.success,
            Mark::Removed => colors.error,
            Mark::Plain | Mark::Changed => colors.muted,
        };
        shown = shown.push(text(sign).size(13).font(theme::MONO).width(16).color(tint));
    }
    shown = shown
        .push(line(source, colors, 13.0))
        .push(space().width(12));
    container(shown.height(20).align_y(Alignment::Center))
        .width(Length::Fill)
        .style(move |theme| match mark {
            Mark::Plain => container::Style::default(),
            Mark::Changed => theme::changed_row(theme),
            Mark::Added => theme::added_row(theme),
            Mark::Removed => theme::removed_row(theme),
        })
        .into()
}
