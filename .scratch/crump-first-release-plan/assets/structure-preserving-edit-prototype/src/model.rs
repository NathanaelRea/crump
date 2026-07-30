use std::ops::Range;

use rowan::{GreenNode, GreenNodeBuilder, Language, SyntaxNode};

const INITIAL_SOURCE: &str = r#"# Odd spacing and comments are intentional.
track bass [id: trk-bass] {
  gain    = 0.72      # keep this comment
  cutoff = lfo(rate: 1/8)  # procedural; do not flatten

  automation filter_cutoff {
    point intro = 1:1, 0.20   # opening
    point drop  = 17:1, 0.80  # impact
  }
}
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
enum Kind {
    Root,
    Whitespace,
    Newline,
    Comment,
    Ident,
    Number,
    String,
    Punctuation,
    Error,
}

impl From<Kind> for rowan::SyntaxKind {
    fn from(kind: Kind) -> Self {
        Self(kind as u16)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum CrumpLanguage {}

impl Language for CrumpLanguage {
    type Kind = Kind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Kind {
        match raw.0 {
            0 => Kind::Root,
            1 => Kind::Whitespace,
            2 => Kind::Newline,
            3 => Kind::Comment,
            4 => Kind::Ident,
            5 => Kind::Number,
            6 => Kind::String,
            7 => Kind::Punctuation,
            _ => Kind::Error,
        }
    }

    fn kind_to_raw(kind: Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

type CrumpNode = SyntaxNode<CrumpLanguage>;

#[derive(Debug, Clone)]
struct Token {
    kind: Kind,
    text: String,
    range: Range<usize>,
}

#[derive(Debug, Clone)]
struct Locator {
    revision: u64,
    role: String,
    range: Range<usize>,
    expected: String,
}

#[derive(Debug, Clone)]
struct Snapshot {
    source: String,
    summary: String,
}

#[derive(Debug, Clone)]
pub enum Action {
    SetGain(String),
    SetPoint { name: String, value: String },
    RenameTrack(String),
    SetCutoff(String),
    SimulateConflict,
    Undo,
    Redo,
    Reload,
}

pub struct Editor {
    source: String,
    revision: u64,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_result: String,
}

impl Editor {
    pub fn new() -> Self {
        let source = INITIAL_SOURCE.to_owned();
        let tree = parse_lossless(&source);
        assert_eq!(tree.text().to_string(), source);
        Self {
            source,
            revision: 1,
            undo: Vec::new(),
            redo: Vec::new(),
            last_result: "Ready. Try a structured edit.".to_owned(),
        }
    }

    pub fn dispatch(&mut self, action: Action) {
        let result = match action {
            Action::SetGain(value) => self.replace_numeric_property("gain", &value),
            Action::SetPoint { name, value } => self.replace_point(&name, &value),
            Action::RenameTrack(name) => self.rename_track(&name),
            Action::SetCutoff(value) => self.replace_numeric_property("cutoff", &value),
            Action::SimulateConflict => self.simulate_conflict(),
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::Reload => self.reload(),
        };

        self.last_result = match result {
            Ok(message) | Err(message) => message,
        };
    }

    pub fn render(&self) -> String {
        let tokens = lex(&self.source);
        let track = locate_track(&tokens);
        let (track_name, track_id) = track
            .map(|(name, id)| (name.expected, id.expected))
            .unwrap_or_else(|| ("<unresolved>".into(), "<unresolved>".into()));
        let gain_range = locate_property(&tokens, self.revision, "gain")
            .map(|locator| format!("{}..{}", locator.range.start, locator.range.end))
            .unwrap_or_else(|_| "<unresolved>".into());

        format!(
            "\x1b[1mPROTOTYPE: Structure-Preserving Source-Backed Edits\x1b[0m\n\n\
             \x1b[1mRevision\x1b[0m       {}\n\
             \x1b[1mTrack name\x1b[0m     {} \x1b[2m(semantic locator)\x1b[0m\n\
             \x1b[1mTrack ID\x1b[0m       {} \x1b[2m(durable source identity)\x1b[0m\n\
             \x1b[1mGain range\x1b[0m     {} \x1b[2m(revision-local locator)\x1b[0m\n\
             \x1b[1mUndo / redo\x1b[0m   {} / {}\n\n\
             \x1b[1mSource (all bytes round-trip through Rowan)\x1b[0m\n{}\n\
             \x1b[1mLast result\x1b[0m\n{}\n\n\
             \x1b[1mCommands\x1b[0m\n\
             gain <value>       point <name> <value>    rename <name>\n\
             cutoff <value>     conflict                undo | redo | reload | quit\n",
            self.revision,
            track_name,
            track_id,
            gain_range,
            self.undo.len(),
            self.redo.len(),
            self.source,
            self.last_result,
        )
    }

    fn replace_numeric_property(&mut self, property: &str, value: &str) -> Result<String, String> {
        parse_number(value)?;
        let tokens = lex(&self.source);
        let locator = locate_property(&tokens, self.revision, property)?;
        if token_kind_at(&tokens, locator.range.clone()) != Some(Kind::Number) {
            return Err(format!(
                "REFUSED: {property} is `{}` ({}) rather than a literal number. The expression remains byte-for-byte unchanged.",
                locator.expected, locator.role
            ));
        }
        self.apply(locator, value, format!("set {property} to {value}"))
    }

    fn replace_point(&mut self, name: &str, value: &str) -> Result<String, String> {
        parse_number(value)?;
        let tokens = lex(&self.source);
        let locator = locate_point_value(&tokens, self.revision, name)?;
        self.apply(locator, value, format!("set point {name} to {value}"))
    }

    fn rename_track(&mut self, name: &str) -> Result<String, String> {
        if name.is_empty() || !name.chars().all(is_ident_continue) {
            return Err(
                "REFUSED: a track name must contain only letters, digits, `_`, or `-`.".into(),
            );
        }
        let tokens = lex(&self.source);
        let (locator, _) =
            locate_track(&tokens).ok_or("REFUSED: track declaration is unresolved.")?;
        let old_name = locator.expected.clone();
        self.apply(
            Locator {
                revision: self.revision,
                ..locator
            },
            name,
            format!("renamed track {old_name} to {name}; durable ID trk-bass was untouched"),
        )
    }

    fn simulate_conflict(&mut self) -> Result<String, String> {
        let tokens = lex(&self.source);
        let stale = locate_property(&tokens, self.revision, "gain")?;
        let marker = if self.source.contains("# keep this comment") {
            "# keep this comment"
        } else {
            "# keep this comment (direct edit)"
        };
        let replacement = if marker.ends_with("(direct edit)") {
            "# keep this comment"
        } else {
            "# keep this comment (direct edit)"
        };
        let start = self
            .source
            .find(marker)
            .ok_or("REFUSED: conflict marker not found.")?;
        self.apply(
            Locator {
                revision: self.revision,
                role: "direct text selection".into(),
                range: start..start + marker.len(),
                expected: marker.into(),
            },
            replacement,
            "direct text edit".into(),
        )?;

        match self.apply(stale, "0.99", "stale TUI gain edit".into()) {
            Ok(_) => Err("FAILED: stale edit unexpectedly applied.".into()),
            Err(reason) => Ok(format!(
                "CONFLICT SAFELY REJECTED after direct edit: {reason} The gain was not changed."
            )),
        }
    }

    fn apply(
        &mut self,
        locator: Locator,
        replacement: &str,
        summary: String,
    ) -> Result<String, String> {
        if locator.revision != self.revision {
            return Err(format!(
                "stale {} locator expected revision {}, current revision is {}.",
                locator.role, locator.revision, self.revision
            ));
        }
        if self.source.get(locator.range.clone()) != Some(locator.expected.as_str()) {
            return Err(format!(
                "{} target text changed before commit.",
                locator.role
            ));
        }

        let before = self.source.clone();
        let mut after = before.clone();
        after.replace_range(locator.range.clone(), replacement);
        let tree = parse_lossless(&after);
        if tree.text().to_string() != after {
            return Err("lossless parse failed to reproduce edited bytes.".into());
        }
        let suffix_start = locator.range.start + replacement.len();
        let outside_unchanged = before[..locator.range.start] == after[..locator.range.start]
            && before[locator.range.end..] == after[suffix_start..];
        if !outside_unchanged {
            return Err("bytes outside the declared edit range changed.".into());
        }

        self.undo.push(Snapshot {
            source: before,
            summary: summary.clone(),
        });
        self.redo.clear();
        self.source = after;
        self.revision += 1;
        Ok(format!(
            "APPLIED: {summary}. Exactly one range changed; all outside bytes are identical."
        ))
    }

    fn undo(&mut self) -> Result<String, String> {
        let snapshot = self.undo.pop().ok_or("REFUSED: nothing to undo.")?;
        self.redo.push(Snapshot {
            source: self.source.clone(),
            summary: snapshot.summary.clone(),
        });
        self.source = snapshot.source;
        self.revision += 1;
        Ok(format!(
            "UNDONE exactly: {}. The complete prior byte sequence was restored.",
            snapshot.summary
        ))
    }

    fn redo(&mut self) -> Result<String, String> {
        let snapshot = self.redo.pop().ok_or("REFUSED: nothing to redo.")?;
        self.undo.push(Snapshot {
            source: self.source.clone(),
            summary: snapshot.summary.clone(),
        });
        self.source = snapshot.source;
        self.revision += 1;
        Ok(format!("REDONE exactly: {}.", snapshot.summary))
    }

    fn reload(&mut self) -> Result<String, String> {
        let tree = parse_lossless(&self.source);
        if tree.text().to_string() != self.source {
            return Err("RELOAD FAILED: lossless parse changed source bytes.".into());
        }
        let tokens = lex(&self.source);
        let (name, id) = locate_track(&tokens).ok_or("RELOAD FAILED: track unresolved.")?;
        self.revision += 1;
        Ok(format!(
            "RELOADED from authoritative text: track `{}` recovered durable ID `{}`. Revision-local locators were discarded.",
            name.expected, id.expected
        ))
    }
}

fn parse_number(value: &str) -> Result<f64, String> {
    value
        .parse::<f64>()
        .map_err(|_| format!("REFUSED: `{value}` is not a numeric literal."))
}

fn parse_lossless(source: &str) -> CrumpNode {
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(Kind::Root.into());
    for token in lex(source) {
        builder.token(token.kind.into(), &token.text);
    }
    builder.finish_node();
    let green: GreenNode = builder.finish();
    CrumpNode::new_root(green)
}

fn lex(source: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut offset = 0;
    while offset < source.len() {
        let rest = &source[offset..];
        let first = rest.chars().next().expect("non-empty remainder");
        let (kind, len) = if first == '\n' {
            (Kind::Newline, 1)
        } else if first.is_whitespace() {
            (
                Kind::Whitespace,
                take_while(rest, |ch| ch != '\n' && ch.is_whitespace()),
            )
        } else if first == '#' {
            (Kind::Comment, rest.find('\n').unwrap_or(rest.len()))
        } else if first == '"' {
            let end = rest[1..]
                .find('"')
                .map(|position| position + 2)
                .unwrap_or(rest.len());
            (Kind::String, end)
        } else if first.is_ascii_digit()
            || (first == '-' && rest[1..].starts_with(|ch: char| ch.is_ascii_digit()))
        {
            (
                Kind::Number,
                take_while(rest, |ch| ch.is_ascii_digit() || matches!(ch, '.' | '-')),
            )
        } else if first.is_ascii_alphabetic() || first == '_' {
            (Kind::Ident, take_while(rest, is_ident_continue))
        } else if "{}[]():,=/".contains(first) {
            (Kind::Punctuation, first.len_utf8())
        } else {
            (Kind::Error, first.len_utf8())
        };
        let end = offset + len;
        tokens.push(Token {
            kind,
            text: source[offset..end].to_owned(),
            range: offset..end,
        });
        offset = end;
    }
    tokens
}

fn take_while(source: &str, predicate: impl Fn(char) -> bool) -> usize {
    source
        .char_indices()
        .take_while(|(_, ch)| predicate(*ch))
        .map(|(index, ch)| index + ch.len_utf8())
        .last()
        .unwrap_or_else(|| source.chars().next().map(char::len_utf8).unwrap_or(0))
}

fn is_ident_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')
}

fn is_trivia(token: &Token) -> bool {
    matches!(token.kind, Kind::Whitespace | Kind::Newline | Kind::Comment)
}

fn next_nontrivia(tokens: &[Token], mut index: usize) -> Option<(usize, &Token)> {
    while let Some(token) = tokens.get(index) {
        if !is_trivia(token) {
            return Some((index, token));
        }
        index += 1;
    }
    None
}

fn locate_track(tokens: &[Token]) -> Option<(Locator, Locator)> {
    let track_index = tokens
        .iter()
        .position(|token| token.kind == Kind::Ident && token.text == "track")?;
    let (name_index, name) = next_nontrivia(tokens, track_index + 1)?;
    let id_index = tokens
        .iter()
        .enumerate()
        .skip(name_index + 1)
        .find(|(_, token)| token.kind == Kind::Ident && token.text == "id")?
        .0;
    let (colon_index, colon) = next_nontrivia(tokens, id_index + 1)?;
    if colon.text != ":" {
        return None;
    }
    let (_, id) = next_nontrivia(tokens, colon_index + 1)?;
    Some((
        Locator {
            revision: 0,
            role: "track name".into(),
            range: name.range.clone(),
            expected: name.text.clone(),
        },
        Locator {
            revision: 0,
            role: "durable track ID".into(),
            range: id.range.clone(),
            expected: id.text.clone(),
        },
    ))
}

fn locate_property(tokens: &[Token], revision: u64, property: &str) -> Result<Locator, String> {
    let property_index = tokens
        .iter()
        .position(|token| token.kind == Kind::Ident && token.text == property)
        .ok_or_else(|| format!("REFUSED: property `{property}` is unresolved."))?;
    let (equals_index, equals) = next_nontrivia(tokens, property_index + 1)
        .ok_or_else(|| format!("REFUSED: property `{property}` has no value."))?;
    if equals.text != "=" {
        return Err(format!("REFUSED: property `{property}` has no `=`."));
    }
    let (value_index, first) = next_nontrivia(tokens, equals_index + 1)
        .ok_or_else(|| format!("REFUSED: property `{property}` has no value."))?;
    let line_end = tokens[value_index..]
        .iter()
        .position(|token| matches!(token.kind, Kind::Newline | Kind::Comment))
        .map(|relative| value_index + relative)
        .unwrap_or(tokens.len());
    let last = tokens[value_index..line_end]
        .iter()
        .rfind(|token| !is_trivia(token))
        .unwrap_or(first);
    Ok(Locator {
        revision,
        role: format!("{property} expression"),
        range: first.range.start..last.range.end,
        expected: tokens[value_index..line_end]
            .iter()
            .map(|token| token.text.as_str())
            .collect::<String>()
            .trim_end()
            .to_owned(),
    })
}

fn locate_point_value(tokens: &[Token], revision: u64, name: &str) -> Result<Locator, String> {
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != Kind::Ident || token.text != "point" {
            continue;
        }
        let (name_index, point_name) =
            next_nontrivia(tokens, index + 1).ok_or("REFUSED: malformed point.")?;
        if point_name.text != name {
            continue;
        }
        let (equals_index, equals) =
            next_nontrivia(tokens, name_index + 1).ok_or("REFUSED: malformed point.")?;
        if equals.text != "=" {
            return Err("REFUSED: malformed point assignment.".into());
        }
        let line_end = tokens[equals_index + 1..]
            .iter()
            .position(|token| matches!(token.kind, Kind::Newline | Kind::Comment))
            .map(|relative| equals_index + 1 + relative)
            .unwrap_or(tokens.len());
        let value = tokens[equals_index + 1..line_end]
            .iter()
            .rev()
            .find(|token| token.kind == Kind::Number)
            .ok_or("REFUSED: point has no numeric value.")?;
        return Ok(Locator {
            revision,
            role: format!("automation point `{name}` value"),
            range: value.range.clone(),
            expected: value.text.clone(),
        });
    }
    Err(format!("REFUSED: automation point `{name}` is unresolved."))
}

fn token_kind_at(tokens: &[Token], range: Range<usize>) -> Option<Kind> {
    tokens
        .iter()
        .find(|token| token.range == range)
        .map(|token| token.kind)
}
