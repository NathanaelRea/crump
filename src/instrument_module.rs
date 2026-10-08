//! Deep source-readable Instrument Module inspection, dependency, search, and preview seam.

use std::{collections::BTreeMap, path::Path};

use serde::Serialize;

use crate::{
    diagnostic::{Diagnostic, Diagnostics},
    dsl::{parse_module, parse_project},
    engine::{CompiledProject, ProcessingMode},
    media::AssetInventory,
    model::{
        Instrument, InstrumentKind, MediaAsset, MediaAssetUsage, NodeKind, PatternEvent,
        PresentationItemKind, Project, SettingValue,
    },
    time::MusicalTime,
    validate::validate_instrument_module,
};

/// One exact Project-asset dependency derived from readable Instrument source.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct ModuleAssetDependency {
    /// Durable Project asset ID required by the Instrument.
    pub(crate) asset_id: String,
    /// Source-v2 usage declarations required by graph Nodes; empty for ordinary sampler/metadata use.
    pub(crate) usages: Vec<MediaAssetUsage>,
    /// Stable readable Instrument declarations that reference the asset.
    pub(crate) references: Vec<String>,
}

/// One parsed and semantically validated Instrument Module.
#[derive(Debug, Clone)]
pub(crate) struct ValidatedInstrumentModule {
    pub(crate) acoustic_profiles: Vec<crate::model::AcousticProfile>,
    pub(crate) source_version: u32,
    pub(crate) instrument: Instrument,
    pub(crate) dependencies: Vec<ModuleAssetDependency>,
}

/// Searchable personal-library entry derived by reparsing source, never an opaque database row.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ModuleCatalogEntry {
    pub(crate) name: String,
    pub(crate) source_version: Option<u32>,
    pub(crate) instrument_id: Option<String>,
    pub(crate) instrument_name: Option<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) dependencies: Vec<ModuleAssetDependency>,
    pub(crate) valid: bool,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

/// Validates one complete Module source and derives its exact asset interface.
pub(crate) fn validate_module_source(
    path: &Path,
    source: &str,
) -> Result<ValidatedInstrumentModule, Diagnostics> {
    let document = parse_module(path, source);
    let source_version = document.source_version;
    let mut diagnostics = document.diagnostics;
    diagnostics.extend(validate_instrument_module(&document.instruments).into_vec());
    if diagnostics.has_errors() || document.instruments.len() != 1 {
        return Err(diagnostics);
    }
    let instrument = document
        .instruments
        .into_iter()
        .next()
        .expect("validated one-Instrument Module");
    // Library installation must not silently drop an acoustic dependency. Project
    // imports can keep profiles separate; export carries them into the Module.
    if let Ok(mut preview) = preview_project(&instrument) {
        preview.acoustic_profiles = document.acoustic_profiles.clone();
        crate::validate::validate_acoustic_profiles(&preview, &mut diagnostics);
    }
    if diagnostics.has_errors() {
        return Err(diagnostics);
    }
    let dependencies = module_asset_dependencies(&instrument);
    Ok(ValidatedInstrumentModule {
        source_version: source_version.expect("validated Module header"),
        instrument,
        acoustic_profiles: document.acoustic_profiles,
        dependencies,
    })
}

/// Produces one catalog row from exact source, retaining invalid diagnostics for inspection.
pub(crate) fn catalog_entry(name: &str, path: &Path, source: &str) -> ModuleCatalogEntry {
    match validate_module_source(path, source) {
        Ok(module) => ModuleCatalogEntry {
            name: name.to_owned(),
            source_version: Some(module.source_version),
            instrument_id: Some(module.instrument.id.clone()),
            instrument_name: Some(module.instrument.name.clone()),
            tags: module.instrument.presentation.tags.clone(),
            dependencies: module.dependencies,
            valid: true,
            diagnostics: Vec::new(),
        },
        Err(diagnostics) => ModuleCatalogEntry {
            name: name.to_owned(),
            source_version: None,
            instrument_id: None,
            instrument_name: None,
            tags: Vec::new(),
            dependencies: Vec::new(),
            valid: false,
            diagnostics: diagnostics.into_vec(),
        },
    }
}

/// Case-insensitive catalog filtering with tag intersection semantics.
pub(crate) fn catalog_matches(
    entry: &ModuleCatalogEntry,
    search: Option<&str>,
    tags: &[String],
) -> bool {
    let tag_match = tags.iter().all(|required| {
        entry
            .tags
            .iter()
            .any(|tag| tag.eq_ignore_ascii_case(required))
    });
    if !tag_match {
        return false;
    }
    let Some(search) = search.map(str::trim).filter(|search| !search.is_empty()) else {
        return true;
    };
    let search = search.to_ascii_lowercase();
    entry.name.to_ascii_lowercase().contains(&search)
        || entry
            .instrument_id
            .as_ref()
            .is_some_and(|value| value.to_ascii_lowercase().contains(&search))
        || entry
            .instrument_name
            .as_ref()
            .is_some_and(|value| value.to_ascii_lowercase().contains(&search))
        || entry
            .tags
            .iter()
            .any(|tag| tag.to_ascii_lowercase().contains(&search))
}

/// Derives every Project-owned asset needed to instantiate the Instrument.
pub(crate) fn module_asset_dependencies(instrument: &Instrument) -> Vec<ModuleAssetDependency> {
    let mut dependencies = BTreeMap::<String, (Vec<MediaAssetUsage>, Vec<String>)>::new();
    let mut include = |asset_id: &str, usage: Option<MediaAssetUsage>, reference: String| {
        let (usages, references) = dependencies.entry(asset_id.to_owned()).or_default();
        if let Some(usage) = usage
            && !usages.contains(&usage)
        {
            usages.push(usage);
            usages.sort();
        }
        if !references.contains(&reference) {
            references.push(reference);
            references.sort();
        }
    };

    if let Some(sampler) = &instrument.sampler {
        for mapping in &sampler.mappings {
            include(
                &mapping.asset,
                None,
                format!("sampler mapping {}", mapping.id),
            );
        }
    }
    for graph in instrument
        .graph
        .iter()
        .chain(instrument.shared_graph.iter())
    {
        for node in &graph.nodes {
            let usage = match node.kind {
                NodeKind::Wavetable => Some(MediaAssetUsage::Wavetable),
                NodeKind::SampleSource => Some(MediaAssetUsage::Sample),
                NodeKind::Granular => Some(MediaAssetUsage::Granular),
                NodeKind::Spectral => Some(MediaAssetUsage::Spectral),
                _ => None,
            };
            if let Some(usage) = usage
                && let Some(SettingValue::Text(asset_id)) = node.setting("asset")
            {
                include(asset_id, Some(usage), format!("node {}", node.id));
            }
            if node.kind == NodeKind::Multisample {
                for directive in &node.directives {
                    if directive.tokens.len() >= 4
                        && directive.tokens[0].value == "zone"
                        && directive.tokens[2].value == "asset"
                    {
                        include(
                            &directive.tokens[3].value,
                            Some(MediaAssetUsage::Multisample),
                            format!("node {} zone {}", node.id, directive.tokens[1].value),
                        );
                    }
                }
            }
        }
    }
    for section in &instrument.presentation.sections {
        for item in &section.items {
            if item.kind == PresentationItemKind::Asset {
                include(
                    &item.id,
                    None,
                    format!("presentation section {}", section.id),
                );
            }
        }
    }

    dependencies
        .into_iter()
        .map(|(asset_id, (usages, references))| ModuleAssetDependency {
            asset_id,
            usages,
            references,
        })
        .collect()
}

/// Builds and renders the bounded source-readable three-note Module preview.
pub(crate) fn render_module_preview(
    module: &ValidatedInstrumentModule,
    project_assets: &[MediaAsset],
    inventory: &AssetInventory,
) -> Result<(u32, Vec<[f32; 2]>), String> {
    let missing = module
        .dependencies
        .iter()
        .filter(|dependency| {
            !project_assets
                .iter()
                .any(|asset| asset.id == dependency.asset_id)
        })
        .map(|dependency| dependency.asset_id.as_str())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "current Project is missing Module assets: {}",
            missing.join(", ")
        ));
    }
    let mut project = preview_project(&module.instrument)?;
    project.assets = project_assets.to_vec();
    project.acoustic_profiles = module.acoustic_profiles.clone();
    let compiled = CompiledProject::compile_with_assets(&project, inventory)
        .map_err(|error| error.to_string())?;
    let frame_count = usize::try_from(compiled.total_frames())
        .map_err(|_| "Module preview frame count exceeds addressable memory".to_owned())?;
    let mut runtime = compiled.runtime();
    let mut frames = Vec::with_capacity(frame_count);
    while frames.len() < frame_count {
        let count = (frame_count - frames.len()).min(256);
        let start = frames.len();
        frames.resize(start + count, [0.0; 2]);
        runtime
            .process_block(&mut frames[start..], ProcessingMode::Offline)
            .map_err(|error| error.to_string())?;
    }
    Ok((compiled.sample_rate(), frames))
}

fn preview_project(instrument: &Instrument) -> Result<Project, String> {
    const NOTE_PREVIEW: &str = r#"crump 2
project "Module Preview" {
  id module-preview
  seed 6173
  sample_rate 48000
  render_tail 1s
  song {
    tempo 120bpm
    meter 4/4
    end 2bar
    instrument placeholder "Placeholder" graph {
      polyphony 4
      graph voice 3 {
        node tone oscillator waveform sine level 0.2
        node position pan position 0
        node out output
        bus pitch note.pitch -> tone.pitch
        bus stereo tone.audio -> position.audio
        bus output position.audio -> out.audio
      }
    }
    pattern preview 2bar {
      note 0beat C3 1beat 0.5
      note 2beat C4 1beat 0.7
      note 4beat C5 1beat 0.9
    }
    track preview "Preview" instrument placeholder level -6db pan 0 {
      clip preview-clip pattern preview start 0bar duration 2bar mode once
    }
  }
}
"#;
    const DRUM_PREVIEW: &str = r#"crump 2
project "Module Preview" {
  id module-preview
  seed 6173
  sample_rate 48000
  render_tail 1s
  song {
    tempo 120bpm
    meter 4/4
    end 2bar
    instrument placeholder "Placeholder" stock_drums {
      polyphony 8
      parameter tone "Tone" ratio 0 1 0.5 0.5 automatable
      parameter punch "Punch" ratio 0 1 0.5 0.5 automatable
      faceplate {
        control tone "Tone" "Tone" maps tone
        control punch "Punch" "Punch" maps punch
      }
    }
    pattern preview 2bar {
      hit 0beat kick 0.5
      hit 1beat snare 0.65
      hit 2beat hat 0.8
      hit 3beat clap 0.95
    }
    track preview "Preview" instrument placeholder level -6db pan 0 {
      clip preview-clip pattern preview start 0bar duration 2bar mode once
    }
  }
}
"#;
    let source = if instrument.kind == InstrumentKind::StockDrums {
        DRUM_PREVIEW
    } else {
        NOTE_PREVIEW
    };
    let mut project = parse_project(Path::new("module-preview.crump"), source)
        .project
        .ok_or_else(|| "internal Module preview template did not parse".to_owned())?;
    project.song.instruments = vec![instrument.clone()];
    project.song.tracks[0].instrument = instrument.id.clone();

    if instrument.kind != InstrumentKind::StockDrums {
        let (low_pitch, high_pitch, low_velocity, high_velocity, note_duration) = instrument
            .presentation
            .preview
            .as_ref()
            .map_or((48, 72, 0.5, 0.9, MusicalTime::whole(1)), |preview| {
                (
                    preview.low_pitch,
                    preview.high_pitch,
                    preview.low_velocity,
                    preview.high_velocity,
                    preview.note_duration,
                )
            });
        let middle_pitch = low_pitch + (high_pitch - low_pitch) / 2;
        let middle_velocity = (low_velocity + high_velocity) * 0.5;
        for (event, pitch, velocity) in project.song.patterns[0]
            .events
            .iter_mut()
            .zip([low_pitch, middle_pitch, high_pitch].into_iter().zip([
                low_velocity,
                middle_velocity,
                high_velocity,
            ]))
            .map(|(event, (pitch, velocity))| (event, pitch, velocity))
        {
            let PatternEvent::Note {
                pitch: event_pitch,
                duration,
                velocity: event_velocity,
                ..
            } = event
            else {
                return Err("internal Module note preview template is invalid".to_owned());
            };
            *event_pitch = pitch;
            *duration = note_duration;
            *event_velocity = velocity;
        }
    }
    Ok(project)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAGGED_MODULE: &str = r#"crump-module 2
instrument tagged "Tagged Lead" graph {
  polyphony 4
  parameter level "Level" ratio 0 1 0.2 0.2 modulatable
  faceplate {
    control level "Level" "Preview level" maps level
  }
  presentation {
    tags lead analog bright
    preview C3 C5 velocity 25% 90% duration 1beat
  }
  graph voice 3 {
    node tone oscillator waveform saw level $level
    node position pan position 0
    node out output
    bus pitch note.pitch -> tone.pitch
    bus stereo tone.audio -> position.audio
    bus output position.audio -> out.audio
  }
}
"#;

    #[test]
    fn metadata_is_source_readable_validated_and_previewable() {
        let module = validate_module_source(Path::new("tagged.crump"), TAGGED_MODULE)
            .expect("tagged Module");
        assert_eq!(
            module.instrument.presentation.tags,
            ["lead", "analog", "bright"]
        );
        let Some(preview) = module.instrument.presentation.preview.as_ref() else {
            panic!("validated tagged Module has no preview");
        };
        assert_eq!(preview.low_pitch, 48);
        assert_eq!(preview.high_pitch, 72);
        assert_eq!(preview.note_duration, MusicalTime::whole(1));
        let (sample_rate, frames) =
            match render_module_preview(&module, &[], &crate::media::AssetInventory::default()) {
                Ok(preview) => preview,
                Err(error) => panic!("render preview: {error}"),
            };
        assert_eq!(sample_rate, 48_000);
        assert!(frames.iter().flatten().all(|sample| sample.is_finite()));
        assert!(frames.iter().flatten().any(|sample| sample.abs() > 1.0e-5));

        let invalid = TAGGED_MODULE
            .replace("tags lead analog bright", "tags Lead lead")
            .replace("preview C3 C5", "preview C5 C3");
        let diagnostics = match validate_module_source(Path::new("invalid.crump"), &invalid) {
            Err(diagnostics) => diagnostics,
            Ok(_) => panic!("invalid metadata unexpectedly validated"),
        };
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .any(|diagnostic| diagnostic.code == "CRV201")
        );
    }

    #[test]
    fn every_starter_module_dependency_resolves_and_preview_renders() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/starter");
        let bundle = crate::project::load_project(root.join("auditions.crump"))
            .expect("load starter Audition Project");
        let project = bundle
            .publishable()
            .expect("valid starter Audition Project");
        let mut module_paths = std::fs::read_dir(root.join("instruments"))
            .expect("starter Instrument directory")
            .map(|entry| entry.expect("starter Instrument entry").path())
            .collect::<Vec<_>>();
        module_paths.sort();
        assert_eq!(module_paths.len(), 9);

        for path in module_paths {
            let source = std::fs::read_to_string(&path).expect("read starter Module");
            let module = validate_module_source(&path, &source).expect("valid starter Module");
            assert!(module.dependencies.iter().all(|dependency| {
                project
                    .assets
                    .iter()
                    .any(|asset| asset.id == dependency.asset_id)
            }));
            let (sample_rate, frames) =
                render_module_preview(&module, &project.assets, &bundle.assets)
                    .expect("render starter Module preview");
            assert_eq!(sample_rate, project.sample_rate);
            assert!(frames.iter().flatten().all(|sample| sample.is_finite()));
            assert!(
                frames.iter().flatten().any(|sample| sample.abs() > 1.0e-5),
                "{} rendered silence",
                path.display()
            );
        }
    }

    #[test]
    fn dependency_inventory_is_sorted_and_deduplicated() {
        let source = include_str!("../fixtures/synth-alpha/minimal-spectral.crump");
        let project = parse_project(Path::new("spectral.crump"), source)
            .project
            .expect("Project");
        let dependencies = module_asset_dependencies(&project.song.instruments[0]);
        assert_eq!(dependencies.len(), 1);
        assert_eq!(dependencies[0].asset_id, "source-texture");
        assert_eq!(dependencies[0].usages, [MediaAssetUsage::Spectral]);
        assert!(dependencies[0].references.len() >= 2);
    }

    #[test]
    fn catalog_filter_uses_search_union_and_tag_intersection() {
        let entry = ModuleCatalogEntry {
            name: "deep-bass".to_owned(),
            source_version: Some(2),
            instrument_id: Some("bass".to_owned()),
            instrument_name: Some("Deep Bass".to_owned()),
            tags: vec!["bass".to_owned(), "analog".to_owned()],
            dependencies: Vec::new(),
            valid: true,
            diagnostics: Vec::new(),
        };
        assert!(catalog_matches(&entry, Some("deep"), &["bass".to_owned()]));
        assert!(catalog_matches(&entry, Some("analog"), &[]));
        assert!(!catalog_matches(&entry, None, &["pad".to_owned()]));
    }
}
