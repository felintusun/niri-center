//! Recursive-descent parser producing the lossless node tree.
//!
//! The parser follows the KDL bare-identifier disambiguation rule that real
//! configs rely on (and that the reference `kdl` crate implements):
//!
//! * an identifier followed by `=` is a **property** name;
//! * an identifier followed by end-of-line / `;` / `}` / EOF is a
//!   **bare-identifier argument** (e.g. `off`, `eDP-1`);
//! * an identifier followed by another value or `{` starts a **new sibling
//!   node** — this is why niri's one-node-per-line shape parses correctly.
//!
//! A node ends at a newline, a `;` (KDL's node terminator, as in
//! `{ tap; natural-scroll; }`), a `}` or EOF; the lookahead rule above is what
//! keeps `scale 2.0\nscale 3.0` from merging.

use super::doc::{Document, Node, Prop, Span, Value, ValueKind};
use std::fmt;

/// A KDL parse failure, with the byte offset into the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub offset: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            out,
            "KDL parse error at byte {}: {}",
            self.offset, self.message
        )
    }
}

impl std::error::Error for ParseError {}

/// Parse `source` into a lossless [`Document`].
pub fn parse(source: &str) -> Result<Document, ParseError> {
    let mut parser = Parser {
        src: source,
        pos: 0,
        next_id: 0,
    };
    let roots = parser.parse_node_list(true)?;
    Ok(Document::new(source.to_string(), roots))
}

struct Parser<'view> {
    src: &'view str,
    pos: usize,
    next_id: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.src.as_bytes().get(self.pos).copied()
    }

    fn err(&self, message: impl Into<String>) -> ParseError {
        ParseError {
            offset: self.pos,
            message: message.into(),
        }
    }

    /// Skip spaces/tabs/CR and comments. Stops *before* any newline, `;` or
    /// EOF without consuming it, and returns whether it stopped at a line break.
    fn skip_inline(&mut self) -> bool {
        loop {
            let Some(byte) = self.peek() else {
                return true; // EOF counts as end-of-line
            };
            match byte {
                b' ' | b'\t' | b'\r' => self.pos += 1,
                b'\n' => return true,
                b'/' if self.src.as_bytes().get(self.pos + 1) == Some(&b'/') => {
                    self.pos += 2;
                    while let Some(byte) = self.peek() {
                        if byte == b'\n' {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                b'/' if self.src.as_bytes().get(self.pos + 1) == Some(&b'*') => {
                    let Some(end) = self.src[self.pos + 2..].find("*/") else {
                        return true;
                    };
                    self.pos += 2 + end + 2;
                }
                _ => return false,
            }
        }
    }

    /// Consume a line break (and any following inline whitespace/comments of
    /// the next line). Used *between* nodes.
    fn consume_line_break(&mut self) {
        // at a '\n' (or EOF)
        if self.peek() == Some(b'\n') {
            self.pos += 1;
        }
    }

    fn parse_node_list(&mut self, top: bool) -> Result<Vec<Node>, ParseError> {
        let mut nodes = Vec::new();
        loop {
            self.skip_inline();
            match self.peek() {
                None => {
                    if !top {
                        return Err(self.err("unclosed block (expected `}`)"));
                    }
                    break;
                }
                Some(b'}') => {
                    if top {
                        return Err(self.err("unexpected `}` at top level"));
                    }
                    break;
                }
                Some(b'\n') => {
                    self.consume_line_break();
                }
                // terminator of the node just parsed (or a stray one)
                Some(b';') => self.pos += 1,
                _ => {
                    let disabled = self.consume_dash_dash();
                    nodes.push(self.parse_node(disabled)?);
                }
            }
        }
        Ok(nodes)
    }

    /// Consume a `/ -` (slash-dash) marker preceding the next node, if present.
    ///
    /// niri skips nodes marked this way — the default config disables examples
    /// with `/-window-rule { … }`. We only record the marker so the UI can
    /// offer disable/enable instead of delete.
    fn consume_dash_dash(&mut self) -> bool {
        if self.peek() == Some(b'/') && self.src.as_bytes().get(self.pos + 1) == Some(&b'-') {
            self.pos += 2;
            self.skip_inline();
            true
        } else {
            false
        }
    }

    fn parse_node(&mut self, disabled: bool) -> Result<Node, ParseError> {
        let id = self.next_id;
        self.next_id += 1;

        let name_start = self.pos;
        let name = self.read_ident()?;

        let mut props = Vec::new();
        let mut args = Vec::new();
        let mut children = Vec::new();
        let mut block = None;
        let mut last_end = self.pos;

        loop {
            self.skip_inline();
            match self.peek() {
                None | Some(b'\n' | b';' | b'}') => break,
                Some(b'{') => {
                    let block_start = self.pos;
                    self.pos += 1; // consume '{'
                    self.consume_line_break();
                    children = self.parse_node_list(false)?;
                    self.skip_inline();
                    if self.peek() != Some(b'}') {
                        return Err(self.err("unclosed block (expected `}`)"));
                    }
                    self.pos += 1; // consume '}'
                    block = Some(Span::new(block_start, self.pos));
                    last_end = self.pos;
                    break;
                }
                Some(b'"') => {
                    let value = self.parse_string()?;
                    last_end = value.span.end;
                    args.push(value);
                }
                Some(b'r') if self.starts_raw_string() => {
                    let value = self.parse_string()?;
                    last_end = value.span.end;
                    args.push(value);
                }
                Some(b'-') | Some(b'+') | Some(b'0'..=b'9') => {
                    let value = self.parse_number()?;
                    last_end = value.span.end;
                    args.push(value);
                }
                Some(byte) if is_ident_start(byte) => {
                    // Decide: property, bare-ident argument, or new sibling.
                    let token_start = self.pos;
                    let (_, ident_end) = read_ident_at(self.src, token_start)?;
                    let saved = self.pos;
                    self.pos = ident_end;
                    self.skip_inline();
                    let next = self.peek();
                    self.pos = saved; // restore, we may consume or defer
                    match next {
                        Some(b'=') => {
                            // property: name = value
                            let name = self.read_ident()?;
                            self.skip_inline();
                            if self.peek() != Some(b'=') {
                                return Err(self.err("expected `=` after property name"));
                            }
                            self.pos += 1;
                            self.skip_inline();
                            let value = self.parse_value()?;
                            last_end = value.span.end;
                            props.push(Prop { name, value });
                        }
                        Some(b'\n' | b';' | b'}') | None => {
                            // bare-identifier argument (end of node / block / file)
                            let text = self.read_ident()?;
                            args.push(Value {
                                kind: ValueKind::Bare(text),
                                span: Span::new(token_start, ident_end),
                            });
                            last_end = ident_end;
                        }
                        _ => {
                            // followed by a value/{/ident on the same line:
                            // this identifier starts a NEW sibling node.
                            // Leave the position untouched and stop this node.
                            break;
                        }
                    }
                }
                Some(byte) => {
                    return Err(self.err(format!(
                        "unexpected byte `{}` in node `{name}`",
                        char::from(byte)
                    )));
                }
            }
        }

        Ok(Node {
            id,
            name,
            node_span: Span::new(name_start, last_end),
            props,
            args,
            children,
            block,
            disabled,
        })
    }

    /// Parse a value used in property position (`name = <value>`).
    fn parse_value(&mut self) -> Result<Value, ParseError> {
        self.skip_inline();
        match self.peek() {
            Some(b'"') => self.parse_string(),
            Some(b'r') if self.starts_raw_string() => self.parse_string(),
            Some(b'-') | Some(b'+') | Some(b'0'..=b'9') => self.parse_number(),
            // Bare identifiers are unambiguous in *property-value* position:
            // everything up to the next token IS the value (`repeat=false`,
            // `hotkey-overlay-title=null`, `allow-when-locked=true`), even when
            // a `{` block or another property follows on the same line.
            Some(byte) if is_ident_start(byte) => {
                let start = self.pos;
                let text = self.read_ident()?;
                let end = self.pos;
                Ok(Value {
                    kind: ValueKind::Bare(text),
                    span: Span::new(start, end),
                })
            }
            _ => Err(self.err("expected a value after `=`")),
        }
    }

    fn parse_string(&mut self) -> Result<Value, ParseError> {
        if self.peek() == Some(b'"') {
            self.parse_normal_string()
        } else if self.starts_raw_string() {
            self.parse_raw_string()
        } else {
            Err(self.err("expected a string"))
        }
    }

    /// `r` followed by `"` or `#"` starts a raw string (`r"..."`, `r#"..."#`).
    fn starts_raw_string(&self) -> bool {
        self.peek() == Some(b'r')
            && matches!(
                self.src.as_bytes().get(self.pos + 1),
                Some(b'"') | Some(b'#')
            )
    }

    /// KDL raw strings: `r"..."` or `r#"..."#` with any number of `#`.
    /// Content is taken verbatim — no escapes.
    fn parse_raw_string(&mut self) -> Result<Value, ParseError> {
        let start = self.pos;
        self.pos += 1; // 'r'
        let mut hashes = 0usize;
        while self.peek() == Some(b'#') {
            hashes += 1;
            self.pos += 1;
        }
        if self.peek() != Some(b'"') {
            return Err(self.err("invalid raw string: expected `\"` after `r#…`"));
        }
        self.pos += 1; // opening quote
        let content_start = self.pos;
        let bytes = self.src.as_bytes();
        let mut at = self.pos;
        loop {
            let Some(&byte) = bytes.get(at) else {
                return Err(self.err("unterminated raw string"));
            };
            // a quote closes it when as many hashes follow as opened it
            if byte == b'"'
                && bytes
                    .get(at + 1..at + 1 + hashes)
                    .is_some_and(|run| run.iter().all(|&byte| byte == b'#'))
            {
                let content = self.src[content_start..at].to_string();
                self.pos = at + 1 + hashes;
                return Ok(Value {
                    kind: ValueKind::Quoted(content),
                    span: Span::new(start, self.pos),
                });
            }
            at += 1;
        }
    }

    fn parse_normal_string(&mut self) -> Result<Value, ParseError> {
        let start = self.pos;
        debug_assert_eq!(self.peek(), Some(b'"'));
        self.pos += 1;
        let mut decoded = String::new();
        loop {
            match self.peek() {
                None => return Err(self.err("unterminated string")),
                Some(b'"') => {
                    self.pos += 1;
                    break;
                }
                Some(b'\\') => {
                    self.pos += 1;
                    match self.peek() {
                        Some(b'"') => {
                            decoded.push('"');
                            self.pos += 1;
                        }
                        Some(b'\\') => {
                            decoded.push('\\');
                            self.pos += 1;
                        }
                        Some(b'n') => {
                            decoded.push('\n');
                            self.pos += 1;
                        }
                        Some(b't') => {
                            decoded.push('\t');
                            self.pos += 1;
                        }
                        Some(other) => {
                            decoded.push('\\');
                            decoded.push(char::from(other));
                            self.pos += 1;
                        }
                        None => return Err(self.err("unterminated string escape")),
                    }
                }
                Some(_) => {
                    let Some(ch) = self.src[self.pos..].chars().next() else {
                        return Err(self.err("unterminated string"));
                    };
                    decoded.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
        Ok(Value {
            kind: ValueKind::Quoted(decoded),
            span: Span::new(start, self.pos),
        })
    }

    fn parse_number(&mut self) -> Result<Value, ParseError> {
        let start = self.pos;
        // optional sign — only if followed by a digit
        match self.peek() {
            Some(b'-') | Some(b'+')
                if self
                    .src
                    .as_bytes()
                    .get(self.pos + 1)
                    .is_some_and(u8::is_ascii_digit) =>
            {
                self.pos += 1;
            }
            _ => {}
        }
        let digits_start = self.pos;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.pos == digits_start {
            return Err(self.err("invalid number"));
        }
        // fraction
        if self.peek() == Some(b'.') {
            self.pos += 1;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        // exponent
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            let mut exp = self.pos + 1;
            if matches!(self.src.as_bytes().get(exp), Some(b'+') | Some(b'-')) {
                exp += 1;
            }
            if self.src.as_bytes().get(exp).is_some_and(u8::is_ascii_digit) {
                self.pos = exp;
                while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                    self.pos += 1;
                }
            }
        }
        Ok(Value {
            kind: ValueKind::Number(self.src[start..self.pos].to_string()),
            span: Span::new(start, self.pos),
        })
    }

    fn read_ident(&mut self) -> Result<String, ParseError> {
        let (text, end) = read_ident_at(self.src, self.pos)?;
        self.pos = end;
        Ok(text)
    }
}

const fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

/// Scan an identifier at `pos`; returns its text and end offset.
fn read_ident_at(src: &str, pos: usize) -> Result<(String, usize), ParseError> {
    let bytes = src.as_bytes();
    let Some(&first) = bytes.get(pos) else {
        return Err(ParseError {
            offset: pos,
            message: "expected an identifier".into(),
        });
    };
    if !is_ident_start(first) {
        return Err(ParseError {
            offset: pos,
            message: format!("expected an identifier, found `{}`", char::from(first)),
        });
    }
    let mut end = pos + 1;
    while let Some(&byte) = bytes.get(end) {
        // `+` is not KDL-standard, but niri keybind names are bare
        // identifiers like `Mod+Space` / `Super+Shift+Enter`.
        if byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' || byte == b'+' {
            end += 1;
        } else {
            break;
        }
    }
    Ok((src[pos..end].to_string(), end))
}
