//! KDL coloring for the source column and the history's diffs.
//!
//! Display only: a small scanner over one line at a time that never reads
//! the config for meaning (the parser in `kdl` does that). A line that the
//! scanner gets wrong still shows every character; it is only colored less
//! well.

use std::ops::Range;

/// What a stretch of a line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    Plain,
    Node,
    Property,
    String,
    /// Numbers and the keywords `true`, `false` and `null`.
    Number,
    Punctuation,
    Comment,
}

/// Characters that end a bare word.
const fn ends_word(c: char) -> bool {
    c.is_whitespace() || matches!(c, '{' | '}' | '(' | ')' | ';' | '=' | '"')
}

/// The stretches of `line`, in order and covering all of it.
pub fn tokens(line: &str) -> Vec<(Range<usize>, Token)> {
    let mut out = Vec::new();
    // a node name is the first word of a line, or the first after { or ;
    let mut head = true;
    let mut rest = line.char_indices().peekable();
    while let Some((start, c)) = rest.next() {
        let after = &line[start..];
        let (end, token) = if c.is_whitespace() {
            let end = after
                .find(|c: char| !c.is_whitespace())
                .map_or(line.len(), |n| start + n);
            (end, Token::Plain)
        } else if after.starts_with("//") {
            (line.len(), Token::Comment)
        } else if after.starts_with("/-") {
            (start + 2, Token::Comment)
        } else if let Some(rest) = after.strip_prefix("/*") {
            let end = rest.find("*/").map_or(line.len(), |n| start + n + 4);
            (end, Token::Comment)
        } else if c == '"' {
            head = false;
            (start + quoted_length(after), Token::String)
        } else if let Some(length) = raw_string_length(after) {
            head = false;
            (start + length, Token::String)
        } else if matches!(c, '{' | '}' | ';') {
            head = true;
            (start + 1, Token::Punctuation)
        } else if matches!(c, '=' | '(' | ')') {
            (start + 1, Token::Punctuation)
        } else {
            let end = after.find(ends_word).map_or(line.len(), |n| start + n);
            let word = &line[start..end];
            let token = if line[end..].starts_with('=') {
                Token::Property
            } else if head {
                Token::Node
            } else if is_number(word)
                || matches!(word, "true" | "false" | "null")
                || matches!(word, "#true" | "#false" | "#null")
            {
                Token::Number
            } else {
                Token::Plain
            };
            head = false;
            (end.max(start + c.len_utf8()), token)
        };
        out.push((start..end, token));
        while rest.peek().is_some_and(|(next, _)| *next < end) {
            rest.next();
        }
    }
    out
}

/// Length of a quoted string at the start of `text`, quotes included; an
/// unclosed one runs to the end of the line.
fn quoted_length(text: &str) -> usize {
    let mut escaped = false;
    for (index, c) in text.char_indices().skip(1) {
        match c {
            '\\' if !escaped => escaped = true,
            '"' if !escaped => return index + 1,
            _ => escaped = false,
        }
    }
    text.len()
}

/// Length of a raw string such as `r#"…"#` at the start of `text`.
fn raw_string_length(text: &str) -> Option<usize> {
    let body = text.strip_prefix('r').or_else(|| text.strip_prefix('#'))?;
    let hashes = body.chars().take_while(|c| *c == '#').count();
    let leading = text.len() - body.len() + hashes;
    if !text[leading..].starts_with('"') {
        return None;
    }
    let closing = format!(
        "\"{}",
        "#".repeat(hashes + usize::from(text.starts_with('#')))
    );
    let opened = leading + 1;
    Some(
        text[opened..]
            .find(&closing)
            .map_or(text.len(), |n| opened + n + closing.len()),
    )
}

fn is_number(word: &str) -> bool {
    let digits = word.strip_prefix(['-', '+']).unwrap_or(word);
    digits.starts_with(|c: char| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(&str, Token)> {
        tokens(line)
            .into_iter()
            .map(|(range, token)| (&line[range], token))
            .filter(|(text, _)| !text.trim().is_empty())
            .collect()
    }

    #[test]
    fn a_line_is_covered_exactly() {
        for line in [
            "    Mod+T hotkey-overlay-title=\"Open terminal\" { spawn \"foot\"; }",
            "    match app-id=r#\"^niri-center\\.\"#",
            "/-window-rule { open-floating true; }",
            "gaps 16 // room \"between\" windows",
            "    proportion 0.33333",
            "unclosed \"string",
            "    ünïcode-node 3 { }",
        ] {
            let covered: String = tokens(line)
                .into_iter()
                .map(|(range, _)| &line[range])
                .collect();
            assert_eq!(covered, line);
        }
    }

    #[test]
    fn names_properties_strings_and_numbers() {
        assert_eq!(
            kinds("    Mod+T hotkey-overlay-title=\"Open terminal\" { spawn \"foot\"; }"),
            vec![
                ("Mod+T", Token::Node),
                ("hotkey-overlay-title", Token::Property),
                ("=", Token::Punctuation),
                ("\"Open terminal\"", Token::String),
                ("{", Token::Punctuation),
                ("spawn", Token::Node),
                ("\"foot\"", Token::String),
                (";", Token::Punctuation),
                ("}", Token::Punctuation),
            ]
        );
        assert_eq!(
            kinds("gaps 16 // room"),
            vec![
                ("gaps", Token::Node),
                ("16", Token::Number),
                ("// room", Token::Comment),
            ]
        );
        assert_eq!(
            kinds("    match app-id=r#\"^a\"# at-startup=true"),
            vec![
                ("match", Token::Node),
                ("app-id", Token::Property),
                ("=", Token::Punctuation),
                ("r#\"^a\"#", Token::String),
                ("at-startup", Token::Property),
                ("=", Token::Punctuation),
                ("true", Token::Number),
            ]
        );
    }
}
