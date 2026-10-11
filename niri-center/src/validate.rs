//! `niri validate` pre-commit gate.
//!
//! Both GUI tools promise "your config can't be broken by the settings app".
//! The mechanism: before any write lands on disk, run the config through
//! `niri validate` and refuse the write on failure. `niri validate` runs the
//! full validation pipeline niri itself uses (keys, modes, rules, profiles),
//! so it is the authoritative check — better than any reimplementation.

use std::path::Path;
use std::process::Command;

/// Outcome of a validation run.
#[derive(Debug, Clone)]
pub struct Report {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Report {
    /// Human-readable diagnostics (stderr preferred, stdout as fallback).
    pub fn diagnostics(&self) -> String {
        if self.ok {
            String::new()
        } else if !self.stderr.trim().is_empty() {
            self.stderr.clone()
        } else {
            self.stdout.clone()
        }
    }
}

/// One error in niri's diagnostics, read from its words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    /// The file as niri names it (its file name) and the 1-based line it
    /// marks, when it points into one.
    pub place: Option<(String, usize)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticKind {
    /// A node name niri does not know: "unexpected node `gapz`".
    UnknownNode(String),
    /// Anything else, in niri's words.
    Other(String),
}

/// The errors in niri's diagnostics, in the order it reports them.
///
/// niri prints each as `Error:   × <what>`, then for most a snippet
/// headed `╭─[file:line:column]` whose numbered lines are followed by a
/// `·` line under the one it means. Lines that are neither are skipped,
/// so a change of layout loses detail, never an error.
pub fn diagnostics(text: &str) -> Vec<Diagnostic> {
    let mut found: Vec<Diagnostic> = Vec::new();
    let mut file = None;
    let mut last_line = None;
    for line in text.lines() {
        let trimmed = line.trim();
        let error = trimmed.strip_prefix("Error:");
        // a cause under an error, as some versions print the parse error
        let cause = trimmed
            .strip_prefix("├─▶")
            .or_else(|| trimmed.strip_prefix("╰─▶"));
        let what = error
            .or(cause)
            .unwrap_or(trimmed)
            .trim_start()
            .trim_start_matches('×')
            .trim();
        let unknown = what
            .strip_prefix("unexpected node `")
            .and_then(|rest| rest.strip_suffix('`'))
            .filter(|name| !name.is_empty());
        if unknown.is_some() || error.is_some() {
            let kind = match unknown {
                Some(name) => DiagnosticKind::UnknownNode(name.to_string()),
                None => DiagnosticKind::Other(what.to_string()),
            };
            found.push(Diagnostic { kind, place: None });
            (file, last_line) = (None, None);
        } else if let Some(header) = trimmed
            .strip_prefix("╭─[")
            .and_then(|rest| rest.strip_suffix(']'))
        {
            file = header.split(':').next().map(str::to_string);
        } else if trimmed.starts_with('·') {
            if let (Some(diagnostic), Some(name), Some(number)) =
                (found.last_mut(), &file, last_line)
                && diagnostic.place.is_none()
            {
                diagnostic.place = Some((name.clone(), number));
            }
        } else if let Some((number, _)) = trimmed.split_once('│') {
            last_line = number.trim().parse().ok();
        }
    }
    found
}

/// Validate `path` using the `niri` binary from `$PATH`.
///
/// Returns `Err` only when the binary is missing or cannot be executed —
/// not when the config is invalid (that is `Report { ok: false, .. }`).
pub fn validate(path: &Path) -> Result<Report, ValidateError> {
    // NO_COLOR: the diagnostics are shown in the GUI, where ANSI escapes
    // would print as garbage
    let output = Command::new("niri")
        .env("NO_COLOR", "1")
        .arg("validate")
        .arg("-c")
        .arg(path)
        .output()
        .map_err(ValidateError::Io)?;
    Ok(Report {
        ok: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Whether niri reads the config at `path`: its words when it does not,
/// or why it could not be asked.
pub fn reads(path: &Path) -> Result<(), String> {
    let report = validate(path).map_err(|e| e.to_string())?;
    if report.ok {
        Ok(())
    } else {
        Err(report.diagnostics())
    }
}

/// Failures other than "the config is invalid".
#[derive(Debug, thiserror::Error)]
pub enum ValidateError {
    #[error("failed to run `niri validate`: {0}")]
    Io(std::io::Error),
}

/// Whether the `niri` binary is on `$PATH` (used to skip validate-dependent
/// tests and to warn the user when validation is unavailable).
pub fn niri_available() -> bool {
    std::process::Command::new("niri")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// niri 26.04's words for a misspelled name in an included file.
    const INCLUDED: &str = "Error:   × error loading config
  ├─▶ error parsing
  ╰─▶ error parsing KDL

Error:   × failed to parse included config
   ╭─[config.kdl:1:1]
 1 │ include \"b.kdl\"
   · ───┬───
   ·    ╰── node starts here
   ╰────
Error:   × error parsing KDL

Error:   × unexpected node `numlok`
   ╭─[b.kdl:2:1]
 2 │   keyboard {
 3 │     numlok
   ·     ───┬───
   ·        ╰── unexpected node
 4 │   }
   ╰────
";

    #[test]
    fn an_unknown_name_is_found_where_niri_points() {
        let found = diagnostics(INCLUDED);
        let unknown = found
            .iter()
            .find(|d| matches!(d.kind, DiagnosticKind::UnknownNode(_)))
            .unwrap();
        assert_eq!(unknown.kind, DiagnosticKind::UnknownNode("numlok".into()));
        assert_eq!(unknown.place, Some(("b.kdl".into(), 3)));
        assert_eq!(found[1].place, Some(("config.kdl".into(), 1)));
    }

    #[test]
    fn a_name_in_a_cause_line_or_alone_is_found_too() {
        for text in [
            "Error:   × error loading config\n  ╰─▶ unexpected node `gpas`",
            "unexpected node `gpas`",
        ] {
            assert!(
                diagnostics(text)
                    .iter()
                    .any(|d| d.kind == DiagnosticKind::UnknownNode("gpas".into())),
                "{text}"
            );
        }
    }

    #[test]
    fn niri_itself_still_words_it_this_way() {
        if !niri_available() {
            return;
        }
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.kdl");
        std::fs::write(&path, "layout {\n    gapz 8\n}\n").unwrap();
        let report = validate(&path).unwrap();
        assert!(!report.ok);
        let found = diagnostics(&report.diagnostics());
        assert!(
            found
                .iter()
                .any(|d| d.kind == DiagnosticKind::UnknownNode("gapz".into())
                    && d.place == Some(("config.kdl".into(), 2))),
            "{found:?}"
        );
    }
}
