//! Lossless line-oriented syntax tokens used by semantic parsing and checked edits.

use std::path::{Path, PathBuf};

use crate::diagnostic::{Diagnostic, Diagnostics, SourceLocation};

/// A semantic token with an exact source range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    pub(crate) kind: TokenKind,
    pub(crate) value: String,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl Token {
    pub(crate) fn raw<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start..self.end]
    }
}

/// Significant token kinds. Whitespace and comments remain in the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenKind {
    Word,
    String,
    OpenBrace,
    CloseBrace,
    Newline,
}

/// A directive and its optional nested block.
#[derive(Debug, Clone)]
pub(crate) struct Statement {
    pub(crate) tokens: Vec<Token>,
    pub(crate) children: Vec<Statement>,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl Statement {
    pub(crate) fn keyword(&self) -> Option<&str> {
        self.tokens.first().map(|token| token.value.as_str())
    }

    pub(crate) fn location(&self, file: &Path, source: &str) -> SourceLocation {
        SourceLocation::from_offsets(file.to_path_buf(), source, self.start, self.end)
    }
}

/// Tokenizes and groups one source file while retaining exact byte ranges.
pub(crate) fn parse_statements(file: &Path, source: &str) -> (Vec<Statement>, Diagnostics) {
    let (tokens, mut diagnostics) = lex(file, source);
    let mut cursor = 0;
    let (statements, _) = group(file, source, &tokens, &mut cursor, false, &mut diagnostics);
    (statements, diagnostics)
}

fn lex(file: &Path, source: &str) -> (Vec<Token>, Diagnostics) {
    let mut tokens = Vec::new();
    let mut diagnostics = Diagnostics::new();
    let mut cursor = 0;
    let bytes = source.as_bytes();

    while cursor < bytes.len() {
        match bytes[cursor] {
            b' ' | b'\t' | b'\r' => cursor += 1,
            b'\n' | b';' => {
                tokens.push(Token {
                    kind: TokenKind::Newline,
                    value: "\n".to_owned(),
                    start: cursor,
                    end: cursor + 1,
                });
                cursor += 1;
            }
            b'#' => {
                cursor += 1;
                while cursor < bytes.len() && bytes[cursor] != b'\n' {
                    cursor += 1;
                }
            }
            b'{' => {
                tokens.push(Token {
                    kind: TokenKind::OpenBrace,
                    value: "{".to_owned(),
                    start: cursor,
                    end: cursor + 1,
                });
                cursor += 1;
            }
            b'}' => {
                tokens.push(Token {
                    kind: TokenKind::CloseBrace,
                    value: "}".to_owned(),
                    start: cursor,
                    end: cursor + 1,
                });
                cursor += 1;
            }
            b'"' => {
                let start = cursor;
                cursor += 1;
                let mut value = String::new();
                let mut terminated = false;
                while cursor < bytes.len() {
                    match bytes[cursor] {
                        b'"' => {
                            cursor += 1;
                            terminated = true;
                            break;
                        }
                        b'\\' => {
                            cursor += 1;
                            if cursor >= bytes.len() {
                                break;
                            }
                            match bytes[cursor] {
                                b'n' => value.push('\n'),
                                b't' => value.push('\t'),
                                b'r' => value.push('\r'),
                                b'"' => value.push('"'),
                                b'\\' => value.push('\\'),
                                other => {
                                    value.push('\\');
                                    value.push(char::from(other));
                                }
                            }
                            cursor += 1;
                        }
                        b'\n' => break,
                        _ => {
                            let Some(character) =
                                source.get(cursor..).and_then(|rest| rest.chars().next())
                            else {
                                cursor = bytes.len();
                                break;
                            };
                            value.push(character);
                            cursor += character.len_utf8();
                        }
                    }
                }
                if !terminated {
                    diagnostics.push(
                        Diagnostic::error("CRP001", "unterminated string literal")
                            .at(SourceLocation::from_offsets(
                                file.to_path_buf(),
                                source,
                                start,
                                cursor,
                            ))
                            .with_help("close the string with a double quote on the same line"),
                    );
                }
                tokens.push(Token {
                    kind: TokenKind::String,
                    value,
                    start,
                    end: cursor,
                });
            }
            _ => {
                let start = cursor;
                while cursor < bytes.len()
                    && !matches!(
                        bytes[cursor],
                        b' ' | b'\t' | b'\r' | b'\n' | b';' | b'{' | b'}' | b'"'
                    )
                {
                    let Some(character) = source.get(cursor..).and_then(|rest| rest.chars().next())
                    else {
                        cursor = bytes.len();
                        break;
                    };
                    cursor += character.len_utf8();
                }
                tokens.push(Token {
                    kind: TokenKind::Word,
                    value: source[start..cursor].to_owned(),
                    start,
                    end: cursor,
                });
            }
        }
    }
    if tokens
        .last()
        .is_none_or(|token| token.kind != TokenKind::Newline)
    {
        tokens.push(Token {
            kind: TokenKind::Newline,
            value: "\n".to_owned(),
            start: source.len(),
            end: source.len(),
        });
    }
    (tokens, diagnostics)
}

fn group(
    file: &Path,
    source: &str,
    tokens: &[Token],
    cursor: &mut usize,
    nested: bool,
    diagnostics: &mut Diagnostics,
) -> (Vec<Statement>, Option<usize>) {
    let mut statements = Vec::new();
    while *cursor < tokens.len() {
        while tokens
            .get(*cursor)
            .is_some_and(|token| token.kind == TokenKind::Newline)
        {
            *cursor += 1;
        }
        let Some(token) = tokens.get(*cursor) else {
            break;
        };
        if token.kind == TokenKind::CloseBrace {
            if nested {
                *cursor += 1;
                if tokens
                    .get(*cursor)
                    .is_some_and(|next| next.kind != TokenKind::Newline)
                {
                    diagnostics.push(
                        Diagnostic::error("CRP002", "unexpected text after closing brace").at(
                            SourceLocation::from_offsets(
                                file.to_path_buf(),
                                source,
                                token.start,
                                token.end,
                            ),
                        ),
                    );
                    skip_line(tokens, cursor);
                }
                return (statements, Some(token.end));
            }
            diagnostics.push(
                Diagnostic::error("CRP003", "closing brace has no matching block").at(
                    SourceLocation::from_offsets(
                        file.to_path_buf(),
                        source,
                        token.start,
                        token.end,
                    ),
                ),
            );
            *cursor += 1;
            continue;
        }

        let start = token.start;
        let mut line = Vec::new();
        let mut opens_block = false;
        while let Some(token) = tokens.get(*cursor) {
            match token.kind {
                TokenKind::Newline => {
                    *cursor += 1;
                    break;
                }
                TokenKind::OpenBrace => {
                    opens_block = true;
                    *cursor += 1;
                    if tokens
                        .get(*cursor)
                        .is_some_and(|next| next.kind != TokenKind::Newline)
                    {
                        diagnostics.push(
                            Diagnostic::error(
                                "CRP004",
                                "an opening brace must end its directive line",
                            )
                            .at(SourceLocation::from_offsets(
                                file.to_path_buf(),
                                source,
                                token.start,
                                token.end,
                            )),
                        );
                        skip_line(tokens, cursor);
                    } else if tokens.get(*cursor).is_some() {
                        *cursor += 1;
                    }
                    break;
                }
                TokenKind::CloseBrace => {
                    diagnostics.push(
                        Diagnostic::error("CRP005", "a closing brace must be on its own line").at(
                            SourceLocation::from_offsets(
                                file.to_path_buf(),
                                source,
                                token.start,
                                token.end,
                            ),
                        ),
                    );
                    break;
                }
                TokenKind::Word | TokenKind::String => {
                    line.push(token.clone());
                    *cursor += 1;
                }
            }
        }
        if line.is_empty() {
            continue;
        }
        let (children, closing_brace_end) = if opens_block {
            group(file, source, tokens, cursor, true, diagnostics)
        } else {
            (Vec::new(), None)
        };
        let end = closing_brace_end.unwrap_or_else(|| {
            children.last().map_or_else(
                || line.last().map_or(start, |token| token.end),
                |statement| statement.end,
            )
        });
        statements.push(Statement {
            tokens: line,
            children,
            start,
            end,
        });
    }
    if nested {
        diagnostics.push(
            Diagnostic::error("CRP006", "block is missing a closing brace")
                .at(SourceLocation::from_offsets(
                    PathBuf::from(file),
                    source,
                    source.len(),
                    source.len(),
                ))
                .with_help("add `}` on its own line"),
        );
    }
    (statements, None)
}

fn skip_line(tokens: &[Token], cursor: &mut usize) {
    while let Some(token) = tokens.get(*cursor) {
        *cursor += 1;
        if token.kind == TokenKind::Newline {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_blocks_and_preserves_token_ranges() {
        let source = "project \"Test\" {\n  id example # comment\n}\n";
        let (statements, diagnostics) = parse_statements(Path::new("project.crump"), source);
        assert!(diagnostics.is_empty());
        assert_eq!(statements[0].keyword(), Some("project"));
        assert_eq!(statements[0].children[0].tokens[1].raw(source), "example");
        assert_eq!(
            &source[statements[0].start..statements[0].end],
            source.trim_end()
        );
    }

    #[test]
    fn sharp_pitch_words_do_not_start_comments() {
        let source = "note 0beat C#5 1beat 1 # trailing comment\n";
        let (statements, diagnostics) = parse_statements(Path::new("project.crump"), source);
        assert!(diagnostics.is_empty());
        assert_eq!(statements[0].tokens[2].value, "C#5");
        assert_eq!(statements[0].tokens.len(), 5);
    }

    #[test]
    fn reports_unclosed_blocks() {
        let (_, diagnostics) = parse_statements(Path::new("project.crump"), "project \"Test\" {\n");
        assert!(diagnostics.has_errors());
    }
}
