//! Stable, source-located diagnostics shared by the CLI and workspace.

use std::{
    fmt,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

/// Diagnostic severity.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// The candidate may publish, but the user should inspect this condition.
    Warning,
    /// The candidate is invalid and cannot publish or render.
    Error,
}

/// A byte-precise source range with human-friendly line and column data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceLocation {
    /// Project-relative source path.
    pub file: PathBuf,
    /// Inclusive UTF-8 byte offset.
    pub start: usize,
    /// Exclusive UTF-8 byte offset.
    pub end: usize,
    /// One-based line.
    pub line: usize,
    /// One-based Unicode-scalar column.
    pub column: usize,
}

impl SourceLocation {
    /// Resolves line and column for a byte range in source.
    #[must_use]
    pub fn from_offsets(file: impl Into<PathBuf>, source: &str, start: usize, end: usize) -> Self {
        let safe_start = start.min(source.len());
        let safe_end = end.max(safe_start).min(source.len());
        let prefix = &source[..safe_start];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let line_start = prefix.rfind('\n').map_or(0, |position| position + 1);
        let column = source[line_start..safe_start].chars().count() + 1;
        Self {
            file: file.into(),
            start: safe_start,
            end: safe_end,
            line,
            column,
        }
    }

    /// Creates a whole-file fallback location.
    #[must_use]
    pub fn file(file: impl Into<PathBuf>) -> Self {
        Self {
            file: file.into(),
            start: 0,
            end: 0,
            line: 1,
            column: 1,
        }
    }
}

/// One actionable parser, validation, runtime, or persistence finding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Diagnostic {
    /// Stable machine-readable code.
    pub code: String,
    /// Whether publication is blocked.
    pub severity: Severity,
    /// Concise description of the problem.
    pub message: String,
    /// Source location when the problem belongs to authored text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<SourceLocation>,
    /// Suggested next action, when one is reliable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
}

impl Diagnostic {
    /// Creates a blocking diagnostic.
    #[must_use]
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Error,
            message: message.into(),
            location: None,
            help: None,
        }
    }

    /// Creates a non-blocking diagnostic.
    #[must_use]
    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Warning,
            message: message.into(),
            location: None,
            help: None,
        }
    }

    /// Attaches a source location.
    #[must_use]
    pub fn at(mut self, location: SourceLocation) -> Self {
        self.location = Some(location);
        self
    }

    /// Attaches actionable help.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let severity = match self.severity {
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        if let Some(location) = &self.location {
            write!(
                formatter,
                "{}:{}:{}: {severity}[{}]: {}",
                location.file.display(),
                location.line,
                location.column,
                self.code,
                self.message
            )?;
        } else {
            write!(formatter, "{severity}[{}]: {}", self.code, self.message)?;
        }
        if let Some(help) = &self.help {
            write!(formatter, "\n  help: {help}")?;
        }
        Ok(())
    }
}

/// A complete set of findings for one operation.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct Diagnostics(Vec<Diagnostic>);

impl Diagnostics {
    /// Creates an empty finding set.
    #[must_use]
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    /// Adds one finding.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.0.push(diagnostic);
    }

    /// Extends this set.
    pub fn extend(&mut self, diagnostics: impl IntoIterator<Item = Diagnostic>) {
        self.0.extend(diagnostics);
    }

    /// Returns all findings in stable order.
    #[must_use]
    pub fn as_slice(&self) -> &[Diagnostic] {
        &self.0
    }

    /// Consumes the set.
    #[must_use]
    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.0
    }

    /// Returns true when any finding blocks publication.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.0
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    }

    /// Returns true when no findings exist.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Adds a diagnostic located from a source range.
    pub fn source_error(
        &mut self,
        code: &str,
        message: impl Into<String>,
        file: &Path,
        source: &str,
        start: usize,
        end: usize,
    ) {
        self.push(
            Diagnostic::error(code, message).at(SourceLocation::from_offsets(
                file.to_path_buf(),
                source,
                start,
                end,
            )),
        );
    }
}

impl IntoIterator for Diagnostics {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
