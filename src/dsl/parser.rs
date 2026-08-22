//! Semantic parser for Crump's deliberately small first-release language.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::{
    diagnostic::{Diagnostic, Diagnostics, SourceLocation},
    model::{
        AutomationInterpolation, AutomationLane, AutomationPoint, ChannelRef, ChannelStrip, Clip,
        ClipMode, ClipSites, Connection, ControlPolarity, DrumVoice, Endpoint, EnvelopeDetector,
        EnvelopeFollowerRoute, FaceplateControl, FormantPolicy, GeneratedAssetProvenance, Graph,
        GraphNode, GrooveSites, GrooveTemplate, Group, Instrument, InstrumentKind, LfoWaveform,
        Locator, Master, MediaAsset, ModulationOperator, ModulationRate, ModulationRoute,
        ModulationScope, NodeKind, OutputTarget, Panner, Parameter, ParameterLock,
        ParameterLockSites, ParameterTarget, Pattern, PatternEvent, PatternVariation,
        PatternVariationSites, Processor, ProcessorKind, ProcessorSetting, Project, Return,
        SOURCE_VERSION, SampleDirection, SampleEventSites, SampleLoopRegion, SampleMapping,
        SamplePlayback, SampleTransformQuality, SamplerDefinition, Send, SettingValue,
        SidechainRoute, Song, TapPoint, Track, Unit, ValueSite, parse_pitch,
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
        let mut assets = Vec::new();
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
                Some("asset") => {
                    if let Some(asset) = self.parse_media_asset(child) {
                        assets.push(asset);
                    }
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
            assets,
            song,
            imports: Vec::new(),
        })
    }

    fn parse_media_asset(&mut self, statement: &Statement) -> Option<MediaAsset> {
        if statement.tokens.len() != 7
            || statement.tokens[2].kind != TokenKind::String
            || statement.tokens[3].value != "wav"
            || statement.tokens[4].kind != TokenKind::String
            || statement.tokens[5].value != "sha256"
            || statement.tokens[6].kind != TokenKind::String
        {
            self.error_at(
                "CRP230",
                "asset syntax is `asset id \"Name\" wav \"media/file.wav\" sha256 \"lowercase-hex\"` with an optional generated provenance block",
                statement,
            );
            return None;
        }
        let generated = if statement.children.is_empty() {
            None
        } else if statement.children.len() == 1
            && statement.children[0].keyword() == Some("generated")
        {
            self.parse_generated_asset(&statement.children[0])
        } else {
            self.error_at(
                "CRP231",
                "an asset may contain only one `generated { ... }` provenance block",
                statement,
            );
            return None;
        };
        Some(MediaAsset {
            id: statement.tokens[1].value.clone(),
            name: statement.tokens[2].value.clone(),
            path: PathBuf::from(&statement.tokens[4].value),
            sha256: statement.tokens[6].value.clone(),
            generated,
            path_site: self.site(&statement.tokens[4]),
            sha256_site: self.site(&statement.tokens[6]),
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_generated_asset(&mut self, statement: &Statement) -> Option<GeneratedAssetProvenance> {
        if !(1..=2).contains(&statement.tokens.len()) || statement.children.is_empty() {
            self.error_at(
                "CRP231",
                "generated provenance syntax is `generated [bounce|sample_transform] { ... }`",
                statement,
            );
            return None;
        }
        match statement.tokens.get(1).map(|token| token.value.as_str()) {
            None | Some("bounce") => {}
            Some("sample_transform") => return self.parse_sample_transform_provenance(statement),
            Some(kind) => {
                self.error_at(
                    "CRP231",
                    format!("unknown generated provenance kind `{kind}`"),
                    statement,
                );
                return None;
            }
        }
        let mut revision = None;
        let mut owner = None;
        let mut tap = None;
        let mut range = None;
        let mut tail = None;
        let mut format = None;
        let mut algorithm = None;
        let mut build = None;
        let mut seen = BTreeSet::new();
        for child in &statement.children {
            if let Some(
                keyword @ ("revision" | "owner" | "tap" | "range" | "tail" | "format" | "algorithm"
                | "build"),
            ) = child.keyword()
                && !seen.insert(keyword)
            {
                self.error_at(
                    "CRP231",
                    format!("`{keyword}` is declared more than once in generated provenance"),
                    child,
                );
                continue;
            }
            match child.keyword() {
                Some("revision") => {
                    revision = self
                        .single_argument(child, "CRP231", "revision requires one identity")
                        .map(|token| token.value.clone());
                }
                Some("owner") => {
                    owner = self
                        .single_argument(
                            child,
                            "CRP231",
                            "owner requires master or one channel identity",
                        )
                        .map(|token| token.value.clone());
                }
                Some("tap") => {
                    tap = self
                        .single_argument(child, "CRP231", "tap requires one identity")
                        .map(|token| token.value.clone());
                }
                Some("range") if child.tokens.len() == 3 => {
                    let start = self.parse_u64_token(&child.tokens[1], "generated range start");
                    let end = self.parse_u64_token(&child.tokens[2], "generated range end");
                    range = start.zip(end);
                }
                Some("tail") => {
                    tail = self
                        .single_argument(child, "CRP231", "tail requires one frame count")
                        .and_then(|token| self.parse_u64_token(token, "generated tail"));
                }
                Some("format") => {
                    format = self
                        .single_argument(child, "CRP231", "format requires one identity")
                        .map(|token| token.value.clone());
                }
                Some("algorithm") => {
                    algorithm = self
                        .single_argument(child, "CRP231", "algorithm requires one identity")
                        .map(|token| token.value.clone());
                }
                Some("build") => {
                    build = self
                        .single_argument(child, "CRP231", "build requires one identity")
                        .map(|token| token.value.clone());
                }
                Some("range") => {
                    self.error_at("CRP231", "range syntax is `range START END`", child);
                }
                Some(keyword) => self.unknown("generated asset provenance", keyword, child),
                None => {}
            }
        }
        let (start_frame, end_frame) = self.required(
            range,
            "CRP231",
            "generated provenance is missing `range START END`",
            statement,
        )?;
        Some(GeneratedAssetProvenance::Bounce {
            source_revision: self.required(
                revision,
                "CRP231",
                "generated provenance is missing `revision`",
                statement,
            )?,
            owner: self.required(
                owner,
                "CRP231",
                "generated provenance is missing `owner`",
                statement,
            )?,
            tap: self.required(
                tap,
                "CRP231",
                "generated provenance is missing `tap`",
                statement,
            )?,
            start_frame,
            end_frame,
            tail_frames: self.required(
                tail,
                "CRP231",
                "generated provenance is missing `tail`",
                statement,
            )?,
            format: self.required(
                format,
                "CRP231",
                "generated provenance is missing `format`",
                statement,
            )?,
            algorithm: self.required(
                algorithm,
                "CRP231",
                "generated provenance is missing `algorithm`",
                statement,
            )?,
            build: self.required(
                build,
                "CRP231",
                "generated provenance is missing `build`",
                statement,
            )?,
        })
    }

    fn parse_sample_transform_provenance(
        &mut self,
        statement: &Statement,
    ) -> Option<GeneratedAssetProvenance> {
        let mut revision = None;
        let mut source_asset = None;
        let mut source_sha256 = None;
        let mut source_range = None;
        let mut output_frames = None;
        let mut pitch_semitones = None;
        let mut formants = None;
        let mut quality = None;
        let mut format = None;
        let mut algorithm = None;
        let mut deterministic_domain = None;
        let mut build = None;
        let mut seen = BTreeSet::new();
        for child in &statement.children {
            if let Some(
                keyword @ ("revision" | "source_asset" | "source_sha256" | "source_range"
                | "output_frames" | "pitch" | "formants" | "quality" | "format"
                | "algorithm" | "domain" | "build"),
            ) = child.keyword()
                && !seen.insert(keyword)
            {
                self.error_at(
                    "CRP232",
                    format!(
                        "`{keyword}` is declared more than once in sample-transform provenance"
                    ),
                    child,
                );
                continue;
            }
            match child.keyword() {
                Some("revision") => {
                    revision = self
                        .single_argument(child, "CRP232", "revision requires one identity")
                        .map(|token| token.value.clone());
                }
                Some("source_asset") => {
                    source_asset = self
                        .single_argument(child, "CRP232", "source_asset requires one asset ID")
                        .map(|token| token.value.clone());
                }
                Some("source_sha256") => {
                    source_sha256 = self
                        .single_argument(child, "CRP232", "source_sha256 requires one hash")
                        .map(|token| token.value.clone());
                }
                Some("source_range") if child.tokens.len() == 3 => {
                    let start = self
                        .parse_u64_token(&child.tokens[1], "sample-transform source range start");
                    let end =
                        self.parse_u64_token(&child.tokens[2], "sample-transform source range end");
                    source_range = start.zip(end);
                }
                Some("output_frames") => {
                    output_frames = self
                        .single_argument(
                            child,
                            "CRP232",
                            "output_frames requires one exact frame count",
                        )
                        .and_then(|token| {
                            self.parse_u64_token(token, "sample-transform output frames")
                        });
                }
                Some("pitch") => {
                    pitch_semitones = self
                        .single_argument(child, "CRP232", "pitch requires one semitone value")
                        .and_then(|token| self.parse_physical(Unit::Semitones, token));
                }
                Some("formants") => {
                    formants = self
                        .single_argument(child, "CRP232", "formants requires shift or preserve")
                        .and_then(|token| match token.value.parse::<FormantPolicy>() {
                            Ok(value) => Some(value),
                            Err(error) => {
                                self.error_at("CRP232", error.to_string(), child);
                                None
                            }
                        });
                }
                Some("quality") => {
                    quality = self
                        .single_argument(child, "CRP232", "quality requires effect_v1")
                        .and_then(
                            |token| match token.value.parse::<SampleTransformQuality>() {
                                Ok(value) => Some(value),
                                Err(error) => {
                                    self.error_at("CRP232", error.to_string(), child);
                                    None
                                }
                            },
                        );
                }
                Some("format") => {
                    format = self
                        .single_argument(child, "CRP232", "format requires one identity")
                        .map(|token| token.value.clone());
                }
                Some("algorithm") => {
                    algorithm = self
                        .single_argument(child, "CRP232", "algorithm requires one identity")
                        .map(|token| token.value.clone());
                }
                Some("domain") => {
                    deterministic_domain = self
                        .single_argument(child, "CRP232", "domain requires one identity")
                        .map(|token| token.value.clone());
                }
                Some("build") => {
                    build = self
                        .single_argument(child, "CRP232", "build requires one identity")
                        .map(|token| token.value.clone());
                }
                Some("source_range") => self.error_at(
                    "CRP232",
                    "source_range syntax is `source_range START END`",
                    child,
                ),
                Some(keyword) => self.unknown("sample-transform provenance", keyword, child),
                None => {}
            }
        }
        let (source_start_frame, source_end_frame) = self.required(
            source_range,
            "CRP232",
            "sample-transform provenance is missing `source_range START END`",
            statement,
        )?;
        Some(GeneratedAssetProvenance::SampleTransform {
            source_revision: self.required(
                revision,
                "CRP232",
                "sample-transform provenance is missing `revision`",
                statement,
            )?,
            source_asset: self.required(
                source_asset,
                "CRP232",
                "sample-transform provenance is missing `source_asset`",
                statement,
            )?,
            source_sha256: self.required(
                source_sha256,
                "CRP232",
                "sample-transform provenance is missing `source_sha256`",
                statement,
            )?,
            source_start_frame,
            source_end_frame,
            output_frames: self.required(
                output_frames,
                "CRP232",
                "sample-transform provenance is missing `output_frames`",
                statement,
            )?,
            pitch_semitones: self.required(
                pitch_semitones,
                "CRP232",
                "sample-transform provenance is missing `pitch`",
                statement,
            )?,
            formants: self.required(
                formants,
                "CRP232",
                "sample-transform provenance is missing `formants`",
                statement,
            )?,
            quality: self.required(
                quality,
                "CRP232",
                "sample-transform provenance is missing `quality`",
                statement,
            )?,
            format: self.required(
                format,
                "CRP232",
                "sample-transform provenance is missing `format`",
                statement,
            )?,
            algorithm: self.required(
                algorithm,
                "CRP232",
                "sample-transform provenance is missing `algorithm`",
                statement,
            )?,
            deterministic_domain: self.required(
                deterministic_domain,
                "CRP232",
                "sample-transform provenance is missing `domain`",
                statement,
            )?,
            build: self.required(
                build,
                "CRP232",
                "sample-transform provenance is missing `build`",
                statement,
            )?,
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
        let mut groove_statements = Vec::new();
        let mut track_statements = Vec::new();
        let mut group_statements = Vec::new();
        let mut return_statements = Vec::new();
        let mut master_statement = None;
        let mut sidechain_statements = Vec::new();
        let mut locator_statements = Vec::new();
        let mut automation_statements = Vec::new();
        let mut modulation_statements = Vec::new();
        let mut follower_statements = Vec::new();

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
                Some("groove") => groove_statements.push(child),
                Some("track") => track_statements.push(child),
                Some("group") => group_statements.push(child),
                Some("return") => return_statements.push(child),
                Some("master") => {
                    if master_statement.replace(child).is_some() {
                        self.error_at("CRP128", "Song contains more than one Master", child);
                    }
                }
                Some("sidechain") => sidechain_statements.push(child),
                Some("locator") => locator_statements.push(child),
                Some("automation") => automation_statements.push(child),
                Some("modulation") => modulation_statements.push(child),
                Some("follower") => follower_statements.push(child),
                Some(keyword) => self.unknown("Song", keyword, child),
                None => {}
            }
        }

        let tempo_bpm = self.required(tempo_bpm, "CRP125", "Song is missing `tempo`", statement)?;
        let meter = self.required(meter, "CRP126", "Song is missing `meter`", statement)?;
        let end_token = self.required(end_token, "CRP127", "Song is missing `end`", statement)?;
        let end = self.parse_time(end_token, meter)?;
        let patterns = pattern_statements
            .into_iter()
            .filter_map(|pattern| self.parse_pattern(pattern, meter))
            .collect();
        let grooves = groove_statements
            .into_iter()
            .filter_map(|groove| self.parse_groove(groove, meter))
            .collect();
        let tracks = track_statements
            .into_iter()
            .filter_map(|track| self.parse_track(track, meter))
            .collect();
        let groups = group_statements
            .into_iter()
            .filter_map(|group| self.parse_group(group))
            .collect();
        let returns = return_statements
            .into_iter()
            .filter_map(|channel| self.parse_return(channel))
            .collect();
        let master = master_statement.and_then(|master| self.parse_master(master));
        let sidechains = sidechain_statements
            .into_iter()
            .filter_map(|route| self.parse_sidechain(route))
            .collect();
        let locators = locator_statements
            .into_iter()
            .filter_map(|locator| self.parse_locator(locator, meter))
            .collect();
        let automations = automation_statements
            .into_iter()
            .filter_map(|automation| self.parse_automation(automation, meter))
            .collect();
        let modulations = modulation_statements
            .into_iter()
            .filter_map(|modulation| self.parse_modulation(modulation, meter))
            .collect();
        let envelope_followers = follower_statements
            .into_iter()
            .filter_map(|follower| self.parse_envelope_follower(follower))
            .collect();
        Some(Song {
            tempo_bpm,
            meter,
            end,
            end_site: self.site(end_token),
            instruments,
            patterns,
            grooves,
            tracks,
            groups,
            returns,
            master,
            sidechains,
            locators,
            automations,
            modulations,
            envelope_followers,
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_instrument(&mut self, statement: &Statement) -> Option<Instrument> {
        if statement.tokens.len() != 4 || statement.tokens[2].kind != TokenKind::String {
            self.error_at(
                "CRP130",
                "instrument syntax is `instrument id \"Name\" graph|stock_drums|sampler {`",
                statement,
            );
            return None;
        }
        let id = statement.tokens[1].value.clone();
        let name = statement.tokens[2].value.clone();
        let kind = match statement.tokens[3].value.as_str() {
            "graph" => InstrumentKind::Graph,
            "stock_drums" => InstrumentKind::StockDrums,
            "sampler" => InstrumentKind::Sampler,
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
        let mut sample_mappings = Vec::new();
        let mut post_processors = Vec::new();
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
                Some("mapping") => {
                    if let Some(mapping) = self.parse_sample_mapping(child) {
                        sample_mappings.push(mapping);
                    }
                }
                Some("processor") => {
                    if let Some(processor) = self.parse_processor(child) {
                        post_processors.push(processor);
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
            sampler: (kind == InstrumentKind::Sampler).then(|| SamplerDefinition {
                mappings: sample_mappings,
                location: statement.location(self.file, self.source),
            }),
            post_processors,
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_sample_mapping(&mut self, statement: &Statement) -> Option<SampleMapping> {
        if statement.tokens.len() != 9
            || statement.tokens[2].value != "asset"
            || statement.tokens[4].value != "keys"
            || statement.tokens[7].value != "root"
        {
            self.error_at(
                "CRP134",
                "mapping syntax is `mapping ID asset ASSET keys LOW HIGH root ROOT {`",
                statement,
            );
            return None;
        }
        let low_key = self.parse_pitch_token(&statement.tokens[5])?;
        let high_key = self.parse_pitch_token(&statement.tokens[6])?;
        let root_key = self.parse_pitch_token(&statement.tokens[8])?;
        let mut region = None;
        let mut playback = None;
        let mut gain = None;
        let mut pan = None;
        let mut envelope = None;
        let mut fade = None;
        let mut direction = None;
        let mut loop_region = None;
        let mut choke_group = None;
        for child in &statement.children {
            match child.keyword() {
                Some("region") if child.tokens.len() == 3 => {
                    let start = self.parse_u64_token(&child.tokens[1], "region start");
                    let end = self.parse_u64_token(&child.tokens[2], "region end");
                    if let (Some(start), Some(end)) = (start, end) {
                        region = Some((start, end, self.site(&child.tokens[1]), self.site(&child.tokens[2])));
                    }
                }
                Some("playback") => {
                    playback = self
                        .single_argument(child, "CRP135", "playback requires one_shot or gated")
                        .and_then(|token| match SamplePlayback::from_str(&token.value) {
                            Ok(value) => Some(value),
                            Err(error) => {
                                self.token_error("CRP135", error.to_string(), token);
                                None
                            }
                        });
                }
                Some("gain") => {
                    gain = self
                        .single_argument(child, "CRP136", "gain requires one dB value")
                        .and_then(|token| self.parse_physical(Unit::Decibels, token).map(|value| (value, self.site(token))));
                }
                Some("pan") => {
                    pan = self
                        .single_argument(child, "CRP137", "pan requires one value")
                        .and_then(|token| self.parse_physical(Unit::Pan, token).map(|value| (value, self.site(token))));
                }
                Some("envelope") if child.tokens.len() == 3 => {
                    let attack = self.parse_physical(Unit::Seconds, &child.tokens[1]);
                    let release = self.parse_physical(Unit::Seconds, &child.tokens[2]);
                    if let (Some(attack), Some(release)) = (attack, release) {
                        envelope = Some((attack, release));
                    }
                }
                Some("fade") if child.tokens.len() == 3 => {
                    let fade_in = self.parse_u32_token(&child.tokens[1], "fade in");
                    let fade_out = self.parse_u32_token(&child.tokens[2], "fade out");
                    if let (Some(fade_in), Some(fade_out)) = (fade_in, fade_out) {
                        fade = Some((fade_in, fade_out, self.site(&child.tokens[1]), self.site(&child.tokens[2])));
                    }
                }
                Some("direction") => {
                    direction = self
                        .single_argument(child, "CRP138", "direction requires forward or reverse")
                        .and_then(|token| match SampleDirection::from_str(&token.value) {
                            Ok(value) => Some((value, self.site(token))),
                            Err(error) => {
                                self.token_error("CRP138", error.to_string(), token);
                                None
                            }
                        });
                }
                Some("loop") if child.tokens.len() == 5 && child.tokens[3].value == "crossfade" => {
                    let start_frame = self.parse_u64_token(&child.tokens[1], "loop start");
                    let end_frame = self.parse_u64_token(&child.tokens[2], "loop end");
                    let crossfade_frames = self.parse_u32_token(&child.tokens[4], "loop crossfade");
                    if let (Some(start_frame), Some(end_frame), Some(crossfade_frames)) =
                        (start_frame, end_frame, crossfade_frames)
                    {
                        loop_region = Some((
                            SampleLoopRegion {
                                start_frame,
                                end_frame,
                                crossfade_frames,
                            },
                            ValueSite::new(
                                self.file.to_path_buf(),
                                child.start,
                                child.end,
                                self.source[child.start..child.end].to_owned(),
                            ),
                        ));
                    }
                }
                Some("choke") => {
                    choke_group = self
                        .single_argument(child, "CRP138", "choke requires one group ID")
                        .map(|token| token.value.clone());
                }
                Some("region" | "envelope" | "fade" | "loop") => self.error_at(
                    "CRP139",
                    "mapping directives include `region START END`, `envelope ATTACK RELEASE`, `fade IN OUT`, and `loop START END crossfade FRAMES`",
                    child,
                ),
                Some(keyword) => self.unknown("sample mapping", keyword, child),
                None => {}
            }
        }
        let (start_frame, end_frame, start_site, end_site) = self.required(
            region,
            "CRP134",
            "sample mapping is missing `region START END`",
            statement,
        )?;
        let playback = self.required(
            playback,
            "CRP135",
            "sample mapping is missing `playback`",
            statement,
        )?;
        let (gain_db, gain_site) = self.required(
            gain,
            "CRP136",
            "sample mapping is missing `gain`",
            statement,
        )?;
        let (pan, pan_site) =
            self.required(pan, "CRP137", "sample mapping is missing `pan`", statement)?;
        let (attack_seconds, release_seconds) = self.required(
            envelope,
            "CRP139",
            "sample mapping is missing `envelope ATTACK RELEASE`",
            statement,
        )?;
        let (fade_in_frames, fade_out_frames, fade_in_site, fade_out_site) = self.required(
            fade,
            "CRP139",
            "sample mapping is missing `fade IN OUT`",
            statement,
        )?;
        let (direction, direction_site) = direction
            .map(|(direction, site)| (direction, Some(site)))
            .unwrap_or((SampleDirection::Forward, None));
        let (loop_region, loop_site) = loop_region
            .map(|(loop_region, site)| (Some(loop_region), Some(site)))
            .unwrap_or((None, None));
        Some(SampleMapping {
            id: statement.tokens[1].value.clone(),
            asset: statement.tokens[3].value.clone(),
            low_key,
            high_key,
            root_key,
            start_frame,
            end_frame,
            playback,
            direction,
            loop_region,
            gain_db,
            pan,
            attack_seconds,
            release_seconds,
            fade_in_frames,
            fade_out_frames,
            choke_group,
            location: statement.location(self.file, self.source),
            start_site,
            end_site,
            gain_site,
            pan_site,
            fade_in_site,
            fade_out_site,
            direction_site,
            loop_site,
        })
    }

    fn parse_parameter(&mut self, statement: &Statement) -> Option<Parameter> {
        if statement.tokens.len() < 8 || statement.tokens[2].kind != TokenKind::String {
            self.error_at(
                "CRP140",
                "parameter syntax is `parameter key \"Name\" unit min max default value [automatable] [modulatable]`",
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
        let mut automatable = false;
        let mut modulatable = false;
        for token in &statement.tokens[8..] {
            let capability = match token.value.as_str() {
                "automatable" if !automatable => &mut automatable,
                "modulatable" if !modulatable => &mut modulatable,
                "automatable" | "modulatable" => {
                    self.token_error(
                        "CRP142",
                        format!("duplicate parameter capability `{}`", token.value),
                        token,
                    );
                    continue;
                }
                other => {
                    self.token_error(
                        "CRP142",
                        format!("unknown parameter capability `{other}`"),
                        token,
                    );
                    continue;
                }
            };
            *capability = true;
        }
        Some(Parameter {
            key: statement.tokens[1].value.clone(),
            name: statement.tokens[2].value.clone(),
            unit,
            minimum,
            maximum,
            default,
            value,
            automatable,
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
        if statement.tokens.len() == 5 && statement.tokens[3].value == "from" {
            return self.parse_pattern_variation(statement, meter);
        }
        if statement.tokens.len() != 3 {
            self.error_at("CRP180", "pattern syntax is `pattern id span {`", statement);
            return None;
        }
        let span = self.parse_time(&statement.tokens[2], meter)?;
        let mut events = Vec::new();
        let mut locks = Vec::new();
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
                Some("lock") => {
                    if child.tokens.len() != 8
                        || child.tokens[2].value != "at"
                        || child.tokens[4].value != "parameter"
                        || child.tokens[6].value != "value"
                    {
                        self.error_at(
                            "CRP235",
                            "Parameter lock syntax is `lock ID at TIME parameter PARAMETER value VALUE`",
                            child,
                        );
                        continue;
                    }
                    if let Some(at) = self.parse_time(&child.tokens[3], meter) {
                        locks.push(ParameterLock {
                            id: child.tokens[1].value.clone(),
                            at,
                            parameter: child.tokens[5].value.clone(),
                            value: child.tokens[7].value.clone(),
                            sites: ParameterLockSites {
                                at: self.site(&child.tokens[3]),
                                parameter: self.site(&child.tokens[5]),
                                value: self.site(&child.tokens[7]),
                            },
                            location: child.location(self.file, self.source),
                        });
                    }
                }
                Some("sample") => {
                    if child.tokens.len() < 20
                        || child.tokens[3].value != "mapping"
                        || child.tokens[5].value != "duration"
                        || child.tokens[7].value != "start"
                        || child.tokens[9].value != "end"
                        || child.tokens[11].value != "gain"
                        || child.tokens[13].value != "pan"
                        || child.tokens[15].value != "fade"
                        || child.tokens[18].value != "velocity"
                    {
                        self.error_at(
                            "CRP185",
                            "sample syntax is `sample ID AT mapping MAPPING duration TIME start FRAME end FRAME gain DB pan PAN fade IN OUT velocity VALUE [direction forward|reverse] [loop START END crossfade FRAMES]`",
                            child,
                        );
                        continue;
                    }
                    let mut suffix = 20;
                    let mut direction = None;
                    let mut direction_site = None;
                    if child
                        .tokens
                        .get(suffix)
                        .is_some_and(|token| token.value == "direction")
                    {
                        let Some(token) = child.tokens.get(suffix + 1) else {
                            self.error_at(
                                "CRP185",
                                "sample direction requires forward or reverse",
                                child,
                            );
                            continue;
                        };
                        direction = match SampleDirection::from_str(&token.value) {
                            Ok(value) => Some(value),
                            Err(error) => {
                                self.token_error("CRP185", error.to_string(), token);
                                continue;
                            }
                        };
                        direction_site = Some(self.site(token));
                        suffix += 2;
                    }
                    let mut loop_region = None;
                    let mut loop_site = None;
                    if child
                        .tokens
                        .get(suffix)
                        .is_some_and(|token| token.value == "loop")
                    {
                        if child.tokens.len() != suffix + 5
                            || child.tokens[suffix + 3].value != "crossfade"
                        {
                            self.error_at(
                                "CRP185",
                                "sample loop syntax is `loop START END crossfade FRAMES`",
                                child,
                            );
                            continue;
                        }
                        let start =
                            self.parse_u64_token(&child.tokens[suffix + 1], "sample loop start");
                        let end =
                            self.parse_u64_token(&child.tokens[suffix + 2], "sample loop end");
                        let crossfade = self
                            .parse_u32_token(&child.tokens[suffix + 4], "sample loop crossfade");
                        let (Some(start_frame), Some(end_frame), Some(crossfade_frames)) =
                            (start, end, crossfade)
                        else {
                            continue;
                        };
                        loop_region = Some(SampleLoopRegion {
                            start_frame,
                            end_frame,
                            crossfade_frames,
                        });
                        let start = child.tokens[suffix].start.saturating_sub(1);
                        loop_site = Some(ValueSite::new(
                            self.file.to_path_buf(),
                            start,
                            child.tokens[suffix + 4].end,
                            self.source[start..child.tokens[suffix + 4].end].to_owned(),
                        ));
                        suffix += 5;
                    }
                    if suffix != child.tokens.len() {
                        self.error_at(
                            "CRP185",
                            "sample suffixes must be `[direction forward|reverse] [loop START END crossfade FRAMES]`",
                            child,
                        );
                        continue;
                    }
                    let at = self.parse_time(&child.tokens[2], meter);
                    let duration = self.parse_time(&child.tokens[6], meter);
                    let start_frame = self.parse_u64_token(&child.tokens[8], "sample start");
                    let end_frame = self.parse_u64_token(&child.tokens[10], "sample end");
                    let gain_db = self.parse_physical(Unit::Decibels, &child.tokens[12]);
                    let pan = self.parse_physical(Unit::Pan, &child.tokens[14]);
                    let fade_in_frames = self.parse_u32_token(&child.tokens[16], "sample fade in");
                    let fade_out_frames =
                        self.parse_u32_token(&child.tokens[17], "sample fade out");
                    let velocity = self.parse_ratio(&child.tokens[19]);
                    if let (
                        Some(at),
                        Some(duration),
                        Some(start_frame),
                        Some(end_frame),
                        Some(gain_db),
                        Some(pan),
                        Some(fade_in_frames),
                        Some(fade_out_frames),
                        Some(velocity),
                    ) = (
                        at,
                        duration,
                        start_frame,
                        end_frame,
                        gain_db,
                        pan,
                        fade_in_frames,
                        fade_out_frames,
                        velocity,
                    ) {
                        events.push(PatternEvent::Sample {
                            id: child.tokens[1].value.clone(),
                            at,
                            mapping: child.tokens[4].value.clone(),
                            duration,
                            start_frame,
                            end_frame,
                            gain_db,
                            pan,
                            fade_in_frames,
                            fade_out_frames,
                            velocity,
                            direction,
                            loop_region,
                            sites: Box::new(SampleEventSites {
                                site: ValueSite::new(
                                    self.file.to_path_buf(),
                                    child.start,
                                    child.end,
                                    self.source[child.start..child.end].to_owned(),
                                ),
                                at: self.site(&child.tokens[2]),
                                duration: self.site(&child.tokens[6]),
                                start: self.site(&child.tokens[8]),
                                end: self.site(&child.tokens[10]),
                                gain: self.site(&child.tokens[12]),
                                pan: self.site(&child.tokens[14]),
                                fade_in: self.site(&child.tokens[16]),
                                fade_out: self.site(&child.tokens[17]),
                                velocity: self.site(&child.tokens[19]),
                                direction: direction_site,
                                loop_site,
                            }),
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
            span_site: self.site(&statement.tokens[2]),
            events,
            locks,
            variation: None,
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_pattern_variation(&mut self, statement: &Statement, meter: Meter) -> Option<Pattern> {
        let span = self.parse_time(&statement.tokens[2], meter)?;
        let mut rotate = None;
        let mut transpose = None;
        let mut velocity = None;
        let mut reverse = None;
        let mut probability = None;
        let mut seed = None;
        for directive in &statement.children {
            match directive.keyword() {
                Some("rotate") if directive.tokens.len() == 2 && rotate.is_none() => {
                    rotate = self
                        .parse_time(&directive.tokens[1], meter)
                        .map(|value| (value, self.site(&directive.tokens[1])));
                }
                Some("transpose") if directive.tokens.len() == 2 && transpose.is_none() => {
                    if let Some(value) = self.parse_physical(Unit::Semitones, &directive.tokens[1])
                    {
                        if value.fract() == 0.0
                            && value >= f64::from(i16::MIN)
                            && value <= f64::from(i16::MAX)
                        {
                            transpose = Some((value as i16, self.site(&directive.tokens[1])));
                        } else {
                            self.token_error(
                                "CRP233",
                                "Pattern Variation transpose must be a whole semitone value",
                                &directive.tokens[1],
                            );
                        }
                    }
                }
                Some("velocity") if directive.tokens.len() == 2 && velocity.is_none() => {
                    velocity = self
                        .parse_ratio(&directive.tokens[1])
                        .map(|value| (value, self.site(&directive.tokens[1])));
                }
                Some("reverse") if directive.tokens.len() == 2 && reverse.is_none() => {
                    reverse = self
                        .parse_bool(&directive.tokens[1], "Pattern Variation reverse")
                        .map(|value| (value, self.site(&directive.tokens[1])));
                }
                Some("probability") if directive.tokens.len() == 2 && probability.is_none() => {
                    probability = self
                        .parse_ratio(&directive.tokens[1])
                        .map(|value| (value, self.site(&directive.tokens[1])));
                }
                Some("seed") if directive.tokens.len() == 2 && seed.is_none() => {
                    seed = self
                        .parse_u64_token(&directive.tokens[1], "Pattern Variation seed")
                        .map(|value| (value, self.site(&directive.tokens[1])));
                }
                Some(keyword) => self.unknown("Pattern Variation", keyword, directive),
                None => {}
            }
        }
        let Some((rotate, rotate_site)) = rotate else {
            self.error_at(
                "CRP234",
                "Pattern Variation requires one `rotate TIME`",
                statement,
            );
            return None;
        };
        let Some((transpose_semitones, transpose_site)) = transpose else {
            self.error_at(
                "CRP234",
                "Pattern Variation requires one `transpose NST`",
                statement,
            );
            return None;
        };
        let Some((velocity_scale, velocity_site)) = velocity else {
            self.error_at(
                "CRP234",
                "Pattern Variation requires one `velocity RATIO`",
                statement,
            );
            return None;
        };
        let Some((reverse, reverse_site)) = reverse else {
            self.error_at(
                "CRP234",
                "Pattern Variation requires one `reverse true|false`",
                statement,
            );
            return None;
        };
        let Some((probability, probability_site)) = probability else {
            self.error_at(
                "CRP234",
                "Pattern Variation requires one `probability RATIO`",
                statement,
            );
            return None;
        };
        let Some((seed, seed_site)) = seed else {
            self.error_at(
                "CRP234",
                "Pattern Variation requires one `seed INTEGER`",
                statement,
            );
            return None;
        };
        Some(Pattern {
            id: statement.tokens[1].value.clone(),
            span,
            span_site: self.site(&statement.tokens[2]),
            events: Vec::new(),
            locks: Vec::new(),
            variation: Some(PatternVariation {
                source: statement.tokens[4].value.clone(),
                rotate,
                transpose_semitones,
                velocity_scale,
                reverse,
                probability,
                seed,
                sites: PatternVariationSites {
                    source: self.site(&statement.tokens[4]),
                    rotate: rotate_site,
                    transpose: transpose_site,
                    velocity: velocity_site,
                    reverse: reverse_site,
                    probability: probability_site,
                    seed: seed_site,
                },
            }),
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
        let mut channel = ChannelStrip {
            inserts: Vec::new(),
            sends: Vec::new(),
            level_db,
            panner: Panner::Balance {
                position: pan,
                site: self.site(&statement.tokens[8]),
            },
            mute: false,
            output: OutputTarget::Master,
            level_site: self.site(&statement.tokens[6]),
            mute_site: None,
        };
        let mut clips = Vec::new();
        for child in &statement.children {
            if child.keyword() != Some("clip") {
                if !self.parse_channel_directive(child, &mut channel) {
                    self.unknown("Track", child.keyword().unwrap_or(""), child);
                }
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
            let mut source_offset = MusicalTime::ZERO;
            let mut transpose_semitones = 0_i16;
            let mut gain_db = 0.0;
            let mut fade_in = MusicalTime::ZERO;
            let mut fade_out = MusicalTime::ZERO;
            let mut groove = None;
            let mut source_offset_site = None;
            let mut transpose_site = None;
            let mut gain_site = None;
            let mut fade_in_site = None;
            let mut fade_out_site = None;
            let mut groove_site = None;
            for directive in &child.children {
                match directive.keyword() {
                    Some("offset" | "source_offset") if directive.tokens.len() == 2 => {
                        if source_offset_site.is_some() {
                            self.error_at("CRP193", "Clip offset may be declared once", directive);
                            continue;
                        }
                        if let Some(value) = self.parse_time(&directive.tokens[1], meter) {
                            source_offset = value;
                            source_offset_site = Some(self.site(&directive.tokens[1]));
                        }
                    }
                    Some("transpose") if directive.tokens.len() == 2 => {
                        if transpose_site.is_some() {
                            self.error_at(
                                "CRP194",
                                "Clip transpose may be declared once",
                                directive,
                            );
                            continue;
                        }
                        if let Some(value) =
                            self.parse_physical(Unit::Semitones, &directive.tokens[1])
                        {
                            if value.fract() == 0.0
                                && value >= f64::from(i16::MIN)
                                && value <= f64::from(i16::MAX)
                            {
                                transpose_semitones = value as i16;
                                transpose_site = Some(self.site(&directive.tokens[1]));
                            } else {
                                self.token_error(
                                    "CRP194",
                                    "Clip transpose must be a whole semitone value",
                                    &directive.tokens[1],
                                );
                            }
                        }
                    }
                    Some("gain") if directive.tokens.len() == 2 => {
                        if gain_site.is_some() {
                            self.error_at("CRP195", "Clip gain may be declared once", directive);
                            continue;
                        }
                        if let Some(value) =
                            self.parse_physical(Unit::Decibels, &directive.tokens[1])
                        {
                            gain_db = value;
                            gain_site = Some(self.site(&directive.tokens[1]));
                        }
                    }
                    Some("fade") if directive.tokens.len() == 3 => {
                        if fade_in_site.is_some() || fade_out_site.is_some() {
                            self.error_at("CRP196", "Clip fade may be declared once", directive);
                            continue;
                        }
                        let parsed_in = self.parse_time(&directive.tokens[1], meter);
                        let parsed_out = self.parse_time(&directive.tokens[2], meter);
                        if let (Some(parsed_in), Some(parsed_out)) = (parsed_in, parsed_out) {
                            fade_in = parsed_in;
                            fade_out = parsed_out;
                            fade_in_site = Some(self.site(&directive.tokens[1]));
                            fade_out_site = Some(self.site(&directive.tokens[2]));
                        }
                    }
                    Some("groove") if directive.tokens.len() == 2 => {
                        if groove_site.is_some() {
                            self.error_at("CRP231", "Clip groove may be declared once", directive);
                            continue;
                        }
                        groove = Some(directive.tokens[1].value.clone());
                        groove_site = Some(self.site(&directive.tokens[1]));
                    }
                    Some(keyword) => self.unknown("Clip", keyword, directive),
                    None => {}
                }
            }
            if let (Some(start), Some(duration), Some(mode)) = (start, duration, mode) {
                clips.push(Clip {
                    id: child.tokens[1].value.clone(),
                    pattern: child.tokens[3].value.clone(),
                    start,
                    duration,
                    mode,
                    source_offset,
                    transpose_semitones,
                    gain_db,
                    fade_in,
                    fade_out,
                    groove,
                    sites: ClipSites {
                        site: ValueSite::new(
                            self.file.to_path_buf(),
                            child.start,
                            child.end,
                            self.source[child.start..child.end].to_owned(),
                        ),
                        pattern: self.site(&child.tokens[3]),
                        start: self.site(&child.tokens[5]),
                        duration: self.site(&child.tokens[7]),
                        mode: self.site(&child.tokens[9]),
                        source_offset: source_offset_site,
                        transpose: transpose_site,
                        gain: gain_site,
                        fade_in: fade_in_site,
                        fade_out: fade_out_site,
                        groove: groove_site,
                    },
                    location: child.location(self.file, self.source),
                });
            }
        }
        Some(Track {
            id: statement.tokens[1].value.clone(),
            name: statement.tokens[2].value.clone(),
            instrument: statement.tokens[4].value.clone(),
            channel,
            clips,
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_group(&mut self, statement: &Statement) -> Option<Group> {
        if statement.tokens.len() != 5
            || statement.tokens[2].kind != TokenKind::String
            || statement.tokens[3].value != "level"
        {
            self.error_at(
                "CRP210",
                "Group syntax is `group id \"Name\" level 0db {`",
                statement,
            );
            return None;
        }
        let mut channel = self.parse_channel_header(statement, 4)?;
        for child in &statement.children {
            if !self.parse_channel_directive(child, &mut channel) {
                self.unknown("Group", child.keyword().unwrap_or(""), child);
            }
        }
        Some(Group {
            id: statement.tokens[1].value.clone(),
            name: statement.tokens[2].value.clone(),
            channel,
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_return(&mut self, statement: &Statement) -> Option<Return> {
        if statement.tokens.len() != 5
            || statement.tokens[2].kind != TokenKind::String
            || statement.tokens[3].value != "level"
        {
            self.error_at(
                "CRP211",
                "Return syntax is `return id \"Name\" level 0db {`",
                statement,
            );
            return None;
        }
        let mut channel = self.parse_channel_header(statement, 4)?;
        for child in &statement.children {
            if !self.parse_channel_directive(child, &mut channel) {
                self.unknown("Return", child.keyword().unwrap_or(""), child);
            }
        }
        Some(Return {
            id: statement.tokens[1].value.clone(),
            name: statement.tokens[2].value.clone(),
            channel,
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_master(&mut self, statement: &Statement) -> Option<Master> {
        if statement.tokens.len() != 3 || statement.tokens[1].value != "level" {
            self.error_at("CRP212", "Master syntax is `master level 0db {`", statement);
            return None;
        }
        let level_db = self.parse_physical(Unit::Decibels, &statement.tokens[2])?;
        let mut inserts = Vec::new();
        for child in &statement.children {
            if child.keyword() == Some("processor") {
                if let Some(processor) = self.parse_processor(child) {
                    inserts.push(processor);
                }
            } else {
                self.unknown("Master", child.keyword().unwrap_or(""), child);
            }
        }
        Some(Master {
            inserts,
            level_db,
            level_site: self.site(&statement.tokens[2]),
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_channel_header(
        &mut self,
        statement: &Statement,
        level_index: usize,
    ) -> Option<ChannelStrip> {
        let level_db = self.parse_physical(Unit::Decibels, &statement.tokens[level_index])?;
        let site = self.site(&statement.tokens[level_index]);
        Some(ChannelStrip {
            inserts: Vec::new(),
            sends: Vec::new(),
            level_db,
            panner: Panner::Balance {
                position: 0.0,
                site: site.clone(),
            },
            mute: false,
            output: OutputTarget::Master,
            level_site: site,
            mute_site: None,
        })
    }

    fn parse_channel_directive(
        &mut self,
        statement: &Statement,
        channel: &mut ChannelStrip,
    ) -> bool {
        match statement.keyword() {
            Some("processor") => {
                if let Some(processor) = self.parse_processor(statement) {
                    channel.inserts.push(processor);
                }
            }
            Some("send") => {
                if let Some(send) = self.parse_send(statement) {
                    channel.sends.push(send);
                }
            }
            Some("panner") => {
                if let Some(panner) = self.parse_panner(statement) {
                    channel.panner = panner;
                }
            }
            Some("mute") => {
                if statement.tokens.len() != 2 {
                    self.error_at("CRP213", "mute syntax is `mute true|false`", statement);
                } else if let Some(value) = self.parse_bool(&statement.tokens[1], "mute") {
                    channel.mute = value;
                    channel.mute_site = Some(self.site(&statement.tokens[1]));
                }
            }
            Some("output") => {
                if statement.tokens.len() != 2 {
                    self.error_at(
                        "CRP214",
                        "output syntax is `output master|group/ID|return/ID`",
                        statement,
                    );
                } else {
                    match OutputTarget::from_str(&statement.tokens[1].value) {
                        Ok(output) => channel.output = output,
                        Err(error) => {
                            self.token_error("CRP215", error.to_string(), &statement.tokens[1])
                        }
                    }
                }
            }
            _ => return false,
        }
        true
    }

    fn parse_panner(&mut self, statement: &Statement) -> Option<Panner> {
        match statement.tokens.get(1).map(|token| token.value.as_str()) {
            Some("mono_pan") if statement.tokens.len() == 3 => {
                let position = self.parse_physical(Unit::Pan, &statement.tokens[2])?;
                Some(Panner::MonoPan {
                    position,
                    site: self.site(&statement.tokens[2]),
                })
            }
            Some("balance") if statement.tokens.len() == 3 => {
                let position = self.parse_physical(Unit::Pan, &statement.tokens[2])?;
                Some(Panner::Balance {
                    position,
                    site: self.site(&statement.tokens[2]),
                })
            }
            Some("stereo_pan") if statement.tokens.len() == 4 => {
                let left = self.parse_physical(Unit::Pan, &statement.tokens[2])?;
                let right = self.parse_physical(Unit::Pan, &statement.tokens[3])?;
                Some(Panner::StereoPan {
                    left,
                    right,
                    left_site: self.site(&statement.tokens[2]),
                    right_site: self.site(&statement.tokens[3]),
                })
            }
            _ => {
                self.error_at(
                    "CRP216",
                    "panner syntax is `panner mono_pan|balance POS` or `panner stereo_pan LEFT RIGHT`",
                    statement,
                );
                None
            }
        }
    }

    fn parse_send(&mut self, statement: &Statement) -> Option<Send> {
        if statement.tokens.len() != 8
            || statement.tokens[2].value != "target"
            || statement.tokens[4].value != "tap"
            || statement.tokens[6].value != "level"
        {
            self.error_at(
                "CRP217",
                "Send syntax is `send id target return/ID tap pre|post level -12db`",
                statement,
            );
            return None;
        }
        let target = statement.tokens[3]
            .value
            .strip_prefix("return/")
            .filter(|target| !target.is_empty())
            .map(str::to_owned);
        let target = self.required(
            target,
            "CRP218",
            "Send target must be `return/ID`",
            statement,
        )?;
        let tap = match TapPoint::from_str(&statement.tokens[5].value) {
            Ok(tap) => tap,
            Err(error) => {
                self.token_error("CRP219", error.to_string(), &statement.tokens[5]);
                return None;
            }
        };
        let level_db = self.parse_physical(Unit::Decibels, &statement.tokens[7])?;
        Some(Send {
            id: statement.tokens[1].value.clone(),
            target,
            tap,
            level_db,
            level_site: self.site(&statement.tokens[7]),
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_processor(&mut self, statement: &Statement) -> Option<Processor> {
        if statement.tokens.len() != 3 {
            self.error_at(
                "CRP220",
                "processor syntax is `processor id utility|eq|compressor|gate|delay|reverb|saturation|modulation_delay|crusher|stutter|limiter|latency {`",
                statement,
            );
            return None;
        }
        let kind = match ProcessorKind::from_str(&statement.tokens[2].value) {
            Ok(kind) => kind,
            Err(error) => {
                self.token_error("CRP221", error.to_string(), &statement.tokens[2]);
                return None;
            }
        };
        let mut bypass = false;
        let mut bypass_site = None;
        let mut settings = Vec::new();
        let mut sidechain_inputs = Vec::new();
        let mut latency_frames = 0;
        let mut latency_site = None;
        for child in &statement.children {
            match child.keyword() {
                Some("bypass") if child.tokens.len() == 2 && bypass_site.is_none() => {
                    if let Some(value) = self.parse_bool(&child.tokens[1], "processor bypass") {
                        bypass = value;
                        bypass_site = Some(self.site(&child.tokens[1]));
                    }
                }
                Some("bypass") => self.error_at(
                    "CRP236",
                    "bypass syntax is `bypass true|false` and may be declared once",
                    child,
                ),
                Some("setting") if child.tokens.len() == 3 => settings.push(ProcessorSetting {
                    key: child.tokens[1].value.clone(),
                    value: child.tokens[2].value.clone(),
                    value_site: self.site(&child.tokens[2]),
                    location: child.location(self.file, self.source),
                }),
                Some("sidechain")
                    if child.tokens.len() == 3 && child.tokens[1].value == "input" =>
                {
                    sidechain_inputs.push(child.tokens[2].value.clone());
                }
                Some("latency") => {
                    if let Some(value) = self.parse_u32_value(child, "processor latency") {
                        latency_frames = value;
                        latency_site = child.tokens.get(1).map(|token| self.site(token));
                    }
                }
                Some("setting") => {
                    self.error_at("CRP222", "setting syntax is `setting KEY VALUE`", child)
                }
                Some("sidechain") => self.error_at(
                    "CRP223",
                    "sidechain input syntax is `sidechain input NAME`",
                    child,
                ),
                Some(keyword) => self.unknown("processor", keyword, child),
                None => {}
            }
        }
        Some(Processor {
            id: statement.tokens[1].value.clone(),
            kind,
            bypass,
            bypass_site,
            settings,
            sidechain_inputs,
            latency_frames,
            latency_site,
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_sidechain(&mut self, statement: &Statement) -> Option<SidechainRoute> {
        if statement.tokens.len() != 8
            || statement.tokens[2].value != "from"
            || statement.tokens[4].value != "tap"
            || statement.tokens[6].value != "to"
        {
            self.error_at(
                "CRP224",
                "sidechain syntax is `sidechain id from track|group|return/ID tap pre|post to processor/ID/INPUT`",
                statement,
            );
            return None;
        }
        let from = match ChannelRef::from_str(&statement.tokens[3].value) {
            Ok(from) => from,
            Err(error) => {
                self.token_error("CRP225", error.to_string(), &statement.tokens[3]);
                return None;
            }
        };
        let tap = match TapPoint::from_str(&statement.tokens[5].value) {
            Ok(tap) => tap,
            Err(error) => {
                self.token_error("CRP226", error.to_string(), &statement.tokens[5]);
                return None;
            }
        };
        let destination = statement.tokens[7].value.split('/').collect::<Vec<_>>();
        let ["processor", processor, input] = destination.as_slice() else {
            self.token_error(
                "CRP227",
                "sidechain destination must be `processor/ID/INPUT`",
                &statement.tokens[7],
            );
            return None;
        };
        Some(SidechainRoute {
            id: statement.tokens[1].value.clone(),
            from,
            tap,
            processor: (*processor).to_owned(),
            input: (*input).to_owned(),
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_groove(&mut self, statement: &Statement, meter: Meter) -> Option<GrooveTemplate> {
        if statement.tokens.len() != 14
            || statement.tokens[2].value != "grid"
            || statement.tokens[4].value != "swing"
            || statement.tokens[6].value != "timing"
            || statement.tokens[8].value != "velocity"
            || statement.tokens[10].value != "probability"
            || statement.tokens[12].value != "seed"
        {
            self.error_at(
                "CRP232",
                "groove syntax is `groove ID grid TIME swing RATIO timing TIME velocity RATIO probability RATIO seed INTEGER`",
                statement,
            );
            return None;
        }
        Some(GrooveTemplate {
            id: statement.tokens[1].value.clone(),
            grid: self.parse_time(&statement.tokens[3], meter)?,
            swing: self.parse_ratio(&statement.tokens[5])?,
            timing: self.parse_time(&statement.tokens[7], meter)?,
            velocity: self.parse_ratio(&statement.tokens[9])?,
            probability: self.parse_ratio(&statement.tokens[11])?,
            seed: self.parse_u64_token(&statement.tokens[13], "Groove seed")?,
            sites: GrooveSites {
                grid: self.site(&statement.tokens[3]),
                swing: self.site(&statement.tokens[5]),
                timing: self.site(&statement.tokens[7]),
                velocity: self.site(&statement.tokens[9]),
                probability: self.site(&statement.tokens[11]),
                seed: self.site(&statement.tokens[13]),
            },
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_locator(&mut self, statement: &Statement, meter: Meter) -> Option<Locator> {
        if statement.tokens.len() != 5
            || statement.tokens[2].kind != TokenKind::String
            || statement.tokens[3].value != "at"
        {
            self.error_at(
                "CRP228",
                "locator syntax is `locator id \"Name\" at TIME`",
                statement,
            );
            return None;
        }
        Some(Locator {
            id: statement.tokens[1].value.clone(),
            name: statement.tokens[2].value.clone(),
            at: self.parse_time(&statement.tokens[4], meter)?,
            at_site: self.site(&statement.tokens[4]),
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_bool(&mut self, token: &Token, name: &str) -> Option<bool> {
        match token.value.as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => {
                self.token_error("CRP229", format!("{name} must be true or false"), token);
                None
            }
        }
    }

    fn parse_automation(&mut self, statement: &Statement, meter: Meter) -> Option<AutomationLane> {
        if statement.tokens.len() != 4 || statement.tokens[2].value != "target" {
            self.error_at(
                "CRP193",
                "Automation syntax is `automation id target instrument/ID/KEY|track/ID/level|balance {`",
                statement,
            );
            return None;
        }
        let target = match ParameterTarget::from_str(&statement.tokens[3].value) {
            Ok(target) => target,
            Err(error) => {
                self.token_error("CRP194", error.to_string(), &statement.tokens[3]);
                return None;
            }
        };
        let mut points = Vec::new();
        for child in &statement.children {
            if child.keyword() != Some("point") {
                self.unknown("Automation", child.keyword().unwrap_or(""), child);
                continue;
            }
            if child.tokens.len() != 5 {
                self.error_at(
                    "CRP195",
                    "Automation point syntax is `point id time value step|linear|power(P)`",
                    child,
                );
                continue;
            }
            let Some(at) = self.parse_time(&child.tokens[2], meter) else {
                continue;
            };
            let interpolation = match AutomationInterpolation::from_str(&child.tokens[4].value) {
                Ok(interpolation) => interpolation,
                Err(error) => {
                    self.token_error("CRP196", error.to_string(), &child.tokens[4]);
                    continue;
                }
            };
            points.push(AutomationPoint {
                id: child.tokens[1].value.clone(),
                at,
                value: child.tokens[3].value.clone(),
                interpolation,
                site: ValueSite::new(
                    self.file.to_path_buf(),
                    child.start,
                    child.end,
                    self.source[child.start..child.end].to_owned(),
                ),
                at_site: self.site(&child.tokens[2]),
                value_site: self.site(&child.tokens[3]),
                interpolation_site: self.site(&child.tokens[4]),
                location: child.location(self.file, self.source),
            });
        }
        Some(AutomationLane {
            id: statement.tokens[1].value.clone(),
            target,
            points,
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_envelope_follower(&mut self, statement: &Statement) -> Option<EnvelopeFollowerRoute> {
        if statement.tokens.len() != 20
            || statement.tokens[2].value != "from"
            || statement.tokens[4].value != "tap"
            || statement.tokens[6].value != "detector"
            || statement.tokens[8].value != "attack"
            || statement.tokens[10].value != "release"
            || statement.tokens[12].value != "floor"
            || statement.tokens[14].value != "ceiling"
            || statement.tokens[16].value != "target"
            || statement.tokens[18].value != "depth"
        {
            self.error_at(
                "CRP233",
                "Follower syntax is `follower ID from CHANNEL tap TAP detector peak|rms attack TIME release TIME floor DB ceiling DB target TARGET depth VALUE {`",
                statement,
            );
            return None;
        }
        let source = match ChannelRef::from_str(&statement.tokens[3].value) {
            Ok(source) => source,
            Err(error) => {
                self.token_error("CRP233", error.to_string(), &statement.tokens[3]);
                return None;
            }
        };
        let tap = match TapPoint::from_str(&statement.tokens[5].value) {
            Ok(tap) => tap,
            Err(error) => {
                self.token_error("CRP233", error.to_string(), &statement.tokens[5]);
                return None;
            }
        };
        let detector = match EnvelopeDetector::from_str(&statement.tokens[7].value) {
            Ok(detector) => detector,
            Err(error) => {
                self.token_error("CRP233", error.to_string(), &statement.tokens[7]);
                return None;
            }
        };
        let attack_seconds = self.parse_physical(Unit::Seconds, &statement.tokens[9])?;
        let release_seconds = self.parse_physical(Unit::Seconds, &statement.tokens[11])?;
        let floor_db = self.parse_physical(Unit::Decibels, &statement.tokens[13])?;
        let ceiling_db = self.parse_physical(Unit::Decibels, &statement.tokens[15])?;
        let target = match ParameterTarget::from_str(&statement.tokens[17].value) {
            Ok(target) => target,
            Err(error) => {
                self.token_error("CRP233", error.to_string(), &statement.tokens[17]);
                return None;
            }
        };
        let mut operators = Vec::new();
        for child in &statement.children {
            let Some(keyword) = child.keyword() else {
                continue;
            };
            let scalar = |parser: &mut Self| {
                parser
                    .single_argument(
                        child,
                        "CRP234",
                        "Follower operator requires one finite scalar",
                    )
                    .and_then(|token| parser.parse_ratio(token))
            };
            let operator = match keyword {
                "add" => scalar(self).map(|value| ModulationOperator::Add { value }),
                "multiply" => scalar(self).map(|value| ModulationOperator::Multiply { value }),
                "scale" => scalar(self).map(|value| ModulationOperator::Scale { value }),
                "bias" => scalar(self).map(|value| ModulationOperator::Bias { value }),
                "clamp" if child.tokens.len() == 3 => {
                    let minimum = self.parse_ratio(&child.tokens[1]);
                    let maximum = self.parse_ratio(&child.tokens[2]);
                    minimum
                        .zip(maximum)
                        .map(|(minimum, maximum)| ModulationOperator::Clamp { minimum, maximum })
                }
                "clamp" => {
                    self.error_at("CRP234", "clamp syntax is `clamp MINIMUM MAXIMUM`", child);
                    None
                }
                _ => {
                    self.unknown("Envelope Follower", keyword, child);
                    None
                }
            };
            if let Some(operator) = operator {
                operators.push(operator);
            }
        }
        Some(EnvelopeFollowerRoute {
            id: statement.tokens[1].value.clone(),
            source,
            tap,
            detector,
            attack_seconds,
            release_seconds,
            floor_db,
            ceiling_db,
            target,
            depth: statement.tokens[19].value.clone(),
            operators,
            location: statement.location(self.file, self.source),
        })
    }

    fn parse_modulation(&mut self, statement: &Statement, meter: Meter) -> Option<ModulationRoute> {
        if statement.tokens.len() != 12
            || statement.tokens[2].value != "lfo"
            || statement.tokens[8].value != "target"
            || statement.tokens[10].value != "depth"
        {
            self.error_at(
                "CRP197",
                "Modulation syntax is `modulation ID lfo SCOPE unipolar|bipolar sine|triangle free|sync RATE target TARGET depth VALUE {`",
                statement,
            );
            return None;
        }
        let scope_token = &statement.tokens[3];
        let scope = if scope_token.value == "global" {
            ModulationScope::Global
        } else if let Some(instrument) = scope_token.value.strip_prefix("instrument/") {
            ModulationScope::Instrument(instrument.to_owned())
        } else if let Some(instrument) = scope_token.value.strip_prefix("per_voice/") {
            ModulationScope::PerVoice(instrument.to_owned())
        } else {
            self.token_error(
                "CRP198",
                "scope must be `global`, `instrument/ID`, or `per_voice/ID`",
                scope_token,
            );
            return None;
        };
        let polarity = match statement.tokens[4].value.as_str() {
            "unipolar" => ControlPolarity::Unipolar,
            "bipolar" => ControlPolarity::Bipolar,
            _ => {
                self.token_error(
                    "CRP199",
                    "LFO polarity must be `unipolar` or `bipolar`",
                    &statement.tokens[4],
                );
                return None;
            }
        };
        let waveform = match statement.tokens[5].value.as_str() {
            "sine" => LfoWaveform::Sine,
            "triangle" => LfoWaveform::Triangle,
            _ => {
                self.token_error(
                    "CRP200",
                    "LFO waveform must be `sine` or `triangle`",
                    &statement.tokens[5],
                );
                return None;
            }
        };
        let rate = match statement.tokens[6].value.as_str() {
            "free" => self
                .parse_physical(Unit::Hertz, &statement.tokens[7])
                .map(ModulationRate::FreeHertz),
            "sync" => self
                .parse_time(&statement.tokens[7], meter)
                .map(ModulationRate::Synchronized),
            _ => {
                self.token_error(
                    "CRP201",
                    "LFO rate mode must be `free` or `sync`",
                    &statement.tokens[6],
                );
                None
            }
        }?;
        let target = match ParameterTarget::from_str(&statement.tokens[9].value) {
            Ok(target) => target,
            Err(error) => {
                self.token_error("CRP202", error.to_string(), &statement.tokens[9]);
                return None;
            }
        };
        let mut operators = Vec::new();
        for child in &statement.children {
            let Some(keyword) = child.keyword() else {
                continue;
            };
            let scalar = |parser: &mut Self| {
                parser
                    .single_argument(
                        child,
                        "CRP203",
                        "Modulation operator requires one finite scalar",
                    )
                    .and_then(|token| parser.parse_ratio(token))
            };
            let operator = match keyword {
                "add" => scalar(self).map(|value| ModulationOperator::Add { value }),
                "multiply" => scalar(self).map(|value| ModulationOperator::Multiply { value }),
                "scale" => scalar(self).map(|value| ModulationOperator::Scale { value }),
                "bias" => scalar(self).map(|value| ModulationOperator::Bias { value }),
                "clamp" if child.tokens.len() == 3 => {
                    let minimum = self.parse_ratio(&child.tokens[1]);
                    let maximum = self.parse_ratio(&child.tokens[2]);
                    minimum
                        .zip(maximum)
                        .map(|(minimum, maximum)| ModulationOperator::Clamp { minimum, maximum })
                }
                "clamp" => {
                    self.error_at("CRP204", "clamp syntax is `clamp MINIMUM MAXIMUM`", child);
                    None
                }
                _ => {
                    self.unknown("Modulation", keyword, child);
                    None
                }
            };
            if let Some(operator) = operator {
                operators.push(operator);
            }
        }
        Some(ModulationRoute {
            id: statement.tokens[1].value.clone(),
            scope,
            polarity,
            waveform,
            rate,
            target,
            depth: statement.tokens[11].value.clone(),
            operators,
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

    fn parse_u32_token(&mut self, token: &Token, name: &str) -> Option<u32> {
        match token.value.parse() {
            Ok(value) => Some(value),
            Err(_) => {
                self.token_error(
                    "CRP206",
                    format!("{name} must be an unsigned integer"),
                    token,
                );
                None
            }
        }
    }

    fn parse_u64_token(&mut self, token: &Token, name: &str) -> Option<u64> {
        match token.value.parse() {
            Ok(value) => Some(value),
            Err(_) => {
                self.token_error(
                    "CRP206",
                    format!("{name} must be an unsigned integer"),
                    token,
                );
                None
            }
        }
    }

    fn parse_pitch_token(&mut self, token: &Token) -> Option<u8> {
        match parse_pitch(&token.value) {
            Ok(value) => Some(value),
            Err(error) => {
                self.token_error("CRP182", error.to_string(), token);
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
    fn rejects_duplicate_generated_provenance_directives() {
        let source = PROJECT.replace(
            "  render_tail 2s",
            r#"  render_tail 2s
  asset tail "Tail" wav "media/tail.wav" sha256 "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef" {
    generated {
      revision "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
      revision "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
      owner master
      tap master_output
      range 0 10
      tail 0
      format stereo_f32_wav
      algorithm test-v1
      build "0.1.0"
    }
  }"#,
        );
        let document = parse_project(Path::new("project.crump"), &source);
        assert!(document.diagnostics.as_slice().iter().any(|diagnostic| {
            diagnostic.code == "CRP231"
                && diagnostic
                    .message
                    .contains("`revision` is declared more than once")
        }));
    }

    #[test]
    fn recovers_multiple_diagnostics() {
        let document = parse_project(Path::new("project.crump"), "crump 2\nwat nope\n");
        assert!(document.diagnostics.as_slice().len() >= 3);
    }
}
