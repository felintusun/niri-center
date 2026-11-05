//! Line-oriented diff, for showing what a change did.
//!
//! A minimal LCS-based line diff with context, per file: the History page
//! shows each change this way, and the source column marks the lines the
//! latest change wrote.

/// One line of a hunk. `Context` lines are unchanged neighbors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Context(String),
    Removed(String),
    Added(String),
}

impl Line {
    /// Unified-diff style prefix for rendering.
    pub const fn prefix(&self) -> &'static str {
        match self {
            Self::Context(_) => " ",
            Self::Removed(_) => "-",
            Self::Added(_) => "+",
        }
    }
    pub fn text(&self) -> &str {
        match self {
            Self::Context(s) | Self::Removed(s) | Self::Added(s) => s,
        }
    }
}

/// A change group with context: starts are 1-based line numbers in the
/// respective files.
#[derive(Debug, Clone)]
pub struct Hunk {
    pub old_start: usize,
    pub new_start: usize,
    pub lines: Vec<Line>,
}

impl Hunk {
    /// Each line with its number in the old file and in the new one; a
    /// line only one side has has only that side's.
    pub fn numbered(&self) -> impl Iterator<Item = (Option<usize>, Option<usize>, &Line)> {
        let (mut old, mut new) = (self.old_start, self.new_start);
        self.lines.iter().map(move |line| {
            let numbers = match line {
                Line::Context(_) => (Some(old), Some(new)),
                Line::Removed(_) => (Some(old), None),
                Line::Added(_) => (None, Some(new)),
            };
            old += usize::from(numbers.0.is_some());
            new += usize::from(numbers.1.is_some());
            (numbers.0, numbers.1, line)
        })
    }
}

/// A per-file diff: only files with changes appear.
#[derive(Debug, Clone)]
pub struct FileDiff {
    pub path: String,
    pub hunks: Vec<Hunk>,
}

/// Diff `old` vs `new` line-wise; empty vec when identical.
pub fn diff(old: &str, new: &str) -> Vec<Hunk> {
    const CONTEXT: usize = 3;
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let mut ops = lcs_ops(&old_lines, &new_lines, |old, new| old == new);
    whole_blocks(&mut ops);

    // each change, by its place in the script, with the kept lines between
    // changes that lie close together: their contexts would overlap, so
    // they share a hunk
    let changed: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, op)| !op.is_keep())
        .map(|(place, _)| place)
        .collect();
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for place in changed {
        match groups.last_mut() {
            Some((_, last)) if place - *last <= 2 * CONTEXT + 1 => *last = place,
            Some(_) | None => groups.push((place, place)),
        }
    }
    let mut hunks = Vec::new();
    // line numbers before op `counted`, carried along instead of recounted
    let (mut counted, mut old_line, mut new_line) = (0, 1, 1);
    for (first, last) in groups {
        let start = first.saturating_sub(CONTEXT);
        let end = (last + 1 + CONTEXT).min(ops.len());
        for op in ops.get(counted..start).unwrap_or_default() {
            old_line += usize::from(!matches!(op, Op::Ins(_)));
            new_line += usize::from(!matches!(op, Op::Del(_)));
        }
        counted = start;
        let lines = ops
            .get(start..end)
            .unwrap_or_default()
            .iter()
            .map(|op| match op {
                Op::Keep(old, _) => Line::Context((*old).to_string()),
                Op::Del(old) => Line::Removed((*old).to_string()),
                Op::Ins(new) => Line::Added((*new).to_string()),
            })
            .collect();
        hunks.push(Hunk {
            old_start: old_line,
            new_start: new_line,
            lines,
        });
    }
    hunks
}

/// Which side of the diff a run of changed lines is on.
#[derive(Clone, Copy)]
enum Side {
    Old,
    New,
}

impl Side {
    /// The item `op` stands for on this side, when it has one there.
    fn item<T: Copy>(self, op: &Op<T, T>) -> Option<T> {
        match (self, *op) {
            (Self::Old, Op::Keep(item, _) | Op::Del(item))
            | (Self::New, Op::Keep(_, item) | Op::Ins(item)) => Some(item),
            (Self::Old, Op::Ins(_)) | (Self::New, Op::Del(_)) => None,
        }
    }

    /// Whether `op` is a line only this side has.
    const fn changed<A, B>(self, op: &Op<A, B>) -> bool {
        matches!(
            (self, op),
            (Self::Old, Op::Del(_)) | (Self::New, Op::Ins(_))
        )
    }
}

/// Whether added or removed lines open every block they close, as a KDL
/// block written or taken out whole does: `}` first and `x {` last is the
/// same text read one line off.
fn whole<'view>(lines: impl Iterator<Item = &'view str>) -> bool {
    let mut depth = 0_i32;
    for line in lines.map(str::trim) {
        if line.starts_with('}') {
            depth -= 1;
            if depth < 0 {
                return false;
            }
        }
        if line.ends_with('{') {
            depth += 1;
        }
    }
    depth == 0
}

/// A line both sides keep can often be matched to an equal line further
/// along a run of added or removed lines instead. Match it there when the
/// runs then hold whole blocks: `+ }  + env {  + y` before a kept `}`
/// reads as `+ env {  + y  + }` after it. The edit script stays as short.
fn whole_blocks<'line>(ops: &mut [Op<&'line str, &'line str>]) {
    for side in [Side::Old, Side::New] {
        // a line only the other side has reads as nothing here
        let text =
            |op: &Op<&'line str, &'line str>| -> &'line str { side.item(op).unwrap_or_default() };
        let run = |ops: &[Op<&'line str, &'line str>], from: usize, to: usize| {
            whole(ops.get(from..to).unwrap_or_default().iter().map(text))
        };
        for kept in 0..ops.len() {
            let Some(&Op::Keep(old, new)) = ops.get(kept) else {
                continue;
            };
            let line = text(&Op::Keep(old, new));
            let changed_before = ops
                .get(..kept)
                .unwrap_or_default()
                .iter()
                .rev()
                .take_while(|op| !op.is_keep())
                .count();
            let changed_after = ops
                .get(kept + 1..)
                .unwrap_or_default()
                .iter()
                .take_while(|op| !op.is_keep())
                .count();
            let (start, end) = (kept - changed_before, kept + 1 + changed_after);
            // the run before the kept line, then the one after it
            for (from, to) in [(start, kept), (kept + 1, end)] {
                if run(ops, from, to) {
                    continue;
                }
                let (low, high) = (from.min(kept), to.max(kept + 1));
                let Some(other) = (from..to).find(|&other| {
                    ops.get(other)
                        .is_some_and(|op| side.changed(op) && text(op) == line)
                        && run(ops, low, other)
                        && run(ops, other + 1, high)
                }) else {
                    continue;
                };
                let Some(moved) = ops.get(other).and_then(|op| side.item(op)) else {
                    continue;
                };
                let (keep, let_go) = match side {
                    Side::Old => (Op::Keep(moved, new), Op::Del(old)),
                    Side::New => (Op::Keep(old, moved), Op::Ins(new)),
                };
                // the other side's lines between the two places stay on the
                // same side of the kept line as before
                let between = |from: usize, to: usize, this: bool| {
                    ops.get(from..to)
                        .unwrap_or_default()
                        .iter()
                        .filter(move |op| side.changed(op) == this)
                        .copied()
                        .collect::<Vec<_>>()
                };
                let region: Vec<Op<&'line str, &'line str>> = if other < kept {
                    let mut region = ops.get(low..other).unwrap_or_default().to_vec();
                    region.extend(between(other + 1, kept, false));
                    region.push(keep);
                    region.extend(between(other + 1, kept, true));
                    region.push(let_go);
                    region
                } else {
                    let mut region = vec![let_go];
                    region.extend(between(kept + 1, other, true));
                    region.push(keep);
                    region.extend(between(kept + 1, other, false));
                    region.extend_from_slice(ops.get(other + 1..high).unwrap_or_default());
                    region
                };
                if let Some(slots) = ops.get_mut(low..high) {
                    slots.copy_from_slice(&region);
                }
                break;
            }
        }
    }
}

/// One step of an edit script from `a` to `b`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Op<A, B> {
    /// The same item in both: as `a` has it, then as `b` has it.
    Keep(A, B),
    /// An item only `a` has.
    Del(A),
    /// An item only `b` has.
    Ins(B),
}

impl<A, B> Op<A, B> {
    pub(crate) const fn is_keep(&self) -> bool {
        matches!(self, Self::Keep(..))
    }
}

/// The edit script from `a` to `b` by their longest common subsequence
/// (standard DP), items matched by `same`. What both start and end with is
/// kept outright first: a change is mostly one block in a long file, and
/// the table then covers only it.
pub(crate) fn lcs_ops<A: Copy, B: Copy>(
    a: &[A],
    b: &[B],
    same: impl Fn(A, B) -> bool,
) -> Vec<Op<A, B>> {
    let prefix = a
        .iter()
        .copied()
        .zip(b.iter().copied())
        .take_while(|&(old, new)| same(old, new))
        .count();
    let (a_rest, b_rest) = (
        a.get(prefix..).unwrap_or_default(),
        b.get(prefix..).unwrap_or_default(),
    );
    let suffix = a_rest
        .iter()
        .rev()
        .copied()
        .zip(b_rest.iter().rev().copied())
        .take_while(|&(old, new)| same(old, new))
        .count();
    let (a_middle, a_end) = a_rest.split_at(a_rest.len() - suffix);
    let (b_middle, b_end) = b_rest.split_at(b_rest.len() - suffix);
    let mut ops: Vec<Op<A, B>> = a
        .iter()
        .copied()
        .zip(b.iter().copied())
        .take(prefix)
        .map(|(old, new)| Op::Keep(old, new))
        .collect();
    ops.extend(lcs_core(a_middle, b_middle, &same));
    ops.extend(
        a_end
            .iter()
            .copied()
            .zip(b_end.iter().copied())
            .map(|(old, new)| Op::Keep(old, new)),
    );
    ops
}

/// The lengths of the longest common subsequences of every pair of
/// suffixes, in one table a row per item of `a` and one more: a suffix past
/// the end is empty and shares nothing.
struct Table {
    width: usize,
    cells: Vec<usize>,
}

impl Table {
    fn at(&self, a_from: usize, b_from: usize) -> usize {
        self.cells
            .get(a_from * self.width + b_from)
            .copied()
            .unwrap_or(0)
    }

    fn set(&mut self, a_from: usize, b_from: usize, length: usize) {
        if let Some(cell) = self.cells.get_mut(a_from * self.width + b_from) {
            *cell = length;
        }
    }
}

fn lcs_core<A: Copy, B: Copy>(a: &[A], b: &[B], same: impl Fn(A, B) -> bool) -> Vec<Op<A, B>> {
    // filled from the ends: each cell from the ones after it
    let width = b.len() + 1;
    let mut table = Table {
        width,
        cells: vec![0; (a.len() + 1) * width],
    };
    for (a_from, old) in a.iter().copied().enumerate().rev() {
        for (b_from, new) in b.iter().copied().enumerate().rev() {
            let length = if same(old, new) {
                table.at(a_from + 1, b_from + 1) + 1
            } else {
                table
                    .at(a_from + 1, b_from)
                    .max(table.at(a_from, b_from + 1))
            };
            table.set(a_from, b_from, length);
        }
    }

    let mut ops = Vec::with_capacity(a.len() + b.len());
    let (mut a_from, mut b_from) = (0, 0);
    while let (Some(&old), Some(&new)) = (a.get(a_from), b.get(b_from)) {
        if same(old, new) {
            ops.push(Op::Keep(old, new));
            a_from += 1;
            b_from += 1;
        } else if table.at(a_from + 1, b_from) >= table.at(a_from, b_from + 1) {
            ops.push(Op::Del(old));
            a_from += 1;
        } else {
            ops.push(Op::Ins(new));
            b_from += 1;
        }
    }
    ops.extend(
        a.get(a_from..)
            .unwrap_or_default()
            .iter()
            .copied()
            .map(Op::Del),
    );
    ops.extend(
        b.get(b_from..)
            .unwrap_or_default()
            .iter()
            .copied()
            .map(Op::Ins),
    );
    ops
}

/// Unified-diff rendering, as `patch` and `git apply` read it.
pub fn render_unified(file: &FileDiff) -> String {
    file.to_string()
}

impl std::fmt::Display for FileDiff {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(out, "--- {}", self.path)?;
        writeln!(out, "+++ {}", self.path)?;
        for hunk in &self.hunks {
            // each side counts its context lines too, as `patch` reads them
            let (old, new) = hunk.numbered().fold((0, 0), |(old, new), (o, n, _)| {
                (
                    old + usize::from(o.is_some()),
                    new + usize::from(n.is_some()),
                )
            });
            let range = |start: usize, count: usize| match count {
                // an empty side names the line before it
                0 => format!("{},0", start.saturating_sub(1)),
                1 => start.to_string(),
                _ => format!("{start},{count}"),
            };
            writeln!(
                out,
                "@@ -{} +{} @@",
                range(hunk.old_start, old),
                range(hunk.new_start, new)
            )?;
            for line in &hunk.lines {
                writeln!(out, "{}{}", line.prefix(), line.text())?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lines `hunks` mark as `kind`, by text.
    fn marked(hunks: &[Hunk], kind: fn(&Line) -> bool) -> Vec<String> {
        hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .filter(|line| kind(line))
            .map(|line| line.text().to_string())
            .collect()
    }

    #[test]
    fn an_added_block_reads_as_a_whole_block() {
        // a change before it stops the shared start short, and the shared
        // end then claims the block's closing brace
        let old = "A\n}\nZ\n";
        let new = "B\n}\nenv {\n    y\n}\nZ\n";
        let block = ["B", "env {", "    y", "}"];
        let added = marked(&diff(old, new), |line| matches!(line, Line::Added(_)));
        assert_eq!(added, block);
        // and the same taken out again
        let removed = marked(&diff(new, old), |line| matches!(line, Line::Removed(_)));
        assert_eq!(removed, block);
    }

    proptest::proptest! {
        /// Matching kept lines elsewhere still spells out both texts, line
        /// for line, and keeps as many lines.
        #[test]
        fn whole_blocks_keeps_a_valid_script(
            a in proptest::collection::vec(0..5_usize, 0..10),
            b in proptest::collection::vec(0..5_usize, 0..10),
        ) {
            const LINES: [&str; 5] = ["a {", "}", "x", "", "    y"];
            let old_lines: Vec<&str> = a.into_iter().map(|line| LINES[line]).collect();
            let new_lines: Vec<&str> = b.into_iter().map(|line| LINES[line]).collect();
            let mut ops = lcs_ops(&old_lines, &new_lines, |old, new| old == new);
            let kept = |ops: &[Op<&str, &str>]| ops.iter().filter(|op| op.is_keep()).count();
            let before = kept(&ops);
            whole_blocks(&mut ops);
            proptest::prop_assert_eq!(kept(&ops), before);
            // read back, each side spells out its own text in order
            let (mut old_read, mut new_read) = (Vec::new(), Vec::new());
            for op in &ops {
                match *op {
                    Op::Keep(old, new) => {
                        proptest::prop_assert_eq!(old, new);
                        old_read.push(old);
                        new_read.push(new);
                    }
                    Op::Del(old) => old_read.push(old),
                    Op::Ins(new) => new_read.push(new),
                }
            }
            proptest::prop_assert_eq!(old_read, old_lines);
            proptest::prop_assert_eq!(new_read, new_lines);
        }
    }

    #[test]
    fn identical_inputs_no_hunks() {
        assert!(diff("a\nb\nc\n", "a\nb\nc\n").is_empty());
    }

    #[test]
    fn single_line_change() {
        let hunks = diff("a\nb\nc\n", "a\nB\nc\n");
        assert_eq!(hunks.len(), 1);
        // hunk starts at the first *context* line (1-based), includes the change
        assert_eq!(hunks[0].old_start, 1);
        assert_eq!(hunks[0].new_start, 1);
        assert!(hunks[0].lines.contains(&Line::Removed("b".into())));
        assert!(hunks[0].lines.contains(&Line::Added("B".into())));
        // surrounding lines are context
        assert!(hunks[0].lines.contains(&Line::Context("a".into())));
        assert!(hunks[0].lines.contains(&Line::Context("c".into())));
    }

    #[test]
    fn insertion_and_removal() {
        let hunks = diff("a\nb\nc\n", "a\nx\nb\nc\nd\n");
        let no_removals = hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .filter_map(|line| match line {
                Line::Removed(t) => Some(t.as_str()),
                Line::Context(_) | Line::Added(_) => None,
            })
            .next()
            .is_none();
        assert!(no_removals, "no pure removals expected");
        let added: Vec<_> = hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .filter_map(|line| match line {
                Line::Added(t) => Some(t.as_str()),
                Line::Context(_) | Line::Removed(_) => None,
            })
            .collect();
        assert!(added.contains(&"x"));
        assert!(added.contains(&"d"));
    }

    #[test]
    fn changes_close_together_share_a_hunk_and_no_line_shows_twice() {
        let old = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";
        let new = "1\n2\nthree\n4\n5\n6\nseven\n8\n9\n10\n";
        let hunks = diff(old, new);
        assert_eq!(hunks.len(), 1);
        let context: Vec<&str> = hunks[0]
            .lines
            .iter()
            .filter(|line| matches!(line, Line::Context(_)))
            .map(Line::text)
            .collect();
        assert_eq!(context, ["1", "2", "4", "5", "6", "8", "9", "10"]);
        // far apart, two hunks
        let far = "1\n2\nthree\n4\n5\n6\n7\n8\n9\n10\n11\nTWELVE\n";
        let old = format!("{old}11\n12\n");
        assert_eq!(diff(&old, far).len(), 2);
    }

    #[test]
    fn a_hunk_header_counts_its_context_lines() {
        let file_diff = FileDiff {
            path: "config.kdl".into(),
            hunks: diff("a\nb\nc\n", "a\nB\nc\nd\n"),
        };
        let out = render_unified(&file_diff);
        assert!(out.contains("@@ -1,3 +1,4 @@"), "{out}");
        let added_to_empty = FileDiff {
            path: "new.kdl".into(),
            hunks: diff("", "x\n"),
        };
        assert!(render_unified(&added_to_empty).contains("@@ -0,0 +1 @@"));
    }

    #[test]
    fn a_rendered_diff_applies_with_git() {
        let Ok(dir) = tempfile::TempDir::new() else {
            return;
        };
        let old = "layout {\n    gaps 8\n    // one\n    // two\n    // three\n    center-focused-column \"never\"\n}\n";
        let new = "layout {\n    gaps 16\n    // one\n    // two\n    // three\n    center-focused-column \"always\"\n}\nbinds {\n}\n";
        std::fs::write(dir.path().join("config.kdl"), old).unwrap();
        let patch = render_unified(&FileDiff {
            path: "config.kdl".into(),
            hunks: diff(old, new),
        });
        std::fs::write(dir.path().join("change.patch"), &patch).unwrap();
        let Ok(status) = std::process::Command::new("git")
            .args(["apply", "-p0", "change.patch"])
            .current_dir(dir.path())
            .status()
        else {
            return; // no git here
        };
        assert!(status.success(), "{patch}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("config.kdl")).unwrap(),
            new
        );
    }

    #[test]
    fn unified_render_has_markers() {
        let file_diff = FileDiff {
            path: "keybinds.kdl".into(),
            hunks: diff("a\nb\nc\n", "a\nB\nc\n"),
        };
        let out = render_unified(&file_diff);
        assert!(out.contains("--- keybinds.kdl"));
        assert!(out.contains("@@ -"));
        assert!(out.contains("-b"));
        assert!(out.contains("+B"));
    }
}
