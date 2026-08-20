//! Semantic parser for Crump's deliberately small first-release language.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::{
    diagnostic::{Diagnostic, Diagnostics, SourceLocation},
    model::{
        Clip, ClipMode, Connection, DrumVoice, Endpoint, FaceplateControl, Graph, GraphNode,
        Instrument, InstrumentKind, NodeKind, Parameter, Pattern, PatternEvent, Project,
        SOURCE_VERSION, SettingValue, Song, Track, Unit, ValueSite, parse_pitch,
    },
    time::{Meter, MusicalTime, parse_musical_time, parse_seconds},
};

use super::syntax::{Statement, Token, TokenKind, parse_statements};

/// One top-level source import.
#[derive(Debug, Clone)]
pub struct ImportDeclaration {
    /// Authored relative or `library:` path.
    pub path: String,
    /// Source declaration location.
    pub location: SourceLocation,
}

/// Parsed root document before imported modules are loaded.
#[derive(Debug)]
pub struct ProjectDocument {
    /// Root Project when its required declarations could be recovered.
    pub project: Option<Project>,
    /// Top-level imports.
    pub imports: Vec<ImportDeclaration>,
    /// Complete parser diagnostics.
    pub diagnostics: Diagnostics,
}

/// Parsed human-readable Instrument Module.
#[derive(Debug)]
pub struct ModuleDocument {
    /// Instruments declared by the module.
    pub instruments: Vec<Instrument>,
    /// Complete parser diagnostics.
    pub diagnostics: Diagnostics,
}

/// Parses one root `project.crump` source document.
#[must_use]
pub fn parse_project(file: &Path, source: &str) -> ProjectDocument {
    let (statements, diagnostics) = parse_statements(file, source);
    let mut parser = Parser {
        file,
        source,
        diagnostics,
    };
    let mut imports = Vec::new();
    let mut project = None;
    let mut saw_header = false;

    for statement in &statements {
        match statement.keyword() {
            Some("crump") => {
                if saw_header {
                    parser.error_at("CRP100", "duplicate `crump` source header", statement);
                } else {
                    saw_header = true;
                    parser.parse_version_header(statement, "crump");
                }
            }
            Some("import") => {
                if let Some(path) =
                    parser.single_argument(statement, "CRP101", "import requires one quoted path")
                {
                    imports.push(ImportDeclaration {
                        path: path.value.clone(),
                        location: statement.location(file, source),
                    });
                }
            }
            Some("project") => {
                if project.is_some() {
                    parser.error_at(
                        "CRP102",
                        "a source file may declare only one Project",
                        statement,
                    );
                } else {
                    project = parser.parse_project_block(statement);
                }
            }
            Some(keyword) => parser.unknown("root", keyword, statement),
            None => {}
        }
    }

    if !saw_header {
        parser.diagnostics.push(
            Diagnostic::error("CRP103", "missing `crump 1` source header")
                .at(SourceLocation::file(file.to_path_buf()))
                .with_help("put `crump 1` on the first non-comment line"),
        );
    }
    if project.is_none() {
        parser.diagnostics.push(
            Diagnostic::error(
                "CRP104",
                "source does not contain a recoverable Project declaration",
            )
            .at(SourceLocation::file(file.to_path_buf())),
        );
    }
    if let Some(parsed) = project.as_mut() {
        parsed.imports = imports
            .iter()
            .map(|import| PathBuf::from(&import.path))
            .collect();
    }

    ProjectDocument {
        project,
        imports,
        diagnostics: parser.diagnostics,
    }
}

/// Parses one `crump-module` Instrument Module.
#[must_use]
pub fn parse_module(file: &Path, source: &str) -> ModuleDocument {
    let (statements, diagnostics) = parse_statements(file, source);
    let mut parser = Parser {
        file,
        source,
        diagnostics,
    };
    let mut instruments = Vec::new();
    let mut saw_header = false;
    for statement in &statements {
        match statement.keyword() {
            Some("crump-module") => {
                if saw_header {
                    parser.error_at(
                        "CRP109",
                        "duplicate `crump-module` source header",
                        statement,
                    );
                } else {
                    saw_header = true;
                    parser.parse_version_header(statement, "crump-module");
                }
            }
            Some("instrument") => {
                if let Some(instrument) = parser.parse_instrument(statement) {
                    instruments.push(instrument);
                }
            }
            Some(keyword) => parser.unknown("Instrument Module", keyword, statement),
            None => {}
        }
    }
    if !saw_header {
        parser.diagnostics.push(
            Diagnostic::error("CRP105", "missing `crump-module 1` source header")
                .at(SourceLocation::file(file.to_path_buf())),
        );
    }
    if instruments.len() != 1 {
        parser.diagnostics.push(
            Diagnostic::error(
                "CRP106",
                format!(
                    "Instrument Module must contain exactly one instrument; found {}",
                    instruments.len()
                ),
            )
            .at(SourceLocation::file(file.to_path_buf())),
        );
    }
    ModuleDocument {
        instruments,
        diagnostics: parser.diagnostics,
    }
}

struct Parser<'a> {
    file: &'a Path,
    source: &'a str,
    diagnostics: Diagnostics,
}

impl Parser<'_> {
    fn parse_version_header(&mut self, statement: &Statement, header: &str) {
        if statement.tokens.len() != 2
            || statement.tokens[1].value.parse::<u32>() != Ok(SOURCE_VERSION)
        {
            self.error_at(
                "CRP107",
                format!("`{header}` header must declare supported version {SOURCE_VERSION}"),
                statement,
            );
        }
        if !statement.children.is_empty() {
            self.error_at(
                "CRP108",
                format!("`{header}` header cannot own a block"),
                statement,
            );
        }
    }

    fn parse_project_block(&mut self, statement: &Statement) -> Option<Project> {
        if statement.tokens.len() != 2 || statement.tokens[1].kind != TokenKind::String {
            self.error_at(
                "CRP110",
                "Project syntax is `project \"Name\" {`",
                statement,
            );
            return None;
        }
        if statement.children.is_empty() {
            self.error_at("CRP111", "Project declaration requires a block", statement);
        }
        let name = statement.tokens[1].value.clone();
        let mut id = None;
        let mut seed = None;
        let mut sample_rate = None;
        let mut render_tail_seconds = None;
        let mut song = None;

        for child in &statement.children {
            match child.keyword() {
                Some("id") => id = self.parse_id_value(child, "Project"),
                Some("seed") => seed = self.parse_u64_value(child, "seed"),
                Some("sample_rate") => sample_rate = self.parse_u32_value(child, "sample_rate"),
                Some("render_tail") => {
                    render_tail_seconds = self
                        .single_argument(child, "CRP112", "render_tail requires a duration")
                        .and_then(|token| match parse_seconds(&token.value) {
                            Ok(value) => Some(value),
                            Err(error) => {
                                self.token_error("CRP113", error.to_string(), token);
                                None
                            }
                        });
                }
                Some("song") => {
                    if song.is_some() {
                        self.error_at("CRP114", "a Project owns exactly one Song", child);
                    } else {
                        song = self.parse_song(child);
                    }
                }
                Some(keyword) => self.unknown("Project", keyword, child),
                None => {}
            }
        }

        let id = self.required(
            id,
            "CRP115",
            "Project is missing an `id` directive",
            statement,
        )?;
        let seed = self.required(
            seed,
            "CRP116",
            "Project is missing a `seed` directive",
            statement,
        )?;
        let sample_rate = self.required(
            sample_rate,
            "CRP117",
            "Project is missing `sample_rate`",
            statement,
        )?;
        let render_tail_seconds = self.required(
            render_tail_seconds,
            "CRP118",
            "Project is missing `render_tail`",
            statement,
        )?;
        let song = self.required(song, "CRP119", "Project is missing its Song", statement)?;
        Some(Project {
            source_version: SOURCE_VERSION,
            id,
            name,
            seed,
            sample_rate,
            render_tail_seconds,
            song,
            imports: Vec::new(),
        })
    }

    fn parse_song(&mut self, statement: &Statement) -> Option<Song> {
        if statement.tokens.len() != 1 || statement.children.is_empty() {
            self.error_at("CRP120", "Song syntax is `song {`", statement);
        }
        let mut tempo_bpm = None;
        let mut meter = None;
        let mut end_token: Option<&Token> = None;
        let mut instruments = Vec::new();
        let mut pattern_statements = Vec::new();
        let mut track_statements = Vec::new();

        for child in &statement.children {
            match child.keyword() {
                Some("tempo") => {
                    tempo_bpm = self
                        .single_argument(child, "CRP121", "tempo requires one BPM value")
                        .and_then(|token| self.parse_tempo(token));
                }
                Some("meter") => {
                    meter = self
                        .single_argument(child, "CRP122", "meter requires a value such as 4/4")
                        .and_then(|token| match Meter::from_str(&token.value) {
                            Ok(value) => Some(value),
                            Err(error) => {
                                self.token_error("CRP123", error.to_string(), token);
                                None
                            }
                        });
                }
                Some("end") => {
                    end_token =
                        self.single_argument(child, "CRP124", "end requires one musical duration")
                }
                Some("instrument") => {
                    if let Some(instrument) = self.parse_instrument(child) {
                        instruments.push(instrument);
                    }
                }
                Some("pattern") => pattern_statements.push(child),
                Some("track") => track_statements.push(child),
                Some(keyword) => self.unknown("Song", keyword, child),
                None => {}
            }
        }

        let tempo_bpm = self.required(tempo_bpm, "CRP125", "Song is missing `tempo`", statement)?;
        let meter = self.required(meter, "CRP126", "Song is missing `meter`", statement)?;
        let end = end_token.and_then(|token| self.parse_time(token, meter));
        let end = self.required(end, "CRP127", "Song is missing `end`", statement)?;
        let patterns = pattern_statements
            .into_iter()
            .filter_map(|pattern| self.parse_pattern(pattern, meter))
            .collect();
        let tracks = track_statements
            .into_iter()
            .filter_map(|track| self.parse_track(track, meter))
            .collect();
        Some(Song {
            tempo_bpm,
            meter,
            end,
            instruments,
            patterns,
            tracks,
        })
    }

    fn parse_instrument(&mut self, statement: &Statement) -> Option<Instrument> {
        if statement.tokens.len() != 4 || statement.tokens[2].kind != TokenKind::String {
            self.error_at(
                "CRP130",
                "instrument syntax is `instrument id \"Name\" graph|stock_drums {`",
                statement,
            );
            return None;
        }
        let id = statement.tokens[1].value.clone();
        let name = statement.tokens[2].value.clone();
        let kind = match statement.tokens[3].value.as_str() {
            "graph" => InstrumentKind::Graph,
            "stock_drums" => InstrumentKind::StockDrums,
            other => {
                self.token_error(
                    "CRP131",
                    format!("unknown instrument kind `{other}`"),
                    &statement.tokens[3],
                );
                return None;
            }
        };
        let mut polyphony = None;
        let mut parameters = Vec::new();
        let mut faceplate = Vec::new();
        let mut graph = None;
        for child in &statement.children {
            match child.keyword() {
                Some("polyphony") => polyphony = self.parse_u16_value(child, "polyphony"),
                Some("parameter") => {
                    if let Some(parameter) = self.parse_parameter(child) {
                        parameters.push(parameter);
                    }
                }
                Some("faceplate") => faceplate = self.parse_faceplate(child),
                Some("graph") => {
                    if graph.is_some() {
                        self.error_at("CRP132", "instrument contains more than one graph", child);
                    } else {
                        graph = self.parse_graph(child);
                    }
                }
                Some(keyword) => self.unknown("instrument", keyword, child),
                None => {}
            }
        }
        let polyphony = self.required(
            polyphony,
            "CRP133",
            "instrument is missing `polyphony`",
            statement,
        )?;
        Some(Instrument {
            id,
            name,
            kind,
            polyphony,
            parameters,
            faceplate,
            graph,
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_parameter(&mut self, statement: &Statement) -> Option<Parameter> {
        if !(8..=9).contains(&statement.tokens.len())
            || statement.tokens[2].kind != TokenKind::String
        {
            self.error_at(
                "CRP140",
                "parameter syntax is `parameter key \"Name\" unit min max default value [modulatable]`",
                statement,
            );
            return None;
        }
        let unit = match Unit::from_str(&statement.tokens[3].value) {
            Ok(unit) => unit,
            Err(error) => {
                self.token_error("CRP141", error.to_string(), &statement.tokens[3]);
                return None;
            }
        };
        let minimum = self.parse_physical(unit, &statement.tokens[4])?;
        let maximum = self.parse_physical(unit, &statement.tokens[5])?;
        let default = self.parse_physical(unit, &statement.tokens[6])?;
        let value_token = &statement.tokens[7];
        let value = self.parse_physical(unit, value_token)?;
        let modulatable = match statement.tokens.get(8).map(|token| token.value.as_str()) {
            None => false,
            Some("modulatable") => true,
            Some(other) => {
                self.token_error(
                    "CRP142",
                    format!("unknown parameter capability `{other}`"),
                    &statement.tokens[8],
                );
                false
            }
        };
        Some(Parameter {
            key: statement.tokens[1].value.clone(),
            name: statement.tokens[2].value.clone(),
            unit,
            minimum,
            maximum,
            default,
            value,
            modulatable,
            value_site: self.site(value_token),
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_faceplate(&mut self, statement: &Statement) -> Vec<FaceplateControl> {
        if statement.tokens.len() != 1 {
            self.error_at("CRP150", "Faceplate syntax is `faceplate {`", statement);
        }
        let mut controls = Vec::new();
        for child in &statement.children {
            if child.keyword() != Some("control") {
                self.unknown("Faceplate", child.keyword().unwrap_or(""), child);
                continue;
            }
            if child.tokens.len() != 6
                || child.tokens[2].kind != TokenKind::String
                || child.tokens[3].kind != TokenKind::String
                || child.tokens[4].value != "maps"
            {
                self.error_at(
                    "CRP151",
                    "control syntax is `control key \"Label\" \"Explanation\" maps parameter`",
                    child,
                );
                continue;
            }
            controls.push(FaceplateControl {
                key: child.tokens[1].value.clone(),
                label: child.tokens[2].value.clone(),
                explanation: child.tokens[3].value.clone(),
                parameter: child.tokens[5].value.clone(),
                location: child.location(self.file, self.source),
            });
        }
        controls
    }

    fn parse_graph(&mut self, statement: &Statement) -> Option<Graph> {
        let max_nodes = self
            .single_argument(statement, "CRP160", "graph syntax is `graph max_nodes {`")
            .and_then(|token| self.parse_u16_token(token, "max_nodes"))?;
        let mut nodes = Vec::new();
        let mut connections = Vec::new();
        for child in &statement.children {
            match child.keyword() {
                Some("node") => {
                    if let Some(node) = self.parse_node(child) {
                        nodes.push(node);
                    }
                }
                Some("connect") => {
                    if child.tokens.len() != 4 || child.tokens[2].value != "->" {
                        self.error_at(
                            "CRP161",
                            "connection syntax is `connect node.port -> node.port`",
                            child,
                        );
                        continue;
                    }
                    let from = self.parse_endpoint(&child.tokens[1]);
                    let to = self.parse_endpoint(&child.tokens[3]);
                    if let (Some(from), Some(to)) = (from, to) {
                        connections.push(Connection {
                            from,
                            to,
                            location: child.location(self.file, self.source),
                        });
                    }
                }
                Some(keyword) => self.unknown("graph", keyword, child),
                None => {}
            }
        }
        Some(Graph {
            max_nodes,
            nodes,
            connections,
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_node(&mut self, statement: &Statement) -> Option<GraphNode> {
        if statement.tokens.len() < 3 || !(statement.tokens.len() - 3).is_multiple_of(2) {
            self.error_at(
                "CRP170",
                "node syntax is `node id kind [setting value ...]`",
                statement,
            );
            return None;
        }
        let kind = match NodeKind::from_str(&statement.tokens[2].value) {
            Ok(kind) => kind,
            Err(error) => {
                self.token_error("CRP171", error.to_string(), &statement.tokens[2]);
                return None;
            }
        };
        let mut settings = BTreeMap::new();
        for pair in statement.tokens[3..].chunks_exact(2) {
            let key = pair[0].value.clone();
            let value = if let Some(parameter) = pair[1].value.strip_prefix('$') {
                SettingValue::Parameter(parameter.to_owned())
            } else if let Ok(number) = pair[1].value.parse::<f64>() {
                SettingValue::Number(number)
            } else {
                SettingValue::Text(pair[1].value.clone())
            };
            if settings.insert(key.clone(), value).is_some() {
                self.token_error(
                    "CRP172",
                    format!("duplicate node setting `{key}`"),
                    &pair[0],
                );
            }
        }
        Some(GraphNode {
            id: statement.tokens[1].value.clone(),
            kind,
            settings,
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_pattern(&mut self, statement: &Statement, meter: Meter) -> Option<Pattern> {
        if statement.tokens.len() != 3 {
            self.error_at("CRP180", "pattern syntax is `pattern id span {`", statement);
            return None;
        }
        let span = self.parse_time(&statement.tokens[2], meter)?;
        let mut events = Vec::new();
        for child in &statement.children {
            match child.keyword() {
                Some("note") => {
                    if child.tokens.len() != 5 {
                        self.error_at(
                            "CRP181",
                            "note syntax is `note at pitch duration velocity`",
                            child,
                        );
                        continue;
                    }
                    let at = self.parse_time(&child.tokens[1], meter);
                    let pitch = match parse_pitch(&child.tokens[2].value) {
                        Ok(pitch) => Some(pitch),
                        Err(error) => {
                            self.token_error("CRP182", error.to_string(), &child.tokens[2]);
                            None
                        }
                    };
                    let duration = self.parse_time(&child.tokens[3], meter);
                    let velocity = self.parse_ratio(&child.tokens[4]);
                    if let (Some(at), Some(pitch), Some(duration), Some(velocity)) =
                        (at, pitch, duration, velocity)
                    {
                        events.push(PatternEvent::Note {
                            at,
                            pitch,
                            duration,
                            velocity,
                            site: ValueSite::new(
                                self.file.to_path_buf(),
                                child.start,
                                child.end,
                                self.source[child.start..child.end].to_owned(),
                            ),
                            at_site: self.site(&child.tokens[1]),
                            pitch_site: self.site(&child.tokens[2]),
                            duration_site: self.site(&child.tokens[3]),
                            velocity_site: self.site(&child.tokens[4]),
                        });
                    }
                }
                Some("hit") => {
                    if child.tokens.len() != 4 {
                        self.error_at(
                            "CRP183",
                            "hit syntax is `hit at kick|snare|hat|clap velocity`",
                            child,
                        );
                        continue;
                    }
                    let at = self.parse_time(&child.tokens[1], meter);
                    let voice = match DrumVoice::from_str(&child.tokens[2].value) {
                        Ok(voice) => Some(voice),
                        Err(error) => {
                            self.token_error("CRP184", error.to_string(), &child.tokens[2]);
                            None
                        }
                    };
                    let velocity = self.parse_ratio(&child.tokens[3]);
                    if let (Some(at), Some(voice), Some(velocity)) = (at, voice, velocity) {
                        events.push(PatternEvent::Hit {
                            at,
                            voice,
                            velocity,
                            site: ValueSite::new(
                                self.file.to_path_buf(),
                                child.start,
                                child.end,
                                self.source[child.start..child.end].to_owned(),
                            ),
                            at_site: self.site(&child.tokens[1]),
                            voice_site: self.site(&child.tokens[2]),
                            velocity_site: self.site(&child.tokens[3]),
                        });
                    }
                }
                Some(keyword) => self.unknown("Pattern", keyword, child),
                None => {}
            }
        }
        Some(Pattern {
            id: statement.tokens[1].value.clone(),
            span,
            events,
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_track(&mut self, statement: &Statement, meter: Meter) -> Option<Track> {
        if statement.tokens.len() != 9
            || statement.tokens[2].kind != TokenKind::String
            || statement.tokens[3].value != "instrument"
            || statement.tokens[5].value != "level"
            || statement.tokens[7].value != "pan"
        {
            self.error_at(
                "CRP190",
                "track syntax is `track id \"Name\" instrument id level -6db pan 0 {`",
                statement,
            );
            return None;
        }
        let level_db = self.parse_physical(Unit::Decibels, &statement.tokens[6])?;
        let pan = self.parse_physical(Unit::Pan, &statement.tokens[8])?;
        let mut clips = Vec::new();
        for child in &statement.children {
            if child.keyword() != Some("clip") {
                self.unknown("Track", child.keyword().unwrap_or(""), child);
                continue;
            }
            if child.tokens.len() != 10
                || child.tokens[2].value != "pattern"
                || child.tokens[4].value != "start"
                || child.tokens[6].value != "duration"
                || child.tokens[8].value != "mode"
            {
                self.error_at(
                    "CRP191",
                    "clip syntax is `clip id pattern id start time duration time mode once|loop`",
                    child,
                );
                continue;
            }
            let start = self.parse_time(&child.tokens[5], meter);
            let duration = self.parse_time(&child.tokens[7], meter);
            let mode = match ClipMode::from_str(&child.tokens[9].value) {
                Ok(mode) => Some(mode),
                Err(error) => {
                    self.token_error("CRP192", error.to_string(), &child.tokens[9]);
                    None
                }
            };
            if let (Some(start), Some(duration), Some(mode)) = (start, duration, mode) {
                clips.push(Clip {
                    id: child.tokens[1].value.clone(),
                    pattern: child.tokens[3].value.clone(),
                    start,
                    duration,
                    mode,
                    location: child.location(self.file, self.source),
                });
            }
        }
        Some(Track {
            id: statement.tokens[1].value.clone(),
            name: statement.tokens[2].value.clone(),
            instrument: statement.tokens[4].value.clone(),
            level_db,
            pan,
            clips,
            level_site: self.site(&statement.tokens[6]),
            pan_site: self.site(&statement.tokens[8]),
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_id_value(&mut self, statement: &Statement, owner: &str) -> Option<String> {
        self.single_argument(
            statement,
            "CRP200",
            format!("{owner} id requires one value"),
        )
        .map(|token| token.value.clone())
    }

    fn parse_u64_value(&mut self, statement: &Statement, name: &str) -> Option<u64> {
        self.single_argument(statement, "CRP201", format!("{name} requires one integer"))
            .and_then(|token| match token.value.parse() {
                Ok(value) => Some(value),
                Err(_) => {
                    self.token_error(
                        "CRP202",
                        format!("{name} must be a non-negative integer"),
                        token,
                    );
                    None
                }
            })
    }

    fn parse_u32_value(&mut self, statement: &Statement, name: &str) -> Option<u32> {
        self.single_argument(statement, "CRP203", format!("{name} requires one integer"))
            .and_then(|token| match token.value.parse() {
                Ok(value) => Some(value),
                Err(_) => {
                    self.token_error(
                        "CRP204",
                        format!("{name} must be a supported integer"),
                        token,
                    );
                    None
                }
            })
    }

    fn parse_u16_value(&mut self, statement: &Statement, name: &str) -> Option<u16> {
        self.single_argument(statement, "CRP205", format!("{name} requires one integer"))
            .and_then(|token| self.parse_u16_token(token, name))
    }

    fn parse_u16_token(&mut self, token: &Token, name: &str) -> Option<u16> {
        match token.value.parse() {
            Ok(value) => Some(value),
            Err(_) => {
                self.token_error(
                    "CRP206",
                    format!("{name} must be an integer from 0 to 65535"),
                    token,
                );
                None
            }
        }
    }

    fn parse_tempo(&mut self, token: &Token) -> Option<f64> {
        let raw = token.value.strip_suffix("bpm").unwrap_or(&token.value);
        match raw.parse::<f64>() {
            Ok(value) if value.is_finite() && value > 0.0 => Some(value),
            _ => {
                self.token_error(
                    "CRP210",
                    "tempo must be finite and greater than zero",
                    token,
                );
                None
            }
        }
    }

    fn parse_time(&mut self, token: &Token, meter: Meter) -> Option<MusicalTime> {
        match parse_musical_time(&token.value, meter) {
            Ok(value) => Some(value),
            Err(error) => {
                self.token_error("CRP211", error.to_string(), token);
                None
            }
        }
    }

    fn parse_physical(&mut self, unit: Unit, token: &Token) -> Option<f64> {
        match unit.parse_value(&token.value) {
            Ok(value) => Some(value),
            Err(error) => {
                self.token_error("CRP212", error.to_string(), token);
                None
            }
        }
    }

    fn parse_ratio(&mut self, token: &Token) -> Option<f64> {
        self.parse_physical(Unit::Ratio, token)
    }

    fn parse_endpoint(&mut self, token: &Token) -> Option<Endpoint> {
        match Endpoint::from_str(&token.value) {
            Ok(endpoint) => Some(endpoint),
            Err(error) => {
                self.token_error("CRP213", error.to_string(), token);
                None
            }
        }
    }

    fn single_argument<'a, M: Into<String>>(
        &mut self,
        statement: &'a Statement,
        code: &str,
        message: M,
    ) -> Option<&'a Token> {
        if statement.tokens.len() != 2 {
            self.error_at(code, message, statement);
            None
        } else {
            statement.tokens.get(1)
        }
    }

    fn required<T>(
        &mut self,
        value: Option<T>,
        code: &str,
        message: &str,
        statement: &Statement,
    ) -> Option<T> {
        if value.is_none() {
            self.error_at(code, message, statement);
        }
        value
    }

    fn site(&self, token: &Token) -> ValueSite {
        ValueSite::new(
            self.file.to_path_buf(),
            token.start,
            token.end,
            token.raw(self.source).to_owned(),
        )
    }

    fn unknown(&mut self, owner: &str, keyword: &str, statement: &Statement) {
        self.error_at(
            "CRP220",
            format!("unknown `{keyword}` directive in {owner}"),
            statement,
        );
    }

    fn error_at<M: Into<String>>(&mut self, code: &str, message: M, statement: &Statement) {
        self.diagnostics
            .push(Diagnostic::error(code, message).at(statement.location(self.file, self.source)));
    }

    fn token_error<M: Into<String>>(&mut self, code: &str, message: M, token: &Token) {
        self.diagnostics.push(
            Diagnostic::error(code, message).at(SourceLocation::from_offsets(
                self.file.to_path_buf(),
                self.source,
                token.start,
                token.end,
            )),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT: &str = r#"crump 1
project "Parser Test" {
  id parser-test
  seed 42
  sample_rate 48000
  render_tail 2s
  song {
    tempo 120bpm
    meter 4/4
    end 2bar
    instrument lead "Lead" graph {
      polyphony 8
      parameter tone "Tone" hz 100hz 12000hz 2000hz 3000hz modulatable
      faceplate {
        control tone "Tone" "Brightness" maps tone
      }
      graph 12 {
        node osc oscillator waveform saw
        node env envelope attack 10ms
        node amp amplifier
        node pan pan position 0
        node out output
        connect note.pitch -> osc.pitch
        connect note.gate -> env.gate
        connect osc.audio -> amp.audio
        connect env.control -> amp.gain
        connect amp.audio -> pan.audio
        connect pan.audio -> out.audio
      }
    }
    pattern melody 1bar {
      note 0beat C4 1beat 0.8
    }
    track lead-track "Lead" instrument lead level -6db pan 0 {
      clip lead-clip pattern melody start 0bar duration 2bar mode loop
    }
  }
}
"#;

    #[test]
    fn parses_minimal_project_and_edit_sites() {
        let document = parse_project(Path::new("project.crump"), PROJECT);
        for diagnostic in document.diagnostics.as_slice() {
            eprintln!("{diagnostic}");
        }
        assert!(!document.diagnostics.has_errors());
        let project = document.project.unwrap();
        assert_eq!(project.song.instruments[0].parameters[0].value, 3_000.0);
        assert_eq!(project.song.patterns[0].events.len(), 1);
        let site = &project.song.instruments[0].parameters[0].value_site;
        assert_eq!(&PROJECT[site.start..site.end], "3000hz");
    }

    #[test]
    fn recovers_multiple_diagnostics() {
        let document = parse_project(Path::new("project.crump"), "crump 2\nwat nope\n");
        assert!(document.diagnostics.as_slice().len() >= 3);
    }
}
