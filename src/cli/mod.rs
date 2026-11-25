//! Stable human and JSON command surface for external Authoring Agents and scripts.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{IsTerminal, Write},
    path::{Component, Path, PathBuf},
    process::ExitCode,
    sync::atomic::{AtomicU64, Ordering},
};

use clap::{Args, Parser, Subcommand, ValueEnum, error::ErrorKind};
use serde_json::{Value, json};

use crate::{
    edit::{
        ArrangementAutomation, ClipPlacement, EditError, GroovePlacement, LevelToError,
        LevelToRequest, LoudnessMeasure, MixLevelOwner, ParameterLockPlacement,
        PatternVariationPlacement, SamplePlacement, SampleSliceMapPlacement,
        WavetableImportOptions, apply_structured_edit, collect_media_assets,
        create_automation_lane, create_automation_point, create_clip, create_groove,
        create_locator, create_parameter_lock, create_pattern_variation, create_sample_slice_map,
        delete_arrangement_time, delete_automation_lane, delete_automation_point, delete_clip,
        delete_groove, delete_locator, delete_parameter_lock, delete_pattern_variation,
        duplicate_arrangement_time, duplicate_clip, duplicate_sample_occurrence,
        import_media_asset, import_wavetable_asset, insert_arrangement_time, level_to, move_clip,
        move_locator, move_sample_occurrence, parse_level_target_db, place_sample_occurrence,
        publish_bounce_asset, publish_sample_transform_asset, resize_clip, set_clip_fades,
        set_clip_gain, set_clip_groove, set_clip_source_offset, set_clip_transpose,
        slip_sample_occurrence, split_clip, split_sample_occurrence, update_groove,
        update_parameter_lock, update_pattern_variation,
    },
    engine::{
        CompiledProject, EngineError, OfflineThreads, QualificationOptions, RenderOptions,
        RenderStemRequest,
        audition::{
            AuditionPath, AuditionPlan, AuditionRange, AuditionTarget, inspection_from_parts,
        },
        bounce::{
            BounceFormatPolicy, BounceRequest, BounceSource, BounceTailPolicy, bounce_revision,
        },
        generated_audio::encode_float_wav,
        inspect_render_destination, plan_render_stems,
        qualification::qualify_compiled_project,
        render_project,
        sample_transform::{
            SampleTransformFormatPolicy, SampleTransformRequest, transform_sample_revision,
        },
    },
    history::{ChangeOrigin, HistoryError, HistoryStore, Publication},
    instrument_module::{
        catalog_entry, catalog_matches, render_module_preview, validate_module_source,
    },
    interrupt::Interrupt,
    model::{
        AutomationInterpolation, ChannelRef, ClipMode, ParameterTarget, RenderTailPolicy, TapPoint,
        WavetableMorph, WavetableNormalization, parse_pitch,
    },
    production_view::{
        ExecuteError as ProductionViewError, MoveDirection, Operation as ProductionViewOperation,
        PhysicalValue, Provenance as ProductionViewProvenance, Request as ProductionViewRequest,
        ValueKind as ProductionViewValueKind, execute as execute_production_view,
    },
    project::{MAX_SOURCE_BYTES, ProjectBundle, instrument_library_dir, load_project},
    project_picker::{ProjectSelection, select_project},
    realtime::{Player, StreamStatus},
    time::parse_musical_time,
    validate::check_musical_sketch,
};

const COMMAND_SCHEMA: u32 = 1;
static MODULE_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

mod arguments;
mod commands;

pub use arguments::Cli;
use arguments::*;
#[cfg(test)]
use commands::{blank_project_source, slug_id, workspace_needs_project_picker};
use commands::{emit_failure, emit_success, execute, serialize_response};

/// Parses process arguments, executes one command, and returns a stable exit status.
pub fn run() -> ExitCode {
    run_from(std::env::args_os())
}

/// Testable command entry point.
pub fn run_from(arguments: impl IntoIterator<Item = OsString>) -> ExitCode {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    let json_requested = arguments.iter().any(|argument| argument == "--json");
    let project_explicit = project_option_is_present(&arguments);
    let cli = match Cli::try_parse_from(arguments) {
        Ok(mut cli) => {
            cli.project_explicit = project_explicit;
            cli
        }
        Err(error) => {
            let informational = matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            );
            let exit_code = if informational { 0 } else { 2 };
            if json_requested {
                let response = if informational {
                    json!({
                        "schema_version": COMMAND_SCHEMA,
                        "ok": true,
                        "command": "help",
                        "data": { "text": error.to_string() },
                    })
                } else {
                    json!({
                        "schema_version": COMMAND_SCHEMA,
                        "ok": false,
                        "command": "cli",
                        "error": {
                            "code": "usage_error",
                            "message": error.to_string(),
                            "diagnostics": [],
                        },
                    })
                };
                println!("{}", serialize_response(&response));
            } else {
                let _ = error.print();
            }
            return ExitCode::from(exit_code);
        }
    };
    match execute(&cli) {
        Ok(success) => {
            emit_success(&cli, success);
            ExitCode::SUCCESS
        }
        Err(failure) => {
            emit_failure(&cli, &failure);
            ExitCode::from(failure.exit_code)
        }
    }
}

fn project_option_is_present(arguments: &[OsString]) -> bool {
    arguments
        .iter()
        .skip(1)
        .take_while(|argument| *argument != "--")
        .any(|argument| {
            argument == "--project"
                || argument
                    .to_string_lossy()
                    .strip_prefix("--project=")
                    .is_some()
        })
}

#[derive(Debug)]
struct Success {
    command: &'static str,
    human: String,
    data: Value,
}

#[derive(Debug)]
struct Failure {
    command: &'static str,
    code: &'static str,
    message: String,
    exit_code: u8,
    diagnostics: Value,
}

impl Failure {
    fn operational(
        command: &'static str,
        code: &'static str,
        error: impl std::fmt::Display,
    ) -> Self {
        Self {
            command,
            code,
            message: error.to_string(),
            exit_code: 1,
            diagnostics: json!([]),
        }
    }

    fn blocked(
        command: &'static str,
        code: &'static str,
        message: impl Into<String>,
        diagnostics: Value,
    ) -> Self {
        Self {
            command,
            code,
            message: message.into(),
            exit_code: 2,
            diagnostics,
        }
    }

    /// A termination signal stopped the command; the exit status is the shell's `128 + signal`.
    fn interrupted(
        command: &'static str,
        signal: crate::interrupt::Signal,
        message: impl Into<String>,
    ) -> Self {
        Self {
            command,
            code: "interrupted",
            message: message.into(),
            exit_code: signal.exit_code(),
            diagnostics: json!([]),
        }
    }

    fn conflict(command: &'static str, code: &'static str, error: impl std::fmt::Display) -> Self {
        Self {
            command,
            code,
            message: error.to_string(),
            exit_code: 3,
            diagnostics: json!([]),
        }
    }
}

fn open_history(root: &Path) -> Result<HistoryStore, Failure> {
    HistoryStore::open(root)
        .map_err(|error| Failure::operational("history", "history_error", error))
}

#[cfg(test)]
mod tests;
