//! Config store: load the whole config set (main + `include`s) → edit via
//! lossless splices → validate the full set → save.
//!
//! Guarantees this layer exists to provide:
//!
//! * **losslessness** — edits are spliced into the exact source bytes; only
//!   the file that owns the edited node is touched;
//! * **safety** — every save first runs the complete edited set through
//!   `niri validate` (mirrored into a temp tree so relative includes resolve)
//!   and only then atomically renames files into place;
//! * **origin-awareness** — with multi-file configs (the norm for niri), each
//!   row in the UI knows which file it came from, so `add`/`remove` write to
//!   the right one (`keybinds.kdl` for binds, `spawn_at_startup.kdl` for
//!   autostart entries, falling back to the main config).

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use crate::bindings::KeyCombo;
use crate::disk::SaveJob;
use crate::history::FileChange;
use crate::include::{self, IncludedFile};
use crate::kdl::{Document, EditError, Node, Span, active_named, parse_document, quoted};

mod engine;
use engine::indentation;
pub use engine::{Block, Override};

const LAYOUT: [Block<'static>; 1] = [Block::named("layout")];
const TOUCHPAD: [Block<'static>; 2] = [Block::named("input"), Block::named("touchpad")];
const KEYBOARD: [Block<'static>; 2] = [Block::named("input"), Block::named("keyboard")];
const XKB: [Block<'static>; 3] = [
    Block::named("input"),
    Block::named("keyboard"),
    Block::named("xkb"),
];

/// `animations { <animation> { … } }`.
const fn animation_chain(animation: &str) -> [Block<'_>; 2] {
    [Block::named("animations"), Block::named(animation)]
}

/// A loaded config set: the main file plus its includes.
#[derive(Debug, Clone)]
pub struct Store {
    path: PathBuf,
    /// Pristine on-disk content of the main file (never mutated).
    original_main: String,
    doc: Document,
    included: Vec<IncludedFile>,
    include_warnings: Vec<String>,
    /// Files beside the config that nothing includes yet, with their text,
    /// for the app to include later: the rule library's.
    spare: Vec<(PathBuf, String)>,
    /// Moves on every change to a file's text, and only then; unique across
    /// stores, so a store loaded anew counts as a change.
    revision: u64,
    /// The active top-level nodes in the order niri reads them, worked out
    /// at every change to the text: every settings row asks for it.
    reading: Vec<engine::NodeAddr>,
}

/// The next store revision.
fn next_revision() -> u64 {
    static REVISIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    REVISIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// A keybinding row: structured key + action, with the origin file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingRow {
    pub key: String,
    /// Display text of the action (e.g. `spawn swayosd-client --output-volume raise`).
    pub action: String,
    /// The action exactly as written in the file (e.g. `spawn "kitty"`): what
    /// an edit form must start from, since `action` drops the quoting.
    pub action_raw: String,
    /// The file it is in, as [`Store::file_label`] names it.
    pub file: String,
    /// It is in an included file, not the main config.
    pub included: bool,
    /// True when the binding is `/-`-commented out (disabled, not deleted).
    pub disabled: bool,
    /// The file of a later binding of the same key, which niri uses instead:
    /// the one read last wins.
    pub hidden_by: Option<String>,
    /// The files of earlier bindings of the same key that this one replaces.
    pub replaces: Vec<String>,
}

/// One row in a plain settings table (autostart): display text + its file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub display: String,
    /// The file it is in, as [`Store::file_label`] names it.
    pub file: String,
    /// It is in an included file, not the main config.
    pub included: bool,
    /// True when the entry is `/-`-commented out (disabled, not deleted).
    pub disabled: bool,
}

/// A file in the loaded set: the main config or one of its includes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Target {
    Main,
    Included(usize),
}

/// Opt-in window rules for niri-center's own windows.
///
/// Single-pane windows (`niri-center --pane …`, app-id `niri-center.<pane>`)
/// float, while the full app (app-id `niri-center`) tiles like any other window.
pub const SUGGESTED_WINDOW_RULES: &str = r##"// niri-center: single-pane windows (`niri-center --pane …`) open floating.
window-rule {
    match app-id=r#"^niri-center\."#
    open-floating true
}
"##;

/// Failures from store operations.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("no config location found (XDG_CONFIG_HOME not set)")]
    NoConfigDir,
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse config: {0}")]
    Parse(crate::kdl::ParseError),
    #[error("edit failed: {0}")]
    Edit(#[from] EditError),
    #[error("no `binds` block in the loaded config set")]
    NoBinds,
    #[error("entry at index {index} not found")]
    EntryNotFound { index: usize },
    #[error("no `{name}` block in the loaded config set")]
    BlockMissing { name: String },
    #[error("output `{name}` not found in the loaded config set")]
    OutputNotFound { name: String },
    #[error("no file in the snapshot is part of the config now")]
    NoSnapshotFiles,
    #[error("no `{name}` on line {line}")]
    NodeNotFound { name: String, line: usize },
    #[error("{path} is not part of the loaded config")]
    NotLoaded { path: PathBuf },
    #[error("the text must be exactly one window-rule {{ … }} block")]
    NotOneRule,
    #[error("`{flag}` flag not found in the input tree")]
    FlagMissing { flag: String },
    #[error("the {what} just written is not where it was written")]
    NotWhereWritten { what: &'static str },
}

impl Store {
    pub fn default_path() -> Option<PathBuf> {
        // Honor niri's own override, then the standard location.
        if let Some(custom) = std::env::var_os("NIRI_CONFIG") {
            return Some(PathBuf::from(custom));
        }
        crate::xdg::base("XDG_CONFIG_HOME", ".config").map(|base| base.join("niri/config.kdl"))
    }

    /// The config at `path` and its includes, each file's text from `read`:
    /// the disk's, or memory's.
    pub fn read(
        path: PathBuf,
        mut read: impl FnMut(&Path) -> io::Result<String>,
    ) -> Result<Self, StoreError> {
        let source = read(&path).map_err(|source| StoreError::Io {
            path: path.clone(),
            source,
        })?;
        let doc = parse_document(&source).map_err(StoreError::Parse)?;
        let (included, include_warnings) = include::resolve(&path, &doc, &mut read);
        Ok(Self {
            path,
            original_main: source,
            doc,
            included,
            include_warnings,
            spare: Vec::new(),
            revision: next_revision(),
            reading: Vec::new(),
        })
        .map(|mut store| {
            store.reading = store.find_reading_order();
            store
        })
    }

    /// A config from one text in hand, as if read from `path`, with no
    /// includes to read: the browser demo's, a library rule's.
    pub fn from_text(path: PathBuf, text: &str) -> Result<Self, StoreError> {
        let main = path.clone();
        Self::read(path, |asked| {
            if asked == main {
                Ok(text.to_string())
            } else {
                Err(io::Error::from(io::ErrorKind::NotFound))
            }
        })
    }

    /// A number that changes whenever the staged text does: cheaper to
    /// compare than the text.
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Directory containing the main config file (where it and its includes
    /// live).
    pub fn config_dir(&self) -> &Path {
        self.path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
    }

    /// A file's name as the app shows it: its path from the main config's
    /// folder (`config.kdl`, `dms/binds.kdl`), or from home (`~/…`) when it
    /// lives elsewhere.
    pub fn file_label(&self, path: &Path) -> String {
        match path.strip_prefix(self.config_dir()) {
            Ok(rest) => rest.display().to_string(),
            Err(_) => tilde(path),
        }
    }

    pub const fn document(&self) -> &Document {
        &self.doc
    }

    pub fn included_files(&self) -> &[IncludedFile] {
        &self.included
    }

    pub fn include_warnings(&self) -> &[String] {
        &self.include_warnings
    }

    pub fn has_includes(&self) -> bool {
        !self.included.is_empty() || active_named(&self.doc.roots, "include").next().is_some()
    }

    // ---- aggregation over the whole set (main + includes) ----

    /// All files in order (main first, then includes).
    fn targets(&self) -> impl Iterator<Item = Target> {
        std::iter::once(Target::Main).chain((0..self.included.len()).map(Target::Included))
    }

    /// Immutable document access by file.
    fn doc(&self, target: Target) -> &Document {
        match target {
            Target::Main => &self.doc,
            Target::Included(i) => &self.included[i].doc,
        }
    }

    /// Mutable document access by file. Every edit through it ends in
    /// [`Self::refresh`], which tells a real change from a no-op.
    fn doc_mut(&mut self, target: Target) -> &mut Document {
        match target {
            Target::Main => &mut self.doc,
            Target::Included(i) => &mut self.included[i].doc,
        }
    }

    /// Every binding over the whole set, in index order: its file, its
    /// place among that file's bindings, and its node.
    fn bindings(&self) -> impl Iterator<Item = (Target, usize, &Node)> {
        self.targets().flat_map(move |target| {
            active_named(&self.doc(target).roots, "binds")
                .flat_map(|block| &block.children)
                .enumerate()
                .map(move |(position, node)| (target, position, node))
        })
    }

    /// Top-level keybinding rows: structured key + action, with origin file.
    pub fn binding_rows(&self) -> Vec<BindingRow> {
        let labels: Vec<String> = self
            .targets()
            .map(|target| self.file_label(self.target_path(target)))
            .collect();
        let label = |target: Target| match target {
            Target::Main => labels[0].clone(),
            Target::Included(i) => labels[i + 1].clone(),
        };
        // every active binding as niri reads them, includes in place: for a
        // combo bound more than once, the last one wins
        let mut read: Vec<Target> = Vec::new();
        let mut place: HashMap<(Target, usize), usize> = HashMap::new();
        let mut by_combo: HashMap<KeyCombo, Vec<usize>> = HashMap::new();
        for addr in self.reading_order() {
            let node = self.node(addr);
            if node.name != "binds" {
                continue;
            }
            for child in node.children.iter().filter(|child| !child.disabled) {
                place.insert((addr.target, child.node_span.start), read.len());
                by_combo
                    .entry(KeyCombo::parse(&child.name))
                    .or_default()
                    .push(read.len());
                read.push(addr.target);
            }
        }
        let mut out = Vec::new();
        for (target, _, child) in self.bindings() {
            let doc = self.doc(target);
            let at = place.get(&(target, child.node_span.start)).copied();
            let same = by_combo
                .get(&KeyCombo::parse(&child.name))
                .map_or(&[][..], Vec::as_slice);
            let (hidden_by, replaces) = match (at, same.last()) {
                (Some(at), Some(&last)) if last != at => (Some(label(read[last])), Vec::new()),
                (Some(at), _) => (
                    None,
                    same.iter()
                        .filter(|&&index| index != at)
                        .map(|&index| label(read[index]))
                        .collect(),
                ),
                (None, _) => (None, Vec::new()),
            };
            out.push(BindingRow {
                key: child.name.clone(),
                action: node_display(doc, child),
                action_raw: child
                    .children
                    .first()
                    .map(|action| action.raw(doc).to_string())
                    .unwrap_or_default(),
                file: label(target),
                included: target != Target::Main,
                disabled: child.disabled,
                hidden_by,
                replaces,
            });
        }
        out
    }

    /// Autostart rows: the `spawn-at-startup` / `spawn-sh-at-startup` nodes.
    pub fn autostart_rows(&self) -> Vec<Row> {
        self.autostarts()
            .map(|(target, node)| {
                let doc = self.doc(target);
                let args: Vec<String> = node.args.iter().map(|v| doc.value_text(v)).collect();
                Row {
                    display: args.join(" "),
                    file: self.file_label(self.target_path(target)),
                    included: target != Target::Main,
                    disabled: node.disabled,
                }
            })
            .collect()
    }

    /// Every autostart entry over the whole set, in index order.
    fn autostarts(&self) -> impl Iterator<Item = (Target, &Node)> {
        self.targets().flat_map(move |target| {
            self.doc(target)
                .iter_all()
                .filter(|node| is_autostart(node))
                .map(move |node| (target, node))
        })
    }

    /// Keys that are bound more than once (across all files).
    pub fn duplicate_keys(&self) -> Vec<String> {
        let mut seen: HashMap<KeyCombo, usize> = HashMap::new();
        let mut dups = Vec::new();
        // niri skips `/-` bindings, so they cannot clash with anything
        for row in self.binding_rows().into_iter().filter(|row| !row.disabled) {
            let count = seen.entry(KeyCombo::parse(&row.key)).or_insert(0);
            *count += 1;
            if *count == 2 {
                dups.push(row.key);
            }
        }
        dups
    }

    /// The binding that already has `key` in the file a binding would go
    /// to, by index: niri refuses a combo bound twice in one file (in
    /// another file the one read last wins). `editing` is the binding being
    /// changed, which may keep its own key; a new one goes where
    /// [`Self::add_binding`] puts it.
    pub fn binding_in_the_way(&self, key: &str, editing: Option<usize>) -> Option<usize> {
        let file = match editing {
            Some(index) => self.bindings().nth(index)?.0,
            None => self.binding_file(),
        };
        let combo = KeyCombo::parse(key);
        self.bindings()
            .enumerate()
            .find(|(index, (target, _, node))| {
                *target == file
                    && !node.disabled
                    && Some(*index) != editing
                    && KeyCombo::parse(&node.name) == combo
            })
            .map(|(index, _)| index)
    }

    pub fn has_binds(&self) -> bool {
        self.first_doc_with(root_has("binds")).is_some()
    }

    // ---- mutations (splices only; validation happens on save) ----

    /// Apply a file's pending splices and parse it again. Only a change to
    /// its text is a new revision: an edit that writes what was there
    /// already is none.
    fn refresh(&mut self, target: Target) -> Result<(), StoreError> {
        let doc = self.doc(target);
        if doc.pending_edits() == 0 {
            return Ok(());
        }
        let text = doc.to_string();
        let changed = text != doc.source;
        *self.doc_mut(target) = parse_document(&text).map_err(StoreError::Parse)?;
        if changed {
            self.revision = next_revision();
            self.reading = self.find_reading_order();
        }
        Ok(())
    }

    /// Append a `binds { }` block to the *main* config if none exists anywhere.
    pub fn ensure_binds(&mut self) -> Result<(), StoreError> {
        if self.has_binds() {
            return Ok(());
        }
        let end = self.doc.source.len();
        self.doc.splice(Span::new(end, end), "\nbinds {\n}\n");
        self.refresh(Target::Main)
    }

    // ---- window rules ----

    /// Every top-level `window-rule` (disabled ones included), file by file.
    pub fn window_rule_rows(&self) -> Vec<WindowRuleRow> {
        self.window_rules()
            .map(|(target, node)| {
                let doc = self.doc(target);
                let (criteria, properties): (Vec<&Node>, Vec<&Node>) = node
                    .children
                    .iter()
                    .partition(|child| matches!(child.name.as_str(), "match" | "exclude"));
                let text = |child: &&Node| child.raw(doc).to_string();
                WindowRuleRow {
                    matchers: criteria
                        .iter()
                        .map(|child| Matcher {
                            exclude: child.name == "exclude",
                            props: child
                                .props
                                .iter()
                                .map(|prop| {
                                    MatchProp::new(
                                        MatchName::read(&prop.name),
                                        prop.value.kind.as_str(),
                                    )
                                })
                                .collect(),
                        })
                        .collect(),
                    line: doc.line_of(node.node_span.start),
                    criteria: criteria.iter().map(text).collect(),
                    properties: properties.iter().map(text).collect(),
                    file: self.file_label(self.target_path(target)),
                    included: target != Target::Main,
                    disabled: node.disabled,
                    plain: holds_only_its_lines(doc, node),
                }
            })
            .collect()
    }

    /// Whether a window rule that is on matches windows in a state: one of
    /// its `match` lines asks for `name` to be `yes` (`is-urgent=true`).
    /// Looks at the nodes, without making a row of every rule.
    pub fn has_rule_for_state(&self, name: &MatchName, yes: bool) -> bool {
        let value = if yes { "true" } else { "false" };
        self.window_rules().any(|(_, rule)| {
            !rule.disabled
                && rule.children.iter().any(|line| {
                    line.name == "match"
                        && !line.disabled
                        && line.props.iter().any(|prop| {
                            prop.name == name.as_str() && prop.value.kind.as_str() == value
                        })
                })
        })
    }

    /// Every top-level `window-rule`, active or not, in file order.
    fn window_rules(&self) -> impl Iterator<Item = (Target, &Node)> {
        self.targets().flat_map(move |target| {
            self.doc(target)
                .roots
                .iter()
                .filter(|node| node.name == "window-rule")
                .map(move |node| (target, node))
        })
    }

    fn window_rule_at(&self, index: usize) -> Result<(Target, Span), StoreError> {
        self.window_rules()
            .nth(index)
            .map(|(target, node)| (target, node.node_span))
            .ok_or(StoreError::EntryNotFound { index })
    }

    /// `/-`-comment out the window rule at `index`, or take the marker
    /// away again.
    pub fn set_window_rule_disabled(
        &mut self,
        index: usize,
        disabled: bool,
    ) -> Result<(), StoreError> {
        let (target, span) = self.window_rule_at(index)?;
        self.set_disabled(target, span, disabled)
    }

    /// Delete the window rule at `index`, block and all.
    pub fn remove_window_rule(&mut self, index: usize) -> Result<(), StoreError> {
        let (target, node_span) = self.window_rule_at(index)?;
        let (span, _) = removal_span(&self.doc(target).source, node_span);
        self.doc_mut(target).splice(span, "");
        self.refresh(target)
    }

    /// Append a rule matching one app (and optionally one title) exactly,
    /// with `properties` as raw KDL lines (`open-floating true`). It goes
    /// after the last active rule in reading order, so it takes precedence.
    pub fn add_window_rule(&mut self, rule: &NewWindowRule) -> Result<usize, StoreError> {
        self.add_window_rule_text(&rule.to_kdl())
    }

    /// Append a whole `window-rule { … }` block, written as given, where
    /// [`Self::add_window_rule`] would put it. Gives where the rule is
    /// among [`Self::window_rule_rows`].
    pub fn add_window_rule_text(&mut self, text: &str) -> Result<usize, StoreError> {
        let target = self
            .reading_order()
            .iter()
            .rfind(|addr| self.node(addr).name == "window-rule")
            .map_or(Target::Main, |addr| addr.target);
        append_block(self.doc_mut(target), text.trim_end());
        self.refresh(target)?;
        // appended at the end of its file: the last rule there
        self.window_rules()
            .enumerate()
            .filter(|(_, (at, _))| *at == target)
            .map(|(index, _)| index)
            .last()
            .ok_or(StoreError::NotWhereWritten {
                what: "window rule",
            })
    }

    /// Append `text` to the file `name` in the config's folder, included
    /// from the main config: the file and its `include` line are added when
    /// the config has neither yet. Nothing is written until saved.
    pub fn add_to_include(&mut self, name: &str, text: &str) -> Result<(), StoreError> {
        let path = self.config_dir().join(name);
        let loaded = self.target_for_path(&path);
        let target = match loaded {
            Some(target) => target,
            None => {
                // a file of that name that nothing includes yet is kept
                let original = self
                    .spare
                    .iter()
                    .find(|(spare, _)| *spare == path)
                    .map(|(_, text)| text.clone())
                    .unwrap_or_default();
                let doc = parse_document(&original).map_err(StoreError::Parse)?;
                self.included.push(IncludedFile {
                    path,
                    original,
                    doc,
                });
                append_block(
                    self.doc_mut(Target::Main),
                    &format!("include {}", quoted(name)),
                );
                self.refresh(Target::Main)?;
                Target::Included(self.included.len() - 1)
            }
        };
        append_block(self.doc_mut(target), text.trim_end());
        self.refresh(target)
    }

    /// The window rule at `index` exactly as written, for editing as text.
    pub fn window_rule_source(&self, index: usize) -> Option<String> {
        let (target, node) = self.window_rules().nth(index)?;
        Some(node.raw(self.doc(target)).to_string())
    }

    /// Replace the window rule at `index` with `text`, which must be one
    /// `window-rule` block. Only that block's bytes change, and the rule
    /// keeps its place: `index` is where it is.
    pub fn replace_window_rule(&mut self, index: usize, text: &str) -> Result<usize, StoreError> {
        let text = text.trim();
        let parsed = parse_document(text).map_err(StoreError::Parse)?;
        let [node] = parsed.roots.as_slice() else {
            return Err(StoreError::NotOneRule);
        };
        if node.name != "window-rule" || node.disabled {
            return Err(StoreError::NotOneRule);
        }
        let (target, span) = self.window_rule_at(index)?;
        self.doc_mut(target).splice(span, text);
        self.refresh(target)?;
        Ok(index)
    }

    /// Whether any active `window-rule` in the set already targets
    /// niri-center's windows (suggested or hand-written).
    pub fn has_niri_center_rules(&self) -> bool {
        self.window_rules().any(|(target, node)| {
            !node.disabled && node.raw(self.doc(target)).contains("niri-center")
        })
    }

    /// Stage [`SUGGESTED_WINDOW_RULES`] at the end of the main config
    /// (lossless append, saved like any edit). Returns `false` without
    /// changing anything when niri-center rules already exist.
    pub fn add_suggested_window_rules(&mut self) -> Result<bool, StoreError> {
        if self.has_niri_center_rules() {
            return Ok(false);
        }
        let end = self.doc.source.len();
        self.doc
            .splice(Span::new(end, end), &format!("\n{SUGGESTED_WINDOW_RULES}"));
        self.refresh(Target::Main)?;
        Ok(true)
    }

    /// The file (main first, then includes in order) that satisfies `pred`.
    fn first_doc_with(&self, pred: impl Fn(&Document) -> bool) -> Option<Target> {
        self.targets().find(|&t| pred(self.doc(t)))
    }

    /// Add `key { body; }` to the first file that has a binds block (main
    /// first, then includes in order); the main config if none does. Gives
    /// where the binding is among [`Self::binding_rows`].
    pub fn add_binding(&mut self, key: &str, body: &str) -> Result<usize, StoreError> {
        // splices into the main config when no file has binds yet
        self.ensure_binds()?;
        let target = self.binding_file();
        let index = self
            .doc(target)
            .roots
            .iter()
            .position(|node| node.name == "binds" && !node.disabled)
            .ok_or(StoreError::NoBinds)?;
        self.insert_into_block(
            &engine::NodeAddr::root(target, index),
            &[binding_line(key, body)],
        )?;
        // the last line of the block it went into
        let added = self.doc(target).roots[index]
            .children
            .last()
            .map(|node| node.node_span);
        self.bindings()
            .position(|(at, _, node)| at == target && Some(node.node_span) == added)
            .ok_or(StoreError::NotWhereWritten { what: "binding" })
    }

    /// The file new bindings go to: the first with a binds block (main
    /// first, then includes in order), else the main config.
    fn binding_file(&self) -> Target {
        self.first_doc_with(root_has("binds"))
            .unwrap_or(Target::Main)
    }

    /// The file [`Self::add_binding`] writes to, as [`Self::file_label`]
    /// names it.
    pub fn new_binding_file(&self) -> String {
        self.file_label(self.target_path(self.binding_file()))
    }

    /// Remove the binding at `index` (0-based over the whole set). The
    /// returned [`RemovedBinding`] puts it back verbatim via
    /// [`Self::restore_binding`].
    pub fn remove_binding(&mut self, index: usize) -> Result<RemovedBinding, StoreError> {
        let (target, position, node_span) = self
            .binding_in_file(index)
            .ok_or(StoreError::EntryNotFound { index })?;
        let source = self.doc(target).source.clone();
        let (span, whole_line) = removal_span(&source, node_span);
        let removed = RemovedBinding {
            target,
            position,
            text: source[span.start..span.end].to_string(),
            whole_line,
        };
        self.doc_mut(target).splice(span, "");
        self.refresh(target)?;
        Ok(removed)
    }

    /// Undo [`Self::remove_binding`]: re-insert the removed text in its file,
    /// before the binding that now holds its position (or at the end of that
    /// file's last `binds` block).
    pub fn restore_binding(&mut self, removed: &RemovedBinding) -> Result<(), StoreError> {
        let doc = self.doc(removed.target);
        let binds_blocks: Vec<&Node> = active_named(&doc.roots, "binds").collect();
        let next = binds_blocks
            .iter()
            .flat_map(|block| &block.children)
            .nth(removed.position);
        let insert_at = match (next, removed.whole_line) {
            (Some(next), true) => line_start(&doc.source, next.node_span.start),
            (Some(next), false) => next.node_span.start,
            (None, whole_line) => {
                let block = binds_blocks
                    .last()
                    .and_then(|b| b.block)
                    .ok_or(StoreError::NoBinds)?;
                let closing_brace = block.end.saturating_sub(1);
                let brace_line = line_start(&doc.source, closing_brace);
                let brace_on_own_line = doc.source[brace_line..closing_brace]
                    .bytes()
                    .all(|b| matches!(b, b' ' | b'\t'));
                if whole_line && brace_on_own_line {
                    brace_line
                } else {
                    closing_brace
                }
            }
        };
        let text = if removed.whole_line {
            removed.text.clone()
        } else {
            format!("{} ", removed.text)
        };
        self.doc_mut(removed.target)
            .splice(Span::new(insert_at, insert_at), &text);
        self.refresh(removed.target)
    }

    /// `/-`-comment out the binding at `index`, or take the marker away
    /// again.
    pub fn set_binding_disabled(&mut self, index: usize, disabled: bool) -> Result<(), StoreError> {
        let (target, _, span) = self
            .binding_in_file(index)
            .ok_or(StoreError::EntryNotFound { index })?;
        self.set_disabled(target, span, disabled)
    }

    /// `/-`-comment out the autostart entry at `index`, or take the marker
    /// away again.
    pub fn set_autostart_disabled(
        &mut self,
        index: usize,
        disabled: bool,
    ) -> Result<(), StoreError> {
        let (target, span) = self
            .autostart_node_at(index)
            .ok_or(StoreError::EntryNotFound { index })?;
        self.set_disabled(target, span, disabled)
    }

    /// Insert or remove the `/ -` disable marker on a node's whole line,
    /// preserving its indentation.
    fn set_disabled(
        &mut self,
        target: Target,
        node_span: Span,
        disabled: bool,
    ) -> Result<(), StoreError> {
        let source = &self.doc(target).source;
        // where the line's text starts: a `/-` marker, or the node
        let start =
            line_start(source, node_span.start) + indentation(source, node_span.start).len();
        let span = if disabled {
            Some((Span::new(start, start), "/- "))
        } else {
            source[start..].strip_prefix("/-").map(|rest| {
                let spaces = rest.len() - rest.trim_start_matches([' ', '\t']).len();
                (Span::new(start, start + 2 + spaces), "")
            })
        };
        if let Some((span, text)) = span {
            self.doc_mut(target).splice(span, text);
        }
        self.refresh(target)
    }

    /// The file of the binding at `index`, its position among that file's
    /// bindings, and its node span.
    fn binding_in_file(&self, index: usize) -> Option<(Target, usize, Span)> {
        self.bindings()
            .nth(index)
            .map(|(target, position, node)| (target, position, node.node_span))
    }

    /// Everything needed to locate and edit one binding: which file, where the
    /// node starts/ends, where its name token ends, and where its action child
    /// sits (if it has a `{ … }` block).
    fn binding_at(&self, index: usize) -> Option<BindingMeta> {
        let (target, _, node) = self.bindings().nth(index)?;
        let bytes = self.doc(target).source.as_bytes();
        let mut end = node.node_span.start;
        while bytes
            .get(end)
            .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'+'))
        {
            end += 1;
        }
        Some(BindingMeta {
            target,
            node_span: node.node_span,
            name_span: Span::new(node.node_span.start, end),
            action_span: node.children.first().map(|action| action.node_span),
        })
    }

    /// Change a binding's key and action, keeping everything else on the line
    /// (props like `hotkey-overlay-title`, `allow-when-locked`, a `/-` marker).
    /// The binding keeps its place: `index` is where it is.
    pub fn edit_binding(
        &mut self,
        index: usize,
        key: &str,
        body: &str,
    ) -> Result<usize, StoreError> {
        let meta = self
            .binding_at(index)
            .ok_or(StoreError::EntryNotFound { index })?;
        match meta.action_span {
            Some(action_span) => {
                // the action's `;` terminator is there already: the key and
                // the action node change, nothing else
                let doc = self.doc_mut(meta.target);
                doc.splice(meta.name_span, key);
                doc.splice(action_span, body.trim_end().trim_end_matches(';'));
            }
            None => {
                // no `{ … }` block: the node is written anew
                self.doc_mut(meta.target)
                    .splice(meta.node_span, &binding_line(key, body));
            }
        }
        self.refresh(meta.target)?;
        Ok(index)
    }

    /// Add an autostart entry (`spawn-at-startup "<cmd>"`), preferring a file
    /// that already has autostart nodes (e.g. `spawn_at_startup.kdl`).
    /// `command` may itself start with `spawn-at-startup`/`spawn-sh-at-startup`
    /// to be taken verbatim.
    pub fn add_autostart(&mut self, command: &str) -> Result<(), StoreError> {
        let trimmed = command.trim();
        let verbatim =
            trimmed.starts_with("spawn-at-startup ") || trimmed.starts_with("spawn-sh-at-startup ");
        // spawn-at-startup takes argv items, so a command with arguments as
        // one item names a program that does not exist: give those a shell
        let line = if verbatim {
            trimmed.to_string()
        } else if trimmed.contains(char::is_whitespace) {
            format!("spawn-sh-at-startup {}", quoted(trimmed))
        } else {
            format!("spawn-at-startup {}", quoted(trimmed))
        };

        let target = self
            .first_doc_with(|doc| doc.iter_all().any(is_autostart))
            .unwrap_or(Target::Main);
        let doc = self.doc_mut(target);
        let end = doc.source.len();
        let sep = if doc.source.ends_with('\n') { "" } else { "\n" };
        doc.splice(Span::new(end, end), &format!("{sep}{line}\n"));
        self.refresh(target)
    }

    /// Remove the autostart entry at `index` over the whole set.
    pub fn remove_autostart(&mut self, index: usize) -> Result<(), StoreError> {
        let (target, span) = self
            .autostart_node_at(index)
            .ok_or(StoreError::EntryNotFound { index })?;
        self.splice_node_line(target, span)
    }

    /// The (file, node span) of the autostart entry at `index`.
    fn autostart_node_at(&self, index: usize) -> Option<(Target, Span)> {
        self.autostarts()
            .nth(index)
            .map(|(target, node)| (target, node.node_span))
    }

    /// Splice out a node's whole line (indent + line break included) from the
    /// file it belongs to.
    fn splice_node_line(&mut self, target: Target, span: Span) -> Result<(), StoreError> {
        let source = self.doc(target).source.clone();
        let start = line_start(&source, span.start);
        self.doc_mut(target).splice(Span::new(start, span.end), "");
        self.refresh(target)
    }

    // ---- settings with a dedicated page (all through the engine) ----

    /// First active output in reading order (for the appearance page).
    pub fn first_output_name(&self) -> Option<String> {
        self.block_names("output").into_iter().next()
    }

    /// Current `backdrop-color` of an output, if set.
    pub fn output_backdrop_color(&self, name: &str) -> Option<String> {
        self.setting(&[Block::with_arg("output", name)], "backdrop-color")
    }

    /// Current `layout` `gaps` value, if set.
    pub fn layout_gaps(&self) -> Option<f64> {
        self.setting(&LAYOUT, "gaps")?.parse().ok()
    }

    /// Set `layout { gaps N }`.
    pub fn set_layout_gaps(&mut self, gaps: f64) -> Result<(), StoreError> {
        self.set_setting(&LAYOUT, "gaps", &format!("{gaps}"))
    }

    /// Current `layout` `center-focused-column` value, if set.
    pub fn center_focused_column(&self) -> Option<String> {
        self.setting(&LAYOUT, "center-focused-column")
    }

    /// Set `layout { center-focused-column "<v>" }`.
    pub fn set_center_focused_column(&mut self, value: &str) -> Result<(), StoreError> {
        self.set_setting(&LAYOUT, "center-focused-column", &quoted(value))
    }

    /// Set `backdrop-color "#…"` on the named output (its block is created
    /// when the config has no active one).
    pub fn set_output_backdrop(&mut self, output: &str, color: &str) -> Result<(), StoreError> {
        self.set_setting(
            &[Block::with_arg("output", output)],
            "backdrop-color",
            &quoted(color),
        )
    }

    /// Set `position x=… y=…` on the named output (its block is created when
    /// the config has no active one, e.g. only niri's `/-output` example).
    pub fn set_output_position(&mut self, output: &str, x: i64, y: i64) -> Result<(), StoreError> {
        self.set_setting(
            &[Block::with_arg("output", output)],
            "position",
            &format!("x={x} y={y}"),
        )
    }

    pub fn touchpad_tap(&self) -> bool {
        self.is_set(&TOUCHPAD, "tap")
    }
    pub fn set_touchpad_tap(&mut self, on: bool) -> Result<(), StoreError> {
        self.set_flag_setting(&TOUCHPAD, "tap", on)
    }

    pub fn touchpad_natural_scroll(&self) -> bool {
        self.is_set(&TOUCHPAD, "natural-scroll")
    }
    pub fn set_touchpad_natural_scroll(&mut self, on: bool) -> Result<(), StoreError> {
        self.set_flag_setting(&TOUCHPAD, "natural-scroll", on)
    }

    pub fn keyboard_numlock(&self) -> bool {
        self.is_set(&KEYBOARD, "numlock")
    }
    pub fn set_keyboard_numlock(&mut self, on: bool) -> Result<(), StoreError> {
        self.set_flag_setting(&KEYBOARD, "numlock", on)
    }

    /// Current xkb `layout` value, if set.
    pub fn keyboard_layout(&self) -> Option<String> {
        self.setting(&XKB, "layout")
    }

    /// Set `input.keyboard.xkb { layout "<v>" }`.
    pub fn set_keyboard_layout(&mut self, layout: &str) -> Result<(), StoreError> {
        self.set_setting(&XKB, "layout", &quoted(layout))
    }

    /// Stage `content` as the whole text of the loaded file at `path`:
    /// undoing a change writes back exactly what was there.
    pub fn stage_file(&mut self, path: &Path, content: &str) -> Result<(), StoreError> {
        let target = self
            .target_for_path(path)
            .ok_or_else(|| StoreError::NotLoaded {
                path: path.to_path_buf(),
            })?;
        self.stage_content(target, content)
    }

    /// Every loaded file with the text it had when loaded or last saved.
    pub fn saved_texts(&self) -> impl Iterator<Item = (&Path, &str)> {
        self.targets()
            .map(|target| (self.target_path(target), self.original(target)))
    }

    /// The text `target` had when loaded or last saved.
    fn original(&self, target: Target) -> &str {
        match target {
            Target::Main => &self.original_main,
            Target::Included(i) => &self.included[i].original,
        }
    }

    /// Whether `target` holds edits not saved yet. Nothing is copied: a
    /// file is compared as it is after its last refresh.
    fn is_pending(&self, target: Target) -> bool {
        *self.doc(target).text() != *self.original(target)
    }

    /// How many files hold edits not saved yet.
    pub fn pending_count(&self) -> usize {
        self.targets()
            .filter(|&target| self.is_pending(target))
            .count()
    }

    /// Whether any file holds edits not saved yet.
    pub fn has_pending(&self) -> bool {
        self.targets().any(|target| self.is_pending(target))
    }

    /// Drop the edits not saved yet: every file back to its saved text.
    pub fn revert_pending(&mut self) {
        let pending: Vec<Target> = self.targets().filter(|&t| self.is_pending(t)).collect();
        for target in pending {
            let saved = self.original(target).to_string();
            // the saved text parsed once already, so it parses again
            let _ = self.stage_content(target, &saved);
        }
    }

    /// This store as it was saved last, without the edits made since.
    pub fn saved_version(&self) -> Self {
        let mut saved = self.clone();
        saved.revert_pending();
        saved
    }

    /// The text of every loaded file as it would be saved, beside the text
    /// it has on disk: what the disk writes.
    pub fn job(&self) -> SaveJob {
        SaveJob {
            revision: self.revision,
            main: self.path.clone(),
            files: self
                .targets()
                .map(|target| FileChange {
                    path: self.target_path(target).to_path_buf(),
                    before: self.original(target).to_string(),
                    after: self.doc(target).text().into_owned(),
                })
                .collect(),
        }
    }

    /// `job` was written: its texts are what the files hold now. Edits made
    /// since stay unsaved, and the revision does not move.
    pub fn saved(&mut self, job: &SaveJob) {
        for file in job.changed() {
            if file.path == self.path {
                self.original_main.clone_from(&file.after);
            } else if let Some(included) = self.included.iter_mut().find(|f| f.path == file.path) {
                included.original.clone_from(&file.after);
            }
        }
    }

    /// Keep `path`'s `text` for the app to include later.
    pub fn keep_spare(&mut self, path: PathBuf, text: String) {
        self.spare.push((path, text));
    }

    /// Stage `content` as the whole text of the loaded file named
    /// `file_name`: a backup put back.
    pub fn stage_named(&mut self, file_name: &str, content: &str) -> Result<(), StoreError> {
        let target = self
            .target_named(file_name)
            .ok_or_else(|| StoreError::NotLoaded {
                path: PathBuf::from(file_name),
            })?;
        self.stage_content(target, content)
    }

    /// Every loaded file by name, with the text it has now (edits not
    /// saved yet included): what a snapshot keeps.
    pub fn snapshot_files(&self) -> Vec<(String, String)> {
        self.targets()
            .map(|target| {
                let name = short_name(self.target_path(target)).unwrap_or("config.kdl");
                (name.to_string(), self.doc(target).text().into_owned())
            })
            .collect()
    }

    /// Stage a snapshot's `files` over the loaded files they match. A
    /// snapshot may hold a file the config no longer loads.
    pub fn stage_snapshot(&mut self, files: &[(String, String)]) -> Result<(), StoreError> {
        let mut staged = 0;
        for (name, content) in files {
            if let Some(target) = self.target_named(name) {
                self.stage_content(target, content)?;
                staged += 1;
            }
        }
        if staged == 0 {
            return Err(StoreError::NoSnapshotFiles);
        }
        Ok(())
    }

    /// The loaded file whose file name is `file_name`.
    fn target_named(&self, file_name: &str) -> Option<Target> {
        self.targets()
            .find(|&target| short_name(self.target_path(target)) == Some(file_name))
    }

    /// Replace a file's whole in-memory content (one splice over all of it).
    fn stage_content(&mut self, target: Target, content: &str) -> Result<(), StoreError> {
        let end = self.doc(target).source.len();
        self.doc_mut(target).splice(Span::new(0, end), content);
        self.refresh(target)
    }

    // ---- animation curves and springs ----

    /// Names of the animations configured in active `animations` blocks.
    pub fn animation_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for addr in self.reading_order() {
            let node = self.node(addr);
            if node.name != "animations" {
                continue;
            }
            for child in node.children.iter().filter(|c| !c.disabled) {
                if !names.contains(&child.name) {
                    names.push(child.name.clone());
                }
            }
        }
        names
    }

    /// Current `curve` of an easing animation, if set.
    pub fn easing_curve(&self, animation: &str) -> Option<String> {
        self.setting(&animation_chain(animation), "curve")
    }

    /// Set `curve "<name>"` on an animation.
    ///
    /// An animation is either a spring or an easing (niri rejects both), so
    /// this drops the `spring` first.
    pub fn set_easing_curve(&mut self, animation: &str, name: &str) -> Result<(), StoreError> {
        let chain = animation_chain(animation);
        self.unset_setting(&chain, "spring")?;
        self.set_setting(&chain, "curve", &quoted(name))
    }

    /// The four control points of a `curve "cubic-bezier" x1 y1 x2 y2`.
    pub fn bezier_points(&self, animation: &str) -> Option<[f64; 4]> {
        let node = self.setting_node(&animation_chain(animation), "curve")?;
        let points: Vec<f64> = node
            .args
            .iter()
            .skip(1)
            .filter_map(|value| value.kind.as_str().parse().ok())
            .collect();
        points.try_into().ok()
    }

    /// `duration-ms` of an easing animation, if set.
    pub fn duration_ms(&self, animation: &str) -> Option<u32> {
        self.setting(&animation_chain(animation), "duration-ms")?
            .parse()
            .ok()
    }

    /// Make an animation an easing: `curve "<name>"` (with its four points
    /// for `cubic-bezier`) and, when given, `duration-ms`. A spring goes,
    /// since niri rejects both.
    pub fn set_easing(
        &mut self,
        animation: &str,
        curve: &str,
        points: Option<[f64; 4]>,
        duration_ms: Option<u32>,
    ) -> Result<(), StoreError> {
        let chain = animation_chain(animation);
        self.unset_setting(&chain, "spring")?;
        let mut args = quoted(curve);
        if let Some(points) = points {
            for point in points {
                args.push(' ');
                args.push_str(&float_text(point));
            }
        }
        if let Some(duration) = duration_ms {
            self.set_setting(&chain, "duration-ms", &duration.to_string())?;
        }
        self.set_setting(&chain, "curve", &args)
    }

    /// Whether an animation is turned off with its own `off` flag.
    pub fn animation_off(&self, animation: &str) -> bool {
        self.is_set(&animation_chain(animation), "off")
    }

    pub fn set_animation_off(&mut self, animation: &str, off: bool) -> Result<(), StoreError> {
        self.set_flag_setting(&animation_chain(animation), "off", off)
    }

    /// Whether an animation has a custom shader, which the app leaves alone.
    pub fn has_custom_shader(&self, animation: &str) -> bool {
        self.is_set(&animation_chain(animation), "custom-shader")
    }

    /// Remove an animation's block, so niri's default applies again.
    pub fn reset_animation(&mut self, animation: &str) -> Result<(), StoreError> {
        self.unset_setting(&[Block::named("animations")], animation)
    }

    /// Current spring parameters of a spring animation, if set.
    pub fn spring_params(&self, animation: &str) -> Option<(f64, f64, f64)> {
        let spring = self.setting_node(&animation_chain(animation), "spring")?;
        let prop = |name: &str| {
            spring
                .props
                .iter()
                .find(|p| p.name == name)
                .and_then(|p| p.value.kind.as_str().parse().ok())
        };
        Some((prop("damping-ratio")?, prop("stiffness")?, prop("epsilon")?))
    }

    /// Set `spring damping-ratio=… stiffness=… epsilon=…` on an animation;
    /// a spring replaces the easing (niri rejects both).
    pub fn set_spring_params(
        &mut self,
        animation: &str,
        damping_ratio: f64,
        stiffness: f64,
        epsilon: f64,
    ) -> Result<(), StoreError> {
        let chain = animation_chain(animation);
        self.unset_setting(&chain, "curve")?;
        self.unset_setting(&chain, "duration-ms")?;
        let props = format!(
            "damping-ratio={} stiffness={} epsilon={}",
            float_text(damping_ratio),
            integer_text(stiffness),
            float_text(epsilon)
        );
        self.set_setting(&chain, "spring", &props)
    }

    // ---- what is not saved yet ----

    /// Every file whose text differs from its saved text: that text, and
    /// the text it has now.
    pub fn pending_files(&self) -> Vec<FileChange> {
        self.targets()
            .filter(|&target| self.is_pending(target))
            .map(|target| FileChange {
                path: self.target_path(target).to_path_buf(),
                before: self.original(target).to_string(),
                after: self.doc(target).text().into_owned(),
            })
            .collect()
    }

    /// The diff of every pending file.
    pub fn pending_diffs(&self) -> Vec<crate::diff::FileDiff> {
        self.pending_files()
            .into_iter()
            .map(|file| crate::diff::FileDiff {
                path: self.file_label(&file.path),
                hunks: crate::diff::diff(&file.before, &file.after),
            })
            .collect()
    }
}

/// A `window-rule` as the window-rules page lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowRuleRow {
    /// `match` / `exclude` lines as written.
    pub criteria: Vec<String>,
    /// The same lines read: which windows the rule reaches.
    pub matchers: Vec<Matcher>,
    /// 1-based line of `window-rule` in its file.
    pub line: usize,
    /// Every other line of the rule as written (`open-floating true`).
    pub properties: Vec<String>,
    /// The file it is in, as [`Store::file_label`] names it.
    pub file: String,
    /// It is in an included file, not the main config.
    pub included: bool,
    pub disabled: bool,
    /// The block holds its lines and nothing else: no comments, no `/-`
    /// lines. Only such a rule can be written anew without losing a word.
    pub plain: bool,
}

/// What a rule says, however it is written: its `match` and `exclude`
/// lines with their values decoded, and its other lines, each with its
/// whitespace squeezed, in any order. Two rules that say the same are one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuleMeaning {
    /// Each line: whether it excludes, and its (name, value) pairs.
    lines: Vec<(bool, Vec<(MatchName, String)>)>,
    properties: std::collections::BTreeSet<String>,
}

impl WindowRuleRow {
    pub fn meaning(&self) -> RuleMeaning {
        RuleMeaning {
            lines: self
                .matchers
                .iter()
                .map(|matcher| {
                    let props = matcher
                        .props
                        .iter()
                        .map(|prop| (prop.name.clone(), prop.value.clone()))
                        .collect();
                    (matcher.exclude, props)
                })
                .collect(),
            properties: self.properties.iter().map(|line| squeezed(line)).collect(),
        }
    }
}

/// Whether `node`'s block holds its child lines and nothing else.
///
/// Its text is compared with the text its parsed parts make: anything the
/// parser stepped over, a comment or a `/-` marker, anywhere inside, makes
/// the two differ. Whitespace and `;` separators do not count.
fn holds_only_its_lines(doc: &Document, node: &Node) -> bool {
    let mut parts = String::new();
    written_parts(doc, node, &mut parts);
    let significant = |text: &str| -> String {
        text.chars()
            .filter(|c| !c.is_whitespace() && *c != ';')
            .collect()
    };
    significant(node.raw(doc)) == significant(&parts)
}

/// A node's tokens as the parser read them, children in braces.
fn written_parts(doc: &Document, node: &Node, out: &mut String) {
    out.push_str(&node.name);
    for value in &node.args {
        out.push(' ');
        out.push_str(value.raw(doc));
    }
    for prop in &node.props {
        out.push_str(&format!(" {}={}", prop.name, prop.value.raw(doc)));
    }
    if node.block.is_some() {
        out.push('{');
        // a `/-` marker is not among them: such a line would come back
        // on in a rule written anew
        for child in &node.children {
            written_parts(doc, child, out);
            out.push(';');
        }
        out.push('}');
    }
}

/// One `match` or `exclude` line: its properties, values decoded
/// (`app-id`, `^firefox$`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matcher {
    pub exclude: bool,
    pub props: Vec<MatchProp>,
}

/// What a property of a `match` or `exclude` line looks at.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MatchName {
    AppId,
    Title,
    IsActive,
    IsFocused,
    IsActiveInColumn,
    IsFloating,
    IsUrgent,
    IsWindowCastTarget,
    AtStartup,
    /// One the app does not read, kept by niri's name.
    Other(String),
}

impl MatchName {
    const KNOWN: [Self; 9] = [
        Self::AppId,
        Self::Title,
        Self::IsActive,
        Self::IsFocused,
        Self::IsActiveInColumn,
        Self::IsFloating,
        Self::IsUrgent,
        Self::IsWindowCastTarget,
        Self::AtStartup,
    ];

    pub fn read(name: &str) -> Self {
        Self::KNOWN
            .into_iter()
            .find(|known| known.as_str() == name)
            .unwrap_or_else(|| Self::Other(name.to_string()))
    }

    /// niri's name for it.
    pub fn as_str(&self) -> &str {
        match self {
            Self::AppId => "app-id",
            Self::Title => "title",
            Self::IsActive => "is-active",
            Self::IsFocused => "is-focused",
            Self::IsActiveInColumn => "is-active-in-column",
            Self::IsFloating => "is-floating",
            Self::IsUrgent => "is-urgent",
            Self::IsWindowCastTarget => "is-window-cast-target",
            Self::AtStartup => "at-startup",
            Self::Other(name) => name,
        }
    }

    /// Its value is one of niri's regular expressions.
    pub const fn is_pattern(&self) -> bool {
        matches!(self, Self::AppId | Self::Title)
    }
}

/// One property of a `match` or `exclude` line. An `app-id` or `title`
/// value is one of niri's regular expressions, compiled as the line is
/// read.
#[derive(Debug, Clone)]
pub struct MatchProp {
    pub name: MatchName,
    pub value: String,
    /// The compiled `app-id` or `title` value. `None` for other properties,
    /// and for a value that is no regular expression.
    pub pattern: Option<regex::Regex>,
}

impl MatchProp {
    pub fn new(name: MatchName, value: impl Into<String>) -> Self {
        let value = value.into();
        let pattern = name
            .is_pattern()
            .then(|| regex::Regex::new(&value).ok())
            .flatten();
        Self {
            name,
            value,
            pattern,
        }
    }
}

impl PartialEq for MatchProp {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.value == other.value
    }
}

impl Eq for MatchProp {}

/// A line the rule form offers. Each one is checked against
/// `niri validate` by the test suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuleProperty {
    OpenFloating,
    OpenMaximized,
    OpenFullscreen,
    OpenUnfocused,
    OpenHalfWidth,
    HideFromScreencasts,
    Transparent,
    Blur,
    RoundedCorners,
    ClipToGeometry,
    BorderWithoutBackground,
}

impl RuleProperty {
    pub const ALL: [Self; 11] = [
        Self::OpenFloating,
        Self::OpenMaximized,
        Self::OpenFullscreen,
        Self::OpenUnfocused,
        Self::OpenHalfWidth,
        Self::HideFromScreencasts,
        Self::Transparent,
        Self::Blur,
        Self::RoundedCorners,
        Self::ClipToGeometry,
        Self::BorderWithoutBackground,
    ];

    /// What it does.
    pub const fn label(self) -> &'static str {
        match self {
            Self::OpenFloating => "Open floating",
            Self::OpenMaximized => "Open maximized",
            Self::OpenFullscreen => "Open fullscreen",
            Self::OpenUnfocused => "Open without taking focus",
            Self::OpenHalfWidth => "Open as a half-width column",
            Self::HideFromScreencasts => "Hide from screencasts",
            Self::Transparent => "Slightly transparent",
            Self::Blur => "Blur what is behind it",
            Self::RoundedCorners => "Rounded corners",
            Self::ClipToGeometry => "Cut the window to its corners",
            Self::BorderWithoutBackground => "Ring and border only around it, not behind it",
        }
    }

    /// The line written into the rule.
    pub const fn kdl(self) -> &'static str {
        match self {
            Self::OpenFloating => "open-floating true",
            Self::OpenMaximized => "open-maximized true",
            Self::OpenFullscreen => "open-fullscreen true",
            Self::OpenUnfocused => "open-focused false",
            Self::OpenHalfWidth => "default-column-width { proportion 0.5; }",
            Self::HideFromScreencasts => "block-out-from \"screencast\"",
            Self::Transparent => "opacity 0.92",
            Self::Blur => "background-effect { blur true; }",
            Self::RoundedCorners => "geometry-corner-radius 12",
            Self::ClipToGeometry => "clip-to-geometry true",
            Self::BorderWithoutBackground => "draw-border-with-background false",
        }
    }

    /// It applies as the window opens, not for as long as the rule matches.
    pub const fn at_open(self) -> bool {
        matches!(
            self,
            Self::OpenFloating
                | Self::OpenMaximized
                | Self::OpenFullscreen
                | Self::OpenUnfocused
                | Self::OpenHalfWidth
        )
    }

    /// Which one a written rule line is, however it is indented.
    pub fn read(line: &str) -> Option<Self> {
        let line = squeezed(line);
        Self::ALL
            .into_iter()
            .find(|property| squeezed(property.kdl()) == line)
    }
}

/// How a rule's text matches a window's app ID or title.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextMatch {
    /// The whole of it, character for character.
    #[default]
    Exact,
    /// Anywhere in it.
    Contains,
    /// The text is a regular expression, written as given.
    Pattern,
}

impl TextMatch {
    /// The pattern niri gets for `text`: escaped unless it is one already.
    pub fn pattern(self, text: &str) -> String {
        match self {
            Self::Exact => format!("^{}$", escaped(text)),
            Self::Contains => escaped(text),
            Self::Pattern => text.to_string(),
        }
    }

    /// How a written pattern reads, and its text: the way back from
    /// [`Self::pattern`]. Anything else is a [`Self::Pattern`].
    pub fn read(pattern: &str) -> (Self, String) {
        if let Some(inner) = pattern
            .strip_prefix('^')
            .and_then(|rest| rest.strip_suffix('$'))
            && let Some(text) = unescaped(inner)
        {
            return (Self::Exact, text);
        }
        match unescaped(pattern) {
            Some(text) => (Self::Contains, text),
            None => (Self::Pattern, pattern.to_string()),
        }
    }
}

/// A rule for [`Store::add_window_rule`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewWindowRule {
    pub app_id: Option<String>,
    pub app_id_how: TextMatch,
    pub title: Option<String>,
    pub title_how: TextMatch,
    /// Yes-or-no conditions: `is-floating=true`.
    pub flags: Vec<(MatchName, bool)>,
    pub properties: Vec<RuleProperty>,
}

impl NewWindowRule {
    /// A written rule as one of these, when writing it anew loses nothing:
    /// it is on and plain, has at most one `match` line (no `exclude`)
    /// that asks only for an app ID, a title and yes-or-no states, and its
    /// other lines are all [`RuleProperty`] lines. The way back from
    /// [`Self::to_kdl`].
    pub fn from_row(row: &WindowRuleRow) -> Option<Self> {
        if row.disabled || !row.plain {
            return None;
        }
        let mut rule = Self::default();
        match row.matchers.as_slice() {
            [] => {}
            [matcher] if !matcher.exclude => {
                for MatchProp { name, value, .. } in &matcher.props {
                    match (name, value.as_str()) {
                        (MatchName::AppId, _) if rule.app_id.is_none() => {
                            let (how, text) = TextMatch::read(value);
                            (rule.app_id_how, rule.app_id) = (how, Some(text));
                        }
                        (MatchName::Title, _) if rule.title.is_none() => {
                            let (how, text) = TextMatch::read(value);
                            (rule.title_how, rule.title) = (how, Some(text));
                        }
                        (MatchName::AppId | MatchName::Title, _) => return None,
                        (_, "true" | "false") => rule.flags.push((name.clone(), value == "true")),
                        _ => return None,
                    }
                }
            }
            _ => return None,
        }
        for line in &row.properties {
            rule.properties.push(RuleProperty::read(line)?);
        }
        Some(rule)
    }

    /// The rule's `match` line, read: which windows it reaches.
    pub fn matcher(&self) -> Matcher {
        let mut props = Vec::new();
        if let Some(app_id) = &self.app_id {
            props.push(MatchProp::new(
                MatchName::AppId,
                self.app_id_how.pattern(app_id),
            ));
        }
        if let Some(title) = &self.title {
            props.push(MatchProp::new(
                MatchName::Title,
                self.title_how.pattern(title),
            ));
        }
        for (name, yes) in &self.flags {
            props.push(MatchProp::new(name.clone(), yes.to_string()));
        }
        Matcher {
            exclude: false,
            props,
        }
    }

    /// The rule as it will be written: an exact `match`, then the properties.
    pub fn to_kdl(&self) -> String {
        let criteria: Vec<String> = self
            .matcher()
            .props
            .iter()
            .map(|MatchProp { name, value, .. }| {
                if name.is_pattern() {
                    format!("{}={}", name.as_str(), raw_string(value))
                } else {
                    format!("{}={value}", name.as_str())
                }
            })
            .collect();
        let mut lines = vec!["window-rule {".to_string()];
        if !criteria.is_empty() {
            lines.push(format!("    match {}", criteria.join(" ")));
        }
        lines.extend(
            self.properties
                .iter()
                .map(|property| format!("    {}", property.kdl())),
        );
        lines.push("}".to_string());
        lines.join("\n")
    }
}

/// `text` with every run of whitespace as one space: KDL that reads the
/// same however it is indented compares equal.
fn squeezed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The characters a regex reads as more than themselves.
const REGEX_METACHARACTERS: &str = "\\.+*?()|[]{}^$";

/// `text` with regex metacharacters escaped, so a pattern finds it as is.
fn escaped(text: &str) -> String {
    let mut pattern = String::new();
    for c in text.chars() {
        if REGEX_METACHARACTERS.contains(c) {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern
}

/// The text a pattern finds character for character, when it is only
/// escaped text; `None` when it uses regex syntax.
fn unescaped(pattern: &str) -> Option<String> {
    let mut text = String::new();
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let next = chars
                .next()
                .filter(|next| REGEX_METACHARACTERS.contains(*next))?;
            text.push(next);
        } else if REGEX_METACHARACTERS.contains(c) {
            return None;
        } else {
            text.push(c);
        }
    }
    Some(text)
}

/// `pattern` as a KDL raw string, which keeps its backslashes as they are.
fn raw_string(pattern: &str) -> String {
    // enough `#`s that the content cannot close the raw string early
    let mut hashes = String::from("#");
    while pattern.contains(&format!("\"{hashes}")) {
        hashes.push('#');
    }
    format!("r{hashes}\"{pattern}\"{hashes}")
}

/// A binding taken out by [`Store::remove_binding`], kept so
/// [`Store::restore_binding`] can put it back byte for byte.
#[derive(Debug, Clone)]
pub struct RemovedBinding {
    target: Target,
    /// Position among the bindings of its file (active `binds` blocks).
    position: usize,
    text: String,
    /// `text` is a whole line (indent through line break) rather than just
    /// the node, which shared its line with other nodes.
    whole_line: bool,
}

/// What to cut when removing the node at `node_span`: its whole line
/// (indent, a `/-` marker, a trailing `//` comment, the line break) when
/// nothing else lives on that line; otherwise the node alone.
fn removal_span(source: &str, node_span: Span) -> (Span, bool) {
    let start = line_start(source, node_span.start);
    let before = source[start..node_span.start].trim_start();
    let before_is_marker = before.is_empty() || before.trim_end() == "/-";
    let end = line_end(source, node_span.end);
    let after = source[node_span.end..end].trim();
    let after_is_comment = after.is_empty() || after.starts_with("//");
    if before_is_marker && after_is_comment {
        (Span::new(start, end), true)
    } else {
        // a node sharing its line: take its `;` terminator along, and the
        // spaces after it
        let end = after_terminator(source, node_span.end).map_or(node_span.end, |end| {
            end + source[end..].len() - source[end..].trim_start_matches([' ', '\t']).len()
        });
        (Span::new(node_span.start, end), false)
    }
}

/// Just past the `;` that ends the node ending at `pos`, spaces before it
/// skipped; `None` when no `;` follows.
fn after_terminator(source: &str, pos: usize) -> Option<usize> {
    let rest = &source[pos..];
    let spaces = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    rest[spaces..].starts_with(';').then_some(pos + spaces + 1)
}

/// The start of the next line after `pos`: past its `\n`, or the end.
fn line_end(source: &str, pos: usize) -> usize {
    source[pos..]
        .find('\n')
        .map_or(source.len(), |offset| pos + offset + 1)
}

/// Location info for editing one binding (key + action spans, block presence).
struct BindingMeta {
    target: Target,
    node_span: Span,
    name_span: Span,
    action_span: Option<Span>,
}

/// Add `text` at the end of `doc`, a blank line before it.
fn append_block(doc: &mut Document, text: &str) {
    let end = doc.source.len();
    let lead = if doc.source.is_empty() || doc.source.ends_with("\n\n") {
        ""
    } else if doc.source.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    doc.splice(Span::new(end, end), &format!("{lead}{text}\n"));
}

/// Start of the line containing byte `pos` (just after the previous `\n`).
/// Uses the newline scan so it stays correct even when a `/ -` disable marker
/// sits between the indentation and the node name.
fn line_start(source: &str, pos: usize) -> usize {
    source[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0)
}

/// A float-typed config value: niri rejects `damping-ratio=1`, wants `1.0`.
fn float_text(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}

/// An integer-typed config value (`stiffness=800`).
fn integer_text(value: f64) -> String {
    format!("{}", value.round() as i64)
}

/// True when the document has an active top-level node named `name`.
fn root_has(name: &str) -> impl Fn(&Document) -> bool {
    move |doc: &Document| active_named(&doc.roots, name).next().is_some()
}

/// Insert `key { body; }` into `doc`'s first binds block (lossless splice).
/// A program niri starts with the session.
fn is_autostart(node: &Node) -> bool {
    matches!(
        node.name.as_str(),
        "spawn-at-startup" | "spawn-sh-at-startup"
    )
}

/// A binding as written: `Mod+T { spawn "kitty"; }`.
fn binding_line(key: &str, body: &str) -> String {
    let body = body.trim_end().trim_end_matches(';');
    format!("{key} {{ {body}; }}")
}

/// Display text for a binds child node: `spawn-sh "kitty"` / `quit`.
fn node_display(doc: &Document, node: &Node) -> String {
    if let Some(inner) = node.children.first() {
        let args: Vec<String> = inner.args.iter().map(|v| doc.value_text(v)).collect();
        if args.is_empty() {
            inner.name.clone()
        } else {
            format!("{} {}", inner.name, args.join(" "))
        }
    } else if node.args.is_empty() {
        node.name.clone()
    } else {
        let args: Vec<String> = node.args.iter().map(|v| doc.value_text(v)).collect();
        format!("{} {}", node.name, args.join(" "))
    }
}

/// `path` with the home folder written as `~`, the way niri configs do.
pub fn tilde(path: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME")
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return format!("~/{}", rest.display());
    }
    path.display().to_string()
}

/// A file's name alone: `binds.kdl`.
fn short_name(path: &Path) -> Option<&str> {
    path.file_name().and_then(|name| name.to_str())
}
