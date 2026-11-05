//! Settings the way niri reads them.
//!
//! niri merges most sections field by field, and includes are positional: a
//! value set later in reading order overrides an earlier one, wherever the
//! include sits in the including file (niri wiki, Configuration: Include —
//! "Includes are positional. They will override options set prior to them.").
//! So the value that applies is the *last* occurrence in reading order, and
//! an edit must land on that occurrence, or niri will keep the old value.
//!
//! Exceptions the wiki names: multipart sections (`output`, `window-rule`,
//! `workspace`) are inserted as is — they are selected here by their first
//! argument — and sections whose contents form one structure (animation
//! subsections, `preset-column-widths`, `struts`) are replaced whole, so only
//! the last such block counts.

use super::{Store, StoreError, Target, after_terminator, line_end, line_start, removal_span};
use crate::diff::{Op, lcs_ops};
use crate::include;
use crate::kdl::{Document, Node, Span, ValueKind, quoted};

/// One step into nested config blocks: a node name, plus — for multipart
/// sections like `output "eDP-1"` — the first argument that picks the block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block<'view> {
    pub name: &'view str,
    pub arg: Option<&'view str>,
}

impl<'view> Block<'view> {
    pub const fn named(name: &'view str) -> Self {
        Self { name, arg: None }
    }

    pub const fn with_arg(name: &'view str, arg: &'view str) -> Self {
        Self {
            name,
            arg: Some(arg),
        }
    }
}

/// A display or workspace that sets a setting for itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Override {
    /// `output` or `workspace`.
    pub block: String,
    /// The display's or workspace's name: `eDP-1`.
    pub name: String,
    /// What it sets, when the setting is one value.
    pub value: Option<String>,
}

impl std::fmt::Display for Override {
    /// `output "eDP-1" (#3c0000)`.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "{} {}", self.block, quoted(&self.name))?;
        if let Some(value) = &self.value {
            write!(out, " ({value})")?;
        }
        Ok(())
    }
}

/// One column or window size as written: `proportion 0.5`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizeEntry {
    pub name: String,
    pub value: String,
}

/// Sections a later block replaces whole instead of merging into.
const REPLACED_WHOLE: [&str; 3] = ["preset-column-widths", "preset-window-heights", "struts"];

/// A node's file and its child indices from that file's roots.
#[derive(Debug, Clone)]
pub(super) struct NodeAddr {
    pub(super) target: Target,
    path: Vec<usize>,
}

impl NodeAddr {
    /// The top-level node at `index` of `target`'s file.
    pub(super) fn root(target: Target, index: usize) -> Self {
        Self {
            target,
            path: vec![index],
        }
    }

    fn child(&self, index: usize) -> Self {
        let mut path = self.path.clone();
        path.push(index);
        Self {
            target: self.target,
            path,
        }
    }
}

impl Store {
    /// The decoded first argument of the `child` niri applies inside the
    /// blocks `chain` (`[layout, focus-ring]` + `width` → `"4"`).
    pub fn setting(&self, chain: &[Block], child: &str) -> Option<String> {
        self.setting_node(chain, child)
            .and_then(|node| node.args.first())
            .map(|value| value.kind.as_str().to_string())
    }

    /// The `child` node niri applies, for reading its props or several args.
    pub fn setting_node(&self, chain: &[Block], child: &str) -> Option<&Node> {
        self.node(&self.applied(chain, child)?).ok()
    }

    /// Where the `child` niri applies is written: its file and 1-based line.
    pub fn setting_line(&self, chain: &[Block], child: &str) -> Option<(&std::path::Path, usize)> {
        let addr = self.applied(chain, child)?;
        let start = self.node(&addr).ok()?.node_span.start;
        Some((
            self.target_path(addr.target).ok()?,
            self.doc(addr.target).ok()?.line_of(start),
        ))
    }

    /// The top-level nodes named one of `names` that niri reads, in reading
    /// order, as their file and 1-based first and last lines.
    pub fn block_lines(&self, names: &[&str]) -> Vec<(&std::path::Path, usize, usize)> {
        self.read_nodes()
            .filter(|(_, node)| names.contains(&node.name.as_str()))
            .filter_map(|(addr, node)| {
                let span = node.node_span;
                let (path, doc) = self.file(addr.target)?;
                Some((
                    path,
                    doc.line_of(span.start),
                    doc.line_of(span.end.saturating_sub(1).max(span.start)),
                ))
            })
            .collect()
    }

    /// The first argument of every top-level `name` node niri reads, in
    /// reading order: the outputs a config sets up, say.
    pub fn block_names(&self, name: &str) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for (_, node) in self.read_nodes() {
            if node.name == name
                && let Some(first) = node.args.first()
            {
                let first = first.kind.as_str().to_string();
                if !names.contains(&first) {
                    names.push(first);
                }
            }
        }
        names
    }

    /// Every node named `name` in the loaded files, at any depth: its file
    /// and 1-based line. For pointing at a misspelled node niri rejected.
    pub fn nodes_named(&self, name: &str) -> Vec<(&std::path::Path, usize)> {
        let mut found = Vec::new();
        for (_, path, doc) in self.files() {
            for node in doc.iter_all().filter(|node| node.name == name) {
                found.push((path, doc.line_of(node.node_span.start)));
            }
        }
        found
    }

    /// The name niri's diagnostics say it does not know, and where: the node
    /// they point at, in the file they name (niri names an include as it is
    /// written, `dms/binds.kdl`), else the first node of that name.
    pub fn misspelled(&self, diagnostics: &str) -> Option<(String, &std::path::Path, usize)> {
        let found = crate::validate::diagnostics(diagnostics);
        let (name, place) = found.iter().find_map(|diagnostic| match &diagnostic.kind {
            crate::validate::DiagnosticKind::UnknownNode(name) => {
                Some((name.clone(), diagnostic.place.as_ref()))
            }
            crate::validate::DiagnosticKind::Other(_) => None,
        })?;
        let named = self.nodes_named(&name);
        let pointed = place.and_then(|(file, line)| {
            named
                .iter()
                .find(|(path, at)| at == line && path.ends_with(file))
                .copied()
        });
        let (path, line) = pointed.or_else(|| named.first().copied())?;
        Some((name, path, line))
    }

    /// Rename the node called `old` that starts on `line` of `path` to
    /// `new`: one splice over its name, everything around it untouched.
    pub fn rename_node(
        &mut self,
        path: &std::path::Path,
        line: usize,
        old: &str,
        new: &str,
    ) -> Result<(), StoreError> {
        let not_found = || StoreError::NodeNotFound {
            name: old.to_string(),
            line,
        };
        let target = self.target_for_path(path).ok_or_else(not_found)?;
        let doc = self.doc(target)?;
        let start = doc
            .iter_all()
            .find(|node| node.name == old && doc.line_of(node.node_span.start) == line)
            .map(|node| node.node_span.start)
            .ok_or_else(not_found)?;
        // a quoted name is left alone: it is not what a typo looks like
        if !doc.source[start..].starts_with(old) {
            return Err(not_found());
        }
        self.doc_mut(target)?
            .splice(Span::new(start, start + old.len()), new);
        self.refresh(target)
    }

    /// The variables niri sets for the programs it starts, in reading
    /// order, a later block overriding an earlier one: name and value,
    /// `None` for `NAME null`, which takes a variable away.
    pub fn environment(&self) -> Vec<(String, Option<String>)> {
        let mut variables: Vec<(String, Option<String>)> = Vec::new();
        for (_, node) in self.read_nodes() {
            if node.name != "environment" {
                continue;
            }
            for child in node.children.iter().filter(|child| !child.disabled) {
                let value = match child.args.first().map(|value| &value.kind) {
                    Some(ValueKind::Bare(word)) if word == "null" || word == "#null" => None,
                    Some(kind) => Some(kind.as_str().to_string()),
                    None => Some(String::new()),
                };
                match variables.iter_mut().find(|(name, _)| *name == child.name) {
                    Some(existing) => existing.1 = value,
                    None => variables.push((child.name.clone(), value)),
                }
            }
        }
        variables
    }

    /// Set a variable for the programs niri starts; `None` writes
    /// `NAME null`, which takes it away.
    pub fn set_environment(&mut self, name: &str, value: Option<&str>) -> Result<(), StoreError> {
        let value = value.map_or_else(|| "null".to_string(), quoted);
        self.set_setting(&[Block::named("environment")], name, &value)
    }

    /// Take a variable out of the config, so niri leaves it as it finds it.
    pub fn remove_environment(&mut self, name: &str) -> Result<(), StoreError> {
        self.unset_setting(&[Block::named("environment")], name)
    }

    /// The current text of a loaded file.
    pub fn text_of(&self, path: &std::path::Path) -> Option<&str> {
        let target = self.target_for_path(path)?;
        Some(self.doc(target).ok()?.source.as_str())
    }

    /// Whether niri sees `child` at all (presence flags like `tap`).
    pub fn is_set(&self, chain: &[Block], child: &str) -> bool {
        self.applied(chain, child).is_some()
    }

    /// Make niri apply `child <args>` (`args` is raw KDL, may be empty):
    /// rewrite the effective occurrence's arguments in place, or add the
    /// child to the last matching block, creating missing blocks.
    pub fn set_setting(
        &mut self,
        chain: &[Block],
        child: &str,
        args: &str,
    ) -> Result<(), StoreError> {
        let Some(addr) = self.applied(chain, child) else {
            return self.insert_setting(chain, child, args);
        };
        let node = self.node(&addr)?;
        let name_end = node.node_span.start + node.name.len();
        let args_end = node
            .args
            .iter()
            .map(|value| value.span.end)
            .chain(node.props.iter().map(|prop| prop.value.span.end))
            .max()
            .unwrap_or(name_end);
        let replacement = if args.is_empty() {
            String::new()
        } else {
            format!(" {args}")
        };
        self.doc_mut(addr.target)?
            .splice(Span::new(name_end, args_end), &replacement);
        self.refresh(addr.target)
    }

    /// Presence flags: add `flag` when turning on, remove every occurrence
    /// when turning off (an earlier one would otherwise still apply).
    pub fn set_flag_setting(
        &mut self,
        chain: &[Block],
        flag: &str,
        on: bool,
    ) -> Result<(), StoreError> {
        match (on, self.is_set(chain, flag)) {
            (true, false) => self.insert_setting(chain, flag, ""),
            (false, true) => self.unset_setting(chain, flag),
            (true, true) | (false, false) => Ok(()),
        }
    }

    /// Remove every occurrence of `child`, so niri falls back to its default.
    pub fn unset_setting(&mut self, chain: &[Block], child: &str) -> Result<(), StoreError> {
        while let Some(addr) = self.applied(chain, child) {
            let node_span = self.node(&addr)?.node_span;
            let source = &self.doc(addr.target)?.source;
            let (span, _) = removal_span(source, node_span);
            self.doc_mut(addr.target)?.splice(span, "");
            self.refresh(addr.target)?;
        }
        Ok(())
    }

    /// The file holding the occurrence of `child` niri applies, or `None`
    /// when the config leaves it unset.
    pub fn setting_file(&self, chain: &[Block], child: &str) -> Option<&std::path::Path> {
        self.target_path(self.applied(chain, child)?.target).ok()
    }

    /// The top-level blocks named one of `within` (`output "eDP-1"`) that
    /// set `child` at `inside`, a chain inside each, in reading order. A
    /// block named twice counts once, with the value niri applies.
    pub fn overrides(&self, within: &[&str], inside: &[Block], child: &str) -> Vec<Override> {
        let wanted = Block::named(child);
        let mut found: Vec<Override> = Vec::new();
        for (addr, node) in self.read_nodes() {
            if !within.contains(&node.name.as_str()) {
                continue;
            }
            let Some(name) = node.args.first().map(|value| value.kind.as_str()) else {
                continue;
            };
            // down this block's own copy of the chain
            let mut blocks = vec![addr.clone()];
            for step in inside {
                blocks = blocks
                    .iter()
                    .filter_map(|block| Some((block, self.node(block).ok()?)))
                    .flat_map(|(block, node)| {
                        node.children
                            .iter()
                            .enumerate()
                            .filter(|(_, child)| step_matches(child, step))
                            .map(|(index, _)| block.child(index))
                    })
                    .collect();
            }
            let Some(set) = blocks.iter().rev().find_map(|block| {
                self.node(block)
                    .ok()?
                    .children
                    .iter()
                    .rev()
                    .find(|node| step_matches(node, &wanted))
            }) else {
                continue;
            };
            let value = set
                .args
                .first()
                .map(|value| value.kind.as_str().to_string());
            match found
                .iter_mut()
                .find(|known| known.block == node.name && known.name == name)
            {
                Some(known) => known.value = value,
                None => found.push(Override {
                    block: node.name.clone(),
                    name: name.to_string(),
                    value,
                }),
            }
        }
        found
    }

    /// The children of the `block` niri applies inside `chain`, such as
    /// `proportion 0.5` in `preset-column-widths`. `None` when the config
    /// leaves the block unset.
    pub fn sizes(&self, chain: &[Block], block: &str) -> Option<Vec<SizeEntry>> {
        let node = self.setting_node(chain, block)?;
        Some(
            node.children
                .iter()
                .filter(|child| !child.disabled)
                .map(|child| {
                    let value = child
                        .args
                        .first()
                        .map(|value| value.kind.as_str().to_string())
                        .unwrap_or_default();
                    SizeEntry {
                        name: child.name.clone(),
                        value,
                    }
                })
                .collect(),
        )
    }

    /// Make the `block` niri applies hold these children; `None` removes it
    /// so niri's default applies. Only the entries that change are touched:
    /// a changed one is replaced where it stands, new ones go after the
    /// last, dropped ones go, and comments and `/-` lines in the block stay.
    /// A block on one line stays on one line.
    pub fn set_sizes(
        &mut self,
        chain: &[Block],
        block: &str,
        entries: Option<&[SizeEntry]>,
    ) -> Result<(), StoreError> {
        let Some(entries) = entries else {
            return self.unset_setting(chain, block);
        };
        let body: String = entries
            .iter()
            .map(|entry| format!(" {} {};", entry.name, entry.value))
            .collect();
        // a new block, or one written without braces: nothing inside to keep
        let braced = format!("{{{body} }}");
        let Some(addr) = self.applied(chain, block) else {
            return self.insert_setting(chain, block, &braced);
        };
        let node = self.node(&addr)?;
        let source = &self.doc(addr.target)?.source;
        let inline = !source[node.node_span.start..node.node_span.end].contains('\n');
        let Some(closing) = node.block.map(|block| block.end.saturating_sub(1)) else {
            let span = node.node_span;
            self.doc_mut(addr.target)?
                .splice(span, &format!("{block} {braced}"));
            return self.refresh(addr.target);
        };
        let children: Vec<&Node> = node
            .children
            .iter()
            .filter(|child| !child.disabled)
            .collect();
        let text = |entry: &SizeEntry| format!("{} {}", entry.name, entry.value);
        // entries kept stay as they are; between them, a changed entry is
        // written in place of an old one, the rest dropped or added there
        let wanted: Vec<&SizeEntry> = entries.iter().collect();
        let ops = lcs_ops(&children, &wanted, |child, entry| {
            child.name == entry.name
                && child.args.first().map_or("", |first| first.kind.as_str()) == entry.value
        });
        let mut edits: Vec<(Span, String)> = Vec::new();
        let mut last_old: Option<&Node> = None;
        let mut rest_of_ops = ops.as_slice();
        while let Some((op, after)) = rest_of_ops.split_first() {
            if let Op::Keep(child, _) = op {
                last_old = Some(child);
                rest_of_ops = after;
                continue;
            }
            let changed = rest_of_ops.iter().take_while(|op| !op.is_keep()).count();
            let (run, after) = rest_of_ops.split_at(changed);
            rest_of_ops = after;
            let dropped: Vec<&Node> = run
                .iter()
                .filter_map(|op| match op {
                    Op::Del(child) => Some(*child),
                    Op::Keep(..) | Op::Ins(_) => None,
                })
                .collect();
            let added: Vec<&SizeEntry> = run
                .iter()
                .filter_map(|op| match op {
                    Op::Ins(entry) => Some(*entry),
                    Op::Keep(..) | Op::Del(_) => None,
                })
                .collect();
            let paired = dropped.len().min(added.len());
            for (child, entry) in dropped.iter().zip(&added) {
                edits.push((child.node_span, text(entry)));
            }
            for child in dropped.iter().skip(paired) {
                edits.push((removal_span(source, child.node_span).0, String::new()));
            }
            let rest: Vec<String> = added.iter().skip(paired).map(|entry| text(entry)).collect();
            if !rest.is_empty() {
                let last_paired = paired
                    .checked_sub(1)
                    .and_then(|place| dropped.get(place).copied());
                let anchor = match last_paired.or(last_old) {
                    Some(child) => Anchor::After(child.node_span),
                    None => children.first().map_or(Anchor::Empty(closing), |first| {
                        Anchor::Before(first.node_span)
                    }),
                };
                edits.push(insertion(source, inline, anchor, &rest));
            }
            if let Some(&child) = dropped.last() {
                last_old = Some(child);
            }
        }
        let doc = self.doc_mut(addr.target)?;
        for (span, text) in edits {
            doc.splice(span, &text);
        }
        self.refresh(addr.target)
    }

    /// Active top-level nodes in reading order: each active `include` is
    /// replaced, where it stands, by the nodes of the file it includes.
    pub(super) fn reading_order(&self) -> &[NodeAddr] {
        &self.reading
    }

    /// [`Self::reading_order`] worked out anew, after a change to the text.
    pub(super) fn find_reading_order(&self) -> Vec<NodeAddr> {
        let mut out = Vec::new();
        let mut visiting = Vec::new();
        self.walk(Target::Main, &mut visiting, &mut out);
        out
    }

    fn walk(&self, target: Target, visiting: &mut Vec<Target>, out: &mut Vec<NodeAddr>) {
        // a cycle: include resolution has already reported it
        if visiting.contains(&target) {
            return;
        }
        let Some((path, doc)) = self.file(target) else {
            return;
        };
        visiting.push(target);
        for (index, node) in doc.roots.iter().enumerate() {
            if node.disabled {
                continue;
            }
            if node.name != "include" {
                out.push(NodeAddr {
                    target,
                    path: vec![index],
                });
                continue;
            }
            let included = node
                .args
                .first()
                .and_then(|value| match &value.kind {
                    ValueKind::Quoted(rel) => Some(rel.as_str()),
                    ValueKind::Bare(_) | ValueKind::Number(_) => None,
                })
                .map(|rel| include::include_path(path, rel))
                .and_then(|file| self.target_for_path(&file));
            if let Some(included) = included {
                self.walk(included, visiting, out);
            }
        }
        visiting.pop();
    }

    /// Each loaded file, main first: its target, path and document. They
    /// come from the list itself, so none can be gone.
    pub(super) fn files(&self) -> impl Iterator<Item = (Target, &std::path::Path, &Document)> {
        self.loaded().map(|file| (file.target, file.path, file.doc))
    }

    /// The path and document of the file `target` stands for.
    pub(super) fn file(&self, target: Target) -> Option<(&std::path::Path, &Document)> {
        Some((self.target_path(target).ok()?, self.doc(target).ok()?))
    }

    /// The active top-level nodes in reading order, with their addresses.
    /// The order is worked out from the documents as they are, so every
    /// address in it finds its node.
    pub(super) fn read_nodes(&self) -> impl Iterator<Item = (&NodeAddr, &Node)> {
        self.reading_order()
            .iter()
            .filter_map(|addr| Some((addr, self.node(addr).ok()?)))
    }

    pub(super) fn target_path(&self, target: Target) -> Result<&std::path::Path, StoreError> {
        match target {
            Target::Main => Ok(&self.path),
            Target::Included(index) => Ok(&self.included(index)?.path),
        }
    }

    /// The loaded file at `file`: the main config or an include.
    pub(super) fn target_for_path(&self, file: &std::path::Path) -> Option<Target> {
        if file == self.path {
            return Some(Target::Main);
        }
        self.included
            .iter()
            .position(|included| included.path == file)
            .map(Target::Included)
    }

    /// The node at `addr`, found anew; `Gone` when an edit since moved it.
    pub(super) fn node(&self, addr: &NodeAddr) -> Result<&Node, StoreError> {
        let doc = self.doc(addr.target)?;
        let (&root, below) = addr.path.split_first().ok_or(StoreError::Gone)?;
        let mut node = doc.roots.get(root).ok_or(StoreError::Gone)?;
        for &index in below {
            node = node.children.get(index).ok_or(StoreError::Gone)?;
        }
        Ok(node)
    }

    /// Every active block matching `chain`, in reading order.
    fn matching_blocks(&self, chain: &[Block]) -> Vec<NodeAddr> {
        let Some((first, rest)) = chain.split_first() else {
            return Vec::new();
        };
        let mut matched: Vec<NodeAddr> = self
            .read_nodes()
            .filter(|(_, node)| step_matches(node, first))
            .map(|(addr, _)| addr.clone())
            .collect();
        let mut parent_name = first.name;
        for step in rest {
            let mut next = Vec::new();
            for (parent, node) in matched
                .iter()
                .filter_map(|parent| Some((parent, self.node(parent).ok()?)))
            {
                for (index, child) in node.children.iter().enumerate() {
                    if step_matches(child, step) {
                        next.push(parent.child(index));
                    }
                }
            }
            if parent_name == "animations" || REPLACED_WHOLE.contains(&step.name) {
                next = next.pop().into_iter().collect();
            }
            parent_name = step.name;
            matched = next;
        }
        matched
    }

    /// The `child` niri applies inside the blocks matching `chain`
    /// (top-level when `chain` is empty): the last active one it reads.
    fn applied(&self, chain: &[Block], child: &str) -> Option<NodeAddr> {
        let step = Block::named(child);
        if chain.is_empty() {
            return self
                .read_nodes()
                .filter(|(_, node)| step_matches(node, &step))
                .last()
                .map(|(addr, _)| addr.clone());
        }
        self.matching_blocks(chain).iter().rev().find_map(|block| {
            let index = self
                .node(block)
                .ok()?
                .children
                .iter()
                .rposition(|node| step_matches(node, &step))?;
            Some(block.child(index))
        })
    }

    /// Add `child <args>` to the last block matching the longest existing
    /// prefix of `chain`, creating the rest of the chain inside it — or, when
    /// not even the first block exists, appending the whole chain to the
    /// main config.
    fn insert_setting(
        &mut self,
        chain: &[Block],
        child: &str,
        args: &str,
    ) -> Result<(), StoreError> {
        let line = if args.is_empty() {
            child.to_string()
        } else {
            format!("{child} {args}")
        };
        // the deepest block of the chain that is there, the rest made in it
        for depth in (1..=chain.len()).rev() {
            let Some((there, missing)) = chain.split_at_checked(depth) else {
                continue;
            };
            if let Some(parent) = self.matching_blocks(there).pop() {
                let lines = nested_lines(missing, &line);
                return self.insert_into_block(&parent, &lines);
            }
        }
        let lines = nested_lines(chain, &line);
        let doc = self.doc_mut(Target::Main)?;
        let end = doc.source.len();
        let lead = if doc.source.is_empty() || doc.source.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        doc.splice(
            Span::new(end, end),
            &format!("{lead}{}\n", lines.join("\n")),
        );
        self.refresh(Target::Main)
    }

    /// Insert `lines` (unindented) as the last children of the block at
    /// `parent`, matching the file's indentation.
    pub(super) fn insert_into_block(
        &mut self,
        parent: &NodeAddr,
        lines: &[String],
    ) -> Result<(), StoreError> {
        let node = self.node(parent)?;
        let block = node.block.ok_or_else(|| StoreError::BlockMissing {
            name: node.name.clone(),
        })?;
        let source = &self.doc(parent.target)?.source;
        let parent_indent = indentation(source, node.node_span.start);
        let child_indent = format!("{parent_indent}    ");
        let indented = |lines: &[String]| {
            lines
                .iter()
                .map(|line| format!("{child_indent}{line}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let closing = block.end - 1;
        let closing_line = line_start(source, closing);
        let interior = &source[block.start + 1..closing];
        let (span, text) = if interior.trim().is_empty() {
            // `{}` or `{ }`: open the block up
            (
                Span::new(block.start + 1, closing),
                format!("\n{}\n{parent_indent}", indented(lines)),
            )
        } else if source[closing_line..closing].trim().is_empty() && closing_line > block.start {
            // the closing brace has its own line: new lines go just above it
            (
                Span::new(closing_line, closing_line),
                format!("{}\n", indented(lines)),
            )
        } else if let [line] = lines {
            // a one-line block: stay on the line
            (Span::new(closing, closing), format!("{line}; "))
        } else {
            (
                Span::new(closing, closing),
                format!("\n{}\n{parent_indent}", indented(lines)),
            )
        };
        self.doc_mut(parent.target)?.splice(span, &text);
        self.refresh(parent.target)
    }
}

/// An active node that is the block `step` names.
fn step_matches(node: &Node, step: &Block) -> bool {
    !node.disabled
        && node.name == step.name
        && step.arg.is_none_or(|arg| {
            node.args
                .first()
                .is_some_and(|value| value.kind.as_str() == arg)
        })
}

/// `line` wrapped in the blocks `chain`, four spaces per level.
fn nested_lines(chain: &[Block], line: &str) -> Vec<String> {
    let Some((head, rest)) = chain.split_first() else {
        return vec![line.to_string()];
    };
    let opening = head.arg.map_or_else(
        || format!("{} {{", head.name),
        |arg| format!("{} {} {{", head.name, quoted(arg)),
    );
    let mut lines = vec![opening];
    lines.extend(
        nested_lines(rest, line)
            .into_iter()
            .map(|inner| format!("    {inner}")),
    );
    lines.push("}".to_string());
    lines
}

/// Where new entries of a block go, relative to the entries it has.
enum Anchor {
    After(Span),
    Before(Span),
    /// The block has none: inside its braces, before the closing one here.
    Empty(usize),
}

/// Where and what to add for `entries` (`fixed 800`) at `anchor`: on the
/// block's one line, or a line each, indented like their neighbors.
fn insertion(source: &str, inline: bool, anchor: Anchor, entries: &[String]) -> (Span, String) {
    let joined =
        |format: &dyn Fn(&String) -> String| entries.iter().map(format).collect::<String>();
    match (inline, anchor) {
        (true, Anchor::After(last)) => match after_terminator(source, last.end) {
            Some(at) => (Span::new(at, at), joined(&|entry| format!(" {entry};"))),
            None => (
                Span::new(last.end, last.end),
                joined(&|entry| format!("; {entry}")),
            ),
        },
        (true, Anchor::Before(first)) => (
            Span::new(first.start, first.start),
            joined(&|entry| format!("{entry}; ")),
        ),
        (true, Anchor::Empty(closing)) => (
            Span::new(closing, closing),
            format!("{} ", joined(&|entry| format!(" {entry};"))),
        ),
        (false, anchor) => {
            let (at, indent) = match anchor {
                Anchor::After(last) => {
                    (line_end(source, last.end), indentation(source, last.start))
                }
                Anchor::Before(first) => (
                    line_start(source, first.start),
                    indentation(source, first.start),
                ),
                Anchor::Empty(closing) => (
                    line_start(source, closing),
                    format!("{}    ", indentation(source, closing)),
                ),
            };
            (
                Span::new(at, at),
                joined(&|entry| format!("{indent}{entry}\n")),
            )
        }
    }
}

/// The whitespace a line starts with, for the line holding `pos`.
pub(super) fn indentation(source: &str, pos: usize) -> String {
    source[line_start(source, pos)..]
        .chars()
        .take_while(|character| matches!(character, ' ' | '\t'))
        .collect()
}
