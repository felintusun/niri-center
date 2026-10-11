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
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let ops = lcs_ops(&a, &b);

    const CONTEXT: usize = 3;
    let is_keep = |index: usize| matches!(ops[index], Op::Keep(..));
    let mut hunks = Vec::new();
    let mut pos = 0;
    // line numbers before op `counted`, carried along instead of recounted
    let (mut counted, mut old_line, mut new_line) = (0, 1, 1);

    while pos < ops.len() {
        if is_keep(pos) {
            pos += 1;
            continue;
        }
        // the changes from here on that lie close together: one hunk, as
        // their contexts would otherwise overlap
        let block_start = pos;
        let mut end = pos;
        loop {
            while end < ops.len() && !is_keep(end) {
                end += 1;
            }
            let mut next = end;
            while next < ops.len() && is_keep(next) {
                next += 1;
            }
            if next < ops.len() && next - end <= 2 * CONTEXT {
                end = next;
            } else {
                break;
            }
        }
        let hunk_end = (end + CONTEXT).min(ops.len());
        let start = block_start.saturating_sub(CONTEXT);
        for op in &ops[counted..start] {
            old_line += usize::from(!matches!(op, Op::Ins(_)));
            new_line += usize::from(!matches!(op, Op::Del(_)));
        }
        counted = start;
        let (old_start, new_start) = (old_line, new_line);

        let mut lines = Vec::new();
        for op in &ops[start..hunk_end] {
            match op {
                Op::Keep(ol, _nl) => lines.push(Line::Context(a[*ol].to_string())),
                Op::Del(ol) => lines.push(Line::Removed(a[*ol].to_string())),
                Op::Ins(nl) => lines.push(Line::Added(b[*nl].to_string())),
            }
        }
        hunks.push(Hunk {
            old_start,
            new_start,
            lines,
        });
        pos = hunk_end;
    }
    hunks
}

/// One step of an edit script from `a` to `b`, by index.
pub(crate) enum Op {
    /// The same item in both: its index in `a`, then in `b`.
    Keep(usize, usize),
    /// An item only `a` has.
    Del(usize),
    /// An item only `b` has.
    Ins(usize),
}

/// The edit script from `a` to `b` by their longest common subsequence
/// (standard DP), for lines and for any other items. What both start and
/// end with is kept outright first: a change is mostly one block in a long
/// file, and the table then covers only it.
pub(crate) fn lcs_ops<T: PartialEq>(a: &[T], b: &[T]) -> Vec<Op> {
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (a_mid, b_mid) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    let mut ops: Vec<Op> = (0..prefix).map(|i| Op::Keep(i, i)).collect();
    ops.extend(lcs_core(a_mid, b_mid).into_iter().map(|op| match op {
        Op::Keep(i, j) => Op::Keep(i + prefix, j + prefix),
        Op::Del(i) => Op::Del(i + prefix),
        Op::Ins(j) => Op::Ins(j + prefix),
    }));
    let (a_end, b_end) = (a.len() - suffix, b.len() - suffix);
    ops.extend((0..suffix).map(|k| Op::Keep(a_end + k, b_end + k)));
    ops
}

fn lcs_core<T: PartialEq>(a: &[T], b: &[T]) -> Vec<Op> {
    let n = a.len();
    let m = b.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }

    let mut ops = Vec::with_capacity(n + m);
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            ops.push(Op::Keep(i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            ops.push(Op::Del(i));
            i += 1;
        } else {
            ops.push(Op::Ins(j));
            j += 1;
        }
    }
    while i < n {
        ops.push(Op::Del(i));
        i += 1;
    }
    while j < m {
        ops.push(Op::Ins(j));
        j += 1;
    }
    ops
}

/// Unified-diff rendering, mostly for tests and future `--export`.
pub fn render_unified(file: &FileDiff) -> String {
    let mut s = format!("--- {}\n+++ {}\n", file.path, file.path);
    for h in &file.hunks {
        // each side counts its context lines too, as `patch` reads them
        let (old, new) = h.numbered().fold((0, 0), |(old, new), (o, n, _)| {
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
        s.push_str(&format!(
            "@@ -{} +{} @@\n",
            range(h.old_start, old),
            range(h.new_start, new)
        ));
        for line in &h.lines {
            s.push_str(&format!("{}{}\n", line.prefix(), line.text()));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

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
            .flat_map(|h| &h.lines)
            .filter_map(|l| match l {
                Line::Removed(t) => Some(t.as_str()),
                Line::Context(_) | Line::Added(_) => None,
            })
            .next()
            .is_none();
        assert!(no_removals, "no pure removals expected");
        let added: Vec<_> = hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter_map(|l| match l {
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
            .filter(|l| matches!(l, Line::Context(_)))
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
        let f = FileDiff {
            path: "config.kdl".into(),
            hunks: diff("a\nb\nc\n", "a\nB\nc\nd\n"),
        };
        let out = render_unified(&f);
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
        let f = FileDiff {
            path: "keybinds.kdl".into(),
            hunks: diff("a\nb\nc\n", "a\nB\nc\n"),
        };
        let out = render_unified(&f);
        assert!(out.contains("--- keybinds.kdl"));
        assert!(out.contains("@@ -"));
        assert!(out.contains("-b"));
        assert!(out.contains("+B"));
    }
}
