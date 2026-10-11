//! Document model: [`Node`] tree with source spans, plus the splice engine.

use std::fmt;

/// A half-open byte range `[start, end)` into the original source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// A parsed value. `Quoted` holds the *decoded* contents (escapes resolved),
/// `Bare` a bare identifier/keyword (`off`, `eDP-1`), `Number` the number text
/// exactly as written (`2.0`, `-4`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueKind {
    Quoted(String),
    Bare(String),
    Number(String),
}

impl ValueKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Quoted(s) | Self::Bare(s) | Self::Number(s) => s,
        }
    }
}

/// A parsed value: kind + span of the raw token (including quotes).
#[derive(Debug, Clone)]
pub struct Value {
    pub kind: ValueKind,
    pub span: Span,
}

impl Value {
    /// Exact raw bytes from the file, including quotes for strings.
    pub fn raw<'a>(&self, doc: &'a Document) -> &'a str {
        &doc.source[self.span.start..self.span.end]
    }
}

/// A property `name = value` inside a node head.
#[derive(Debug, Clone)]
pub struct Prop {
    pub name: String,
    pub value: Value,
}

/// A parsed KDL node. `children` are block children; sibling nodes are never
/// nested here — the tree (roots/children) owns them.
#[derive(Debug, Clone)]
pub struct Node {
    pub id: usize,
    pub name: String,
    /// From the start of the `name` token to the end of the last token of this
    /// node (including the closing `}` when a block is present).
    pub node_span: Span,
    pub props: Vec<Prop>,
    pub args: Vec<Value>,
    pub children: Vec<Self>,
    /// Span of the whole `{ ... }` block including braces, if any.
    pub block: Option<Span>,
    /// Preceded by a `/ -` (slash-dash) marker: niri skips such nodes at load,
    /// but we keep them parsed and losslessly editable so the UI can offer
    /// "disable / enable" instead of delete.
    pub disabled: bool,
}

impl Node {
    /// The node exactly as written, from its name to its last token.
    pub fn raw<'a>(&self, doc: &'a Document) -> &'a str {
        &doc.source[self.node_span.start..self.node_span.end]
    }
}

/// The nodes in `nodes` named `name` that niri actually reads: `/-`-disabled
/// nodes are skipped, and with them everything nested inside.
pub fn active_named<'nodes, 'name>(
    nodes: &'nodes [Node],
    name: &'name str,
) -> impl Iterator<Item = &'nodes Node> + use<'nodes, 'name> {
    nodes
        .iter()
        .filter(move |node| node.name == name && !node.disabled)
}

/// One step in a path used with [`Document::find`]:
/// `[Name("output"), Index(1)]` = the second node named `output`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSeg {
    Name(&'static str),
    /// Choose the `n`-th (0-based) occurrence among same-named siblings.
    Index(usize),
}

/// A deferred text splice: replace `source[start..end]` with `replacement`.
#[derive(Debug, Clone)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

/// A lossless KDL document: original source, node tree, pending edits.
#[derive(Debug, Clone)]
pub struct Document {
    pub source: String,
    pub roots: Vec<Node>,
    edits: Vec<Edit>,
    /// Where each line of `source` starts, found as it is parsed: rows and
    /// the source column ask for many line numbers.
    line_starts: Vec<usize>,
}

/// Errors from editing operations.
#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("no node at path {path:?}")]
    NodeNotFound { path: Vec<PathSeg> },
    #[error("node `{name}` has no property `{prop}`")]
    PropNotFound { name: String, prop: String },
    #[error("node `{name}` has no argument at index {index}")]
    ArgNotFound { name: String, index: usize },
    #[error("replacement text must not contain newlines; got {text:?}")]
    ReplacementHasNewline { text: String },
}

impl Document {
    pub(crate) fn new(source: String, roots: Vec<Node>) -> Self {
        Self {
            roots,
            edits: Vec::new(),
            line_starts: std::iter::once(0)
                .chain(source.match_indices('\n').map(|(at, _)| at + 1))
                .collect(),
            source,
        }
    }

    /// The 1-based line `offset` of the source is on.
    pub fn line_of(&self, offset: usize) -> usize {
        // the lines starting at or before it
        self.line_starts
            .partition_point(|&start| start <= offset.min(self.source.len()))
    }

    /// All nodes in document order (DFS).
    pub fn iter_all(&self) -> impl Iterator<Item = &Node> {
        fn walk<'a>(node: &'a Node, out: &mut Vec<&'a Node>) {
            out.push(node);
            for child in &node.children {
                walk(child, out);
            }
        }
        let mut out = Vec::new();
        for root in &self.roots {
            walk(root, &mut out);
        }
        out.into_iter()
    }

    /// All nodes (any depth) with the given name, in document order.
    pub fn find_all(&self, name: &str) -> Vec<&Node> {
        self.iter_all().filter(|n| n.name == name).collect()
    }

    /// Find a node by path. `Name` segments match child-node names; an
    /// `Index` following a `Name` picks the n-th sibling of that name.
    /// The path must be complete (leaf node), not partial.
    pub fn find(&self, path: &[PathSeg]) -> Option<&Node> {
        let mut siblings: Vec<&Node> = self.roots.iter().collect();
        let mut found = None;
        let mut i = 0;
        while i < path.len() {
            match &path[i] {
                PathSeg::Name(name) => {
                    let matching: Vec<&Node> = siblings
                        .iter()
                        .filter(|n| n.name == *name)
                        .copied()
                        .collect();
                    let index = match path.get(i + 1) {
                        Some(PathSeg::Index(n)) => {
                            i += 1; // consume the Index this iteration
                            *n
                        }
                        _ => 0,
                    };
                    let node = matching.get(index).copied()?;
                    found = Some(node);
                    siblings = node.children.iter().collect();
                    i += 1;
                }
                PathSeg::Index(_) => return None,
            }
        }
        found
    }

    /// Decoded text of a value.
    ///
    /// `Quoted` already carries the decoded content (escapes resolved for
    /// normal strings, verbatim for raw strings), so we clone it directly —
    /// never re-derive from the raw span.
    pub fn value_text(&self, value: &Value) -> String {
        match &value.kind {
            ValueKind::Quoted(s) => s.clone(),
            ValueKind::Bare(s) | ValueKind::Number(s) => s.clone(),
        }
    }

    /// Replace the value of property `prop` on the node at `path`.
    ///
    /// `replacement` is the raw KDL text to write (e.g. `"1.5"` or `1.5`).
    pub fn edit_prop(
        &mut self,
        path: &[PathSeg],
        prop: &str,
        replacement: &str,
    ) -> Result<(), EditError> {
        let node = self.find(path).ok_or_else(|| EditError::NodeNotFound {
            path: path.to_vec(),
        })?;
        let span = node
            .props
            .iter()
            .find(|p| p.name == prop)
            .ok_or_else(|| EditError::PropNotFound {
                name: node.name.clone(),
                prop: prop.to_string(),
            })?
            .value
            .span;
        self.push_edit(new_edit(span, replacement)?);
        Ok(())
    }

    /// Replace the argument at `index` of the node at `path`.
    pub fn edit_arg(
        &mut self,
        path: &[PathSeg],
        index: usize,
        replacement: &str,
    ) -> Result<(), EditError> {
        let node = self.find(path).ok_or_else(|| EditError::NodeNotFound {
            path: path.to_vec(),
        })?;
        let span = node
            .args
            .get(index)
            .ok_or_else(|| EditError::ArgNotFound {
                name: node.name.clone(),
                index,
            })?
            .span;
        self.push_edit(new_edit(span, replacement)?);
        Ok(())
    }

    fn push_edit(&mut self, edit: Edit) {
        self.edits.push(edit);
        // an insertion goes before a replacement that starts where it does
        self.edits.sort_by_key(|e| (e.start, e.end));
        // P0 edits are non-overlapping by construction (each spans one value).
    }

    /// Raw splice: replace `source[span.start..span.end]` with `replacement`.
    ///
    /// Unrestricted (may contain newlines) — for structural edits like adding
    /// whole nodes or appending blocks. Multi-line text is fine here because
    /// it lands verbatim, exactly as written.
    pub fn splice(&mut self, span: Span, replacement: &str) {
        self.push_edit(raw_edit(span, replacement));
    }

    /// The text with its pending edits applied; borrowed when there are
    /// none, as after every refresh.
    pub fn text(&self) -> std::borrow::Cow<'_, str> {
        if self.edits.is_empty() {
            std::borrow::Cow::Borrowed(&self.source)
        } else {
            std::borrow::Cow::Owned(apply_edits(&self.source, &self.edits))
        }
    }

    /// Number of pending (unapplied) edits.
    pub const fn pending_edits(&self) -> usize {
        self.edits.len()
    }
}

impl fmt::Display for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&apply_edits(&self.source, &self.edits))
    }
}

/// Apply splices to the source text: `source[edits[i].start..edits[i].end]`
/// replaced by `edits[i].replacement`.
fn apply_edits(source: &str, edits: &[Edit]) -> String {
    if edits.is_empty() {
        return source.to_string();
    }
    let mut out = String::with_capacity(source.len() + 64);
    let mut cursor = 0;
    for edit in edits {
        debug_assert!(edit.end <= source.len());
        out.push_str(&source[cursor..edit.start]);
        out.push_str(&edit.replacement);
        cursor = edit.end;
    }
    out.push_str(&source[cursor..]);
    out
}

fn new_edit(span: Span, replacement: &str) -> Result<Edit, EditError> {
    if replacement.contains('\n') {
        return Err(EditError::ReplacementHasNewline {
            text: replacement.to_string(),
        });
    }
    Ok(Edit {
        start: span.start,
        end: span.end,
        replacement: replacement.to_string(),
    })
}

/// Unrestricted splice into the source (used for structural edits: adding
/// whole nodes, appending blocks). Does not guard against newlines.
fn raw_edit(span: Span, replacement: &str) -> Edit {
    Edit {
        start: span.start,
        end: span.end,
        replacement: replacement.to_string(),
    }
}

/// `text` as a KDL string literal: in quotes, with `\` and `"` escaped.
pub fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}
