//! Machine-checkable Milestone 0 contract for the native synthesis alpha.
//!
//! These checks freeze proposed source snapshots, exact lawful assets, decisions, and provisional
//! resource evidence. They intentionally do not claim production parser/runtime support or sound
//! quality.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use crump::{
    dsl::{parse_module, parse_project},
    engine::{
        CompiledProject,
        mix_graph::{
            MAX_PUBLICATION_RUNTIME_BYTES, MAX_RESIDENT_PUBLICATION_RUNTIMES,
            MAX_WORK_UNITS_PER_FRAME,
        },
    },
    media::{
        MAX_MEDIA_ASSET_BYTES, MAX_MEDIA_ASSETS, MAX_PROJECT_MEDIA_BYTES, load_asset_inventory,
    },
    model::{NodeKind, SettingValue},
    project::load_project,
    validate::{
        MAX_GRAPH_NODES, MAX_GRAPH_VOICES, MAX_MODULATION_OPERATORS_PER_ROUTE,
        MAX_MODULATION_ROUTES, MAX_MULTISAMPLE_ZONES, MAX_SAMPLE_SLICES, validate_project,
        validate_project_assets,
    },
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Deserialize)]
struct Manifest {
    schema_version: u32,
    authority: String,
    production_compatibility: String,
    proposal_contract_status: String,
    current_compatibility_contract: String,
    current_architecture_decision: String,
    fixture_language: String,
    fixtures: Vec<Fixture>,
    assets: Vec<Asset>,
    limits: Limits,
    qualification_max: QualificationMax,
    source_mode_maxima: SourceModeMaxima,
    decisions: Vec<Decision>,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    path: String,
    sha256: String,
    runnable: bool,
    role: String,
    source_mode: String,
    required_tokens: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    id: String,
    path: String,
    sha256: String,
    frames: u32,
    channels: u16,
    sample_rate: u32,
    uses: Vec<String>,
    origin: String,
    license: String,
}

#[derive(Debug, Deserialize)]
struct Limits {
    primary_sources_per_instrument: usize,
    sources_of_one_asset_kind_per_voice: usize,
    voice_graph_nodes: u16,
    shared_graph_nodes: u16,
    instrument_polyphony: u16,
    project_graph_voices: u32,
    unison_lanes_per_source: usize,
    unison_lanes_per_voice: usize,
    envelopes_per_instrument: usize,
    lfos_per_instrument: usize,
    macros_per_instrument: usize,
    instrument_routes: usize,
    route_operators: usize,
    filters_per_instrument: usize,
    wavetable_frames_per_asset: usize,
    sample_slices_per_source: usize,
    multisample_zones_per_instrument: usize,
    grains_per_source_voice: usize,
    grains_per_voice: usize,
    spectral_peaks_per_frame: usize,
    spectral_sources_per_voice: usize,
    aggregate_work_units_per_frame: u64,
    publication_peak_bytes: u64,
    resident_publication_runtimes: u64,
}

#[derive(Debug, Deserialize)]
struct QualificationMax {
    profile: String,
    path: String,
    polyphony: usize,
    primary_sources: usize,
    unison_lanes_per_voice: usize,
    envelopes: usize,
    lfos: usize,
    macros: usize,
    routes: usize,
    filters: usize,
    grains_per_voice: usize,
    spectral_sources_per_voice: usize,
    spectral_peaks_per_frame: usize,
    estimated_work_units_per_frame: u64,
    estimated_publication_peak_bytes: u64,
    prototype_measurement: String,
    actual_admitted_resources: ActualAdmittedResources,
}

#[derive(Debug, Deserialize)]
struct SourceModeMaxima {
    profile: String,
    path: String,
    polyphony: usize,
    first_rejected_polyphony: usize,
    granular_sources_per_voice: usize,
    grains_per_source_voice: usize,
    grains_per_voice: usize,
    spectral_sources_per_voice: usize,
    spectral_peaks_per_source: usize,
    actual_admitted_resources: ActualAdmittedResources,
}

#[derive(Debug, Deserialize)]
struct ActualAdmittedResources {
    graph_voices: u64,
    graph_node_instances: u64,
    shared_graphs: u64,
    shared_graph_work_units_per_frame: u64,
    control_targets: usize,
    work_units_per_frame: u64,
    publication_transition_work_units_per_frame: u64,
    runtime_bytes: u64,
    publication_peak_bytes: u64,
    admitted: bool,
    feature_domain: String,
}

#[derive(Debug, Deserialize)]
struct Decision {
    id: String,
    status: String,
    disposition: String,
}

#[derive(Debug, Deserialize)]
struct Probe {
    schema_version: u32,
    profile: String,
    sample_rate: u32,
    measured_frames: usize,
    cpu_model: String,
    host_identity: String,
    rustc: String,
    rustc_flags: String,
    probe_source_sha256: String,
    measurement_method: String,
    combined_ns_per_frame_observations: Vec<f64>,
    prototype_utilization_observations: Vec<f64>,
    combined_ns_per_frame: f64,
    frame_deadline_ns: f64,
    prototype_utilization: f64,
    mutable_voice_bytes: u64,
    mutable_runtime_bytes: u64,
    immutable_program_bytes: u64,
    shared_state_bytes: u64,
    publication_peak_bytes: u64,
    work_units_per_frame: u64,
    checksum: f64,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn manifest() -> Manifest {
    let bytes = fs::read(root().join("fixtures/synth-alpha/manifest.json"))
        .expect("read native synthesis alpha manifest");
    serde_json::from_slice(&bytes).expect("parse native synthesis alpha manifest")
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn assert_balanced_blocks(path: &str, source: &str) {
    let mut depth = 0_i32;
    for character in source.chars() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                assert!(depth >= 0, "{path} closes a block before opening it");
            }
            _ => {}
        }
    }
    assert_eq!(depth, 0, "{path} has unbalanced source blocks");
}

fn declarations(source: &str) -> BTreeMap<&str, BTreeSet<&str>> {
    let mut declarations = BTreeMap::<&str, BTreeSet<&str>>::new();
    for line in source.lines() {
        let words = line.split_whitespace().collect::<Vec<_>>();
        if words.len() < 2 {
            continue;
        }
        let keyword = words[0];
        let id = words[1];
        let declaration = match keyword {
            "asset" | "instrument" => words.len() >= 4,
            "parameter" => words.len() >= 8,
            "control" => words.len() >= 6,
            "section" => words.len() >= 4,
            "source" | "route" | "node" | "warp" | "zone" => words.len() >= 3,
            "bus" => words.contains(&"->"),
            "slice" => words.len() >= 4,
            _ => false,
        };
        if declaration {
            assert!(
                declarations.entry(keyword).or_default().insert(id),
                "duplicate {keyword} ID {id}"
            );
        }
    }
    declarations
}

#[test]
fn alpha_contract_inventory_and_decision_dispositions_are_recorded() {
    let root = root();
    let manifest = manifest();
    assert_eq!(manifest.schema_version, 2);
    assert_eq!(
        manifest.authority,
        "mixed-production-and-design-fixture-contract"
    );
    assert_eq!(manifest.production_compatibility, "declared-per-fixture");
    assert_eq!(manifest.proposal_contract_status, "milestone-5-runnable");
    assert_eq!(manifest.fixture_language, "declared-per-fixture");
    assert!(
        root.join(&manifest.current_compatibility_contract)
            .is_file()
    );
    assert!(root.join(&manifest.current_architecture_decision).is_file());
    let current_contract = fs::read_to_string(root.join(&manifest.current_compatibility_contract))
        .expect("read current native synthesis compatibility contract");
    for fact in [
        "`crump-module 1`",
        "`crump-module 2`",
        "`connect FROM -> TO`",
        "`voice_sum.audio` crossing",
        "stable Instrument-owned Modulation sources and routes",
        "`sample_source` and `multisample` Nodes",
        "deterministic stereo `spectral` Node",
        "`2-alpha`",
    ] {
        assert!(
            current_contract.contains(fact),
            "current compatibility contract is missing {fact:?}"
        );
    }
    assert_eq!(manifest.fixtures.len(), 9);
    assert_eq!(
        manifest
            .fixtures
            .iter()
            .filter(|fixture| fixture.runnable)
            .count(),
        8
    );
    assert_eq!(manifest.assets.len(), 4);

    let source_modes = manifest
        .fixtures
        .iter()
        .map(|fixture| fixture.source_mode.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        source_modes,
        BTreeSet::from([
            "integrated",
            "granular",
            "granular-spectral",
            "multisample",
            "sample",
            "spectral",
            "wavetable",
        ])
    );
    assert_eq!(
        manifest
            .fixtures
            .iter()
            .filter(|fixture| fixture.role == "minimal-source")
            .count(),
        5
    );

    let recognized_statuses = [
        "accepted",
        "candidate_needs_producer_validation",
        "deferred_until_demonstrated_gap",
        "qualification_input",
    ];
    let mut decision_ids = BTreeSet::new();
    for decision in &manifest.decisions {
        assert!(decision_ids.insert(decision.id.as_str()));
        assert!(recognized_statuses.contains(&decision.status.as_str()));
        assert!(!decision.disposition.trim().is_empty());
    }
    assert_eq!(decision_ids.len(), 13);
    for required in [
        "wavetable-formats-import-morph-normalization",
        "presentation-source-roles",
        "resource-ceilings",
        "oscillator-warp-order",
        "filter-palette",
        "modulation-sources-curves-aux-order",
        "voice-shared-boundary",
        "sample-loop-slice-rate-sfz",
        "granular-algorithm",
        "spectral-algorithm",
        "missing-creative-processors",
        "starter-content-policy",
        "producer-reference-hardware-protocol",
    ] {
        assert!(
            decision_ids.contains(required),
            "missing decision {required}"
        );
    }

    let limits = &manifest.limits;
    assert_eq!(limits.primary_sources_per_instrument, 8);
    assert_eq!(limits.sources_of_one_asset_kind_per_voice, 4);
    assert_eq!(limits.voice_graph_nodes, MAX_GRAPH_NODES);
    assert_eq!(limits.shared_graph_nodes, MAX_GRAPH_NODES);
    assert_eq!(limits.instrument_polyphony, 64);
    assert_eq!(limits.project_graph_voices, MAX_GRAPH_VOICES);
    assert_eq!(limits.unison_lanes_per_source, 8);
    assert_eq!(limits.unison_lanes_per_voice, 32);
    assert_eq!(limits.envelopes_per_instrument, 8);
    assert_eq!(limits.lfos_per_instrument, 16);
    assert_eq!(limits.macros_per_instrument, 8);
    assert_eq!(limits.instrument_routes, MAX_MODULATION_ROUTES);
    assert_eq!(limits.route_operators, MAX_MODULATION_OPERATORS_PER_ROUTE);
    assert_eq!(limits.filters_per_instrument, 8);
    assert_eq!(limits.wavetable_frames_per_asset, 256);
    assert_eq!(limits.sample_slices_per_source, MAX_SAMPLE_SLICES);
    assert_eq!(
        limits.multisample_zones_per_instrument,
        MAX_MULTISAMPLE_ZONES
    );
    assert_eq!(limits.grains_per_source_voice, 64);
    assert_eq!(limits.grains_per_voice, 128);
    assert_eq!(limits.spectral_peaks_per_frame, 128);
    assert_eq!(limits.spectral_sources_per_voice, 2);
    assert_eq!(
        limits.aggregate_work_units_per_frame,
        MAX_WORK_UNITS_PER_FRAME
    );
    assert_eq!(limits.publication_peak_bytes, MAX_PUBLICATION_RUNTIME_BYTES);
    assert_eq!(
        limits.resident_publication_runtimes,
        MAX_RESIDENT_PUBLICATION_RUNTIMES
    );
    assert_eq!(MAX_MEDIA_ASSETS, 1_024);
    assert_eq!(MAX_MEDIA_ASSET_BYTES, 64 * 1_048_576);
    assert_eq!(MAX_PROJECT_MEDIA_BYTES, 1_024 * 1_048_576);
}

#[test]
fn fixtures_are_readable_unique_and_only_completed_slices_are_runnable() {
    let root = root();
    let manifest = manifest();
    for fixture in &manifest.fixtures {
        let path = root.join(&fixture.path);
        let bytes = fs::read(&path).expect("read alpha source fixture");
        assert_eq!(sha256(&bytes), fixture.sha256);
        let source = String::from_utf8(bytes).expect("synthesis source fixture is UTF-8");
        if fixture.runnable {
            assert!(
                source.starts_with("crump 2\n") || source.starts_with("crump-module 2\n"),
                "{} must use a stable source-v2 header",
                fixture.path
            );
        } else {
            assert!(
                source.starts_with("crump-module 2-alpha\n"),
                "{} must remain a fail-closed design snapshot",
                fixture.path
            );
        }
        assert_balanced_blocks(&fixture.path, &source);
        for token in &fixture.required_tokens {
            assert!(
                source.contains(token),
                "{} is missing required contract token {token:?}",
                fixture.path
            );
        }
        let declarations = declarations(&source);
        assert!(
            declarations
                .get("instrument")
                .is_some_and(|ids| ids.len() == 1)
        );
        assert!(declarations.get("node").is_some_and(|ids| !ids.is_empty()));
        assert!(declarations.get("bus").is_some_and(|ids| !ids.is_empty()));

        let diagnostics = if source.starts_with("crump-module") {
            parse_module(&path, &source).diagnostics
        } else {
            parse_project(&path, &source).diagnostics
        };
        assert_eq!(
            diagnostics.has_errors(),
            !fixture.runnable,
            "{} has the wrong parser status: {diagnostics:?}",
            fixture.path
        );
        if fixture.runnable {
            let parsed = parse_project(&path, &source);
            let project = parsed.project.expect("runnable fixture Project");
            let diagnostics = validate_project(&project);
            assert!(
                !diagnostics.has_errors(),
                "{} failed model validation: {diagnostics:?}",
                fixture.path
            );
            let (assets, diagnostics) =
                load_asset_inventory(path.parent().expect("fixture directory"), &project.assets);
            assert!(
                !diagnostics.has_errors(),
                "{} failed asset inventory loading: {diagnostics:?}",
                fixture.path
            );
            let diagnostics = validate_project_assets(&project, &assets);
            assert!(
                !diagnostics.has_errors(),
                "{} failed exact-asset validation: {diagnostics:?}",
                fixture.path
            );
            CompiledProject::compile_with_assets(&project, &assets)
                .expect("runnable fixture must compile");
        }
    }
}

#[test]
fn lawful_fixture_assets_match_exact_hash_container_and_provenance() {
    let root = root();
    let manifest = manifest();
    let all_fixture_source = manifest
        .fixtures
        .iter()
        .map(|fixture| fs::read_to_string(root.join(&fixture.path)).expect("read fixture"))
        .collect::<String>();
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for asset in &manifest.assets {
        assert!(ids.insert(asset.id.as_str()));
        assert!(paths.insert(asset.path.as_str()));
        let path = root.join(&asset.path);
        let bytes = fs::read(&path).expect("read alpha WAV asset");
        assert_eq!(sha256(&bytes), asset.sha256);
        assert!(all_fixture_source.contains(&asset.sha256));
        let reader = hound::WavReader::open(&path).expect("open alpha WAV asset");
        assert_eq!(reader.spec().channels, asset.channels);
        assert_eq!(reader.spec().sample_rate, asset.sample_rate);
        assert_eq!(reader.duration(), asset.frames);
        assert!(!asset.uses.is_empty());
        assert!(asset.origin.contains("generate_assets.py"));
        assert_eq!(asset.license, "MIT");
    }
}

#[test]
fn qualification_max_profile_matches_fixture_and_probe_evidence() {
    let root = root();
    let manifest = manifest();
    let maximum = &manifest.qualification_max;
    assert_eq!(maximum.profile, "qualification-max-v1");
    assert_eq!(maximum.path, "fixtures/synth-alpha/maximum-topology.crump");
    assert!(maximum.polyphony <= usize::from(manifest.limits.instrument_polyphony));
    assert!(maximum.primary_sources <= manifest.limits.primary_sources_per_instrument);
    assert!(maximum.unison_lanes_per_voice <= manifest.limits.unison_lanes_per_voice);
    assert!(maximum.envelopes <= manifest.limits.envelopes_per_instrument);
    assert!(maximum.lfos <= manifest.limits.lfos_per_instrument);
    assert!(maximum.macros <= manifest.limits.macros_per_instrument);
    assert!(maximum.routes <= manifest.limits.instrument_routes);
    assert!(maximum.filters <= manifest.limits.filters_per_instrument);
    assert!(maximum.grains_per_voice <= manifest.limits.grains_per_voice);
    assert!(maximum.spectral_sources_per_voice <= manifest.limits.spectral_sources_per_voice);
    assert!(maximum.spectral_peaks_per_frame <= manifest.limits.spectral_peaks_per_frame);
    assert!(
        maximum.estimated_work_units_per_frame <= manifest.limits.aggregate_work_units_per_frame
    );
    assert!(maximum.estimated_publication_peak_bytes <= manifest.limits.publication_peak_bytes);

    let source = fs::read_to_string(root.join(&maximum.path)).expect("read maximum fixture");
    assert_eq!(
        source
            .lines()
            .filter(|line| line.starts_with("        route route-"))
            .count(),
        64
    );
    assert_eq!(
        source
            .lines()
            .filter(|line| line.starts_with("        source lfo-"))
            .count(),
        16
    );
    assert_eq!(
        source
            .lines()
            .filter(|line| line.starts_with("      parameter macro-"))
            .count(),
        8
    );
    assert_eq!(
        source
            .lines()
            .filter(|line| line.starts_with("        node envelope-"))
            .count(),
        8
    );
    assert_eq!(
        source
            .lines()
            .filter(|line| line.starts_with("        node filter-"))
            .count(),
        8
    );
    assert_eq!(
        source
            .lines()
            .filter(|line| {
                line.starts_with("        node source-") && !line.contains("source-sum")
            })
            .count(),
        8
    );

    let bundle = load_project(root.join(&maximum.path)).expect("load qualification maximum");
    let project = bundle.publishable().expect("valid qualification maximum");
    let compiled = CompiledProject::compile_with_assets(project, &bundle.assets)
        .expect("compile qualification maximum");
    let actual = &maximum.actual_admitted_resources;
    let resources = &compiled.mix_graph_inspection().resources;
    assert_eq!(actual.graph_voices, resources.graph_voices);
    assert_eq!(actual.graph_node_instances, resources.graph_node_instances);
    assert_eq!(actual.shared_graphs, resources.shared_graphs);
    assert_eq!(
        actual.shared_graph_work_units_per_frame,
        resources.shared_graph_work_units_per_frame
    );
    assert_eq!(actual.control_targets, resources.control_targets);
    assert_eq!(actual.work_units_per_frame, resources.work_units_per_frame);
    assert_eq!(
        actual.publication_transition_work_units_per_frame,
        resources.publication_transition_work_units_per_frame
    );
    assert_eq!(actual.admitted, resources.admitted);
    assert_eq!(actual.feature_domain, "pipewire-enabled production build");
    if cfg!(feature = "pipewire") {
        assert_eq!(actual.runtime_bytes, resources.runtime_bytes);
        assert_eq!(
            actual.publication_peak_bytes,
            resources.publication_peak_bytes
        );
    }

    let probe_bytes = fs::read(root.join(&maximum.prototype_measurement)).expect("read probe");
    let probe: Probe = serde_json::from_slice(&probe_bytes).expect("parse probe");
    assert_eq!(probe.schema_version, 1);
    assert_eq!(probe.profile, maximum.profile);
    assert_eq!(probe.sample_rate, 48_000);
    assert!(probe.measured_frames >= 24_000);
    assert!(!probe.cpu_model.trim().is_empty());
    assert!(!probe.host_identity.trim().is_empty());
    assert!(probe.rustc.starts_with("rustc "));
    assert_eq!(
        probe.rustc_flags,
        "--edition 2024 -C opt-level=3 -C target-cpu=native"
    );
    let probe_source = fs::read(root.join("prototypes/synth-alpha/resource_probe.rs"))
        .expect("read resource probe source");
    assert_eq!(probe.probe_source_sha256, sha256(&probe_source));
    assert_eq!(
        probe.measurement_method,
        "median-of-five-consecutive-processes"
    );
    assert_eq!(probe.combined_ns_per_frame_observations.len(), 5);
    assert_eq!(probe.prototype_utilization_observations.len(), 5);
    for (combined, utilization) in probe
        .combined_ns_per_frame_observations
        .iter()
        .zip(&probe.prototype_utilization_observations)
    {
        assert!((combined / probe.frame_deadline_ns - utilization).abs() < 0.000_001);
    }
    let mut combined_observations = probe.combined_ns_per_frame_observations.clone();
    combined_observations.sort_by(f64::total_cmp);
    assert_eq!(probe.combined_ns_per_frame, combined_observations[2]);
    assert!(
        probe
            .prototype_utilization_observations
            .contains(&probe.prototype_utilization)
    );
    assert!(
        probe
            .prototype_utilization_observations
            .iter()
            .all(|value| value.is_finite() && *value > 0.0 && *value < 1.0)
    );
    assert!(probe.combined_ns_per_frame.is_finite() && probe.combined_ns_per_frame > 0.0);
    assert!(probe.frame_deadline_ns > probe.combined_ns_per_frame);
    assert!(probe.prototype_utilization > 0.0 && probe.prototype_utilization < 1.0);
    assert!(probe.mutable_voice_bytes > 0);
    assert!(probe.mutable_runtime_bytes > probe.mutable_voice_bytes);
    assert!(probe.immutable_program_bytes > 0);
    assert!(probe.shared_state_bytes > 0);
    assert_eq!(
        probe.publication_peak_bytes,
        maximum.estimated_publication_peak_bytes
    );
    assert_eq!(
        probe.work_units_per_frame,
        maximum.estimated_work_units_per_frame
    );
    assert!(probe.checksum.is_finite());
}

#[test]
fn independent_granular_and_spectral_maxima_hit_caps_and_admission_boundary() {
    let root = root();
    let manifest = manifest();
    let maximum = &manifest.source_mode_maxima;
    assert_eq!(maximum.profile, "granular-spectral-independent-maxima-v1");
    assert_eq!(
        maximum.path,
        "fixtures/synth-alpha/maximum-granular-spectral.crump"
    );
    assert_eq!(maximum.polyphony, 5);
    assert_eq!(maximum.first_rejected_polyphony, 6);
    assert_eq!(maximum.granular_sources_per_voice, 2);
    assert_eq!(
        maximum.grains_per_source_voice,
        manifest.limits.grains_per_source_voice
    );
    assert_eq!(maximum.grains_per_voice, manifest.limits.grains_per_voice);
    assert_eq!(
        maximum.spectral_sources_per_voice,
        manifest.limits.spectral_sources_per_voice
    );
    assert_eq!(
        maximum.spectral_peaks_per_source,
        manifest.limits.spectral_peaks_per_frame
    );

    let bundle = load_project(root.join(&maximum.path)).expect("load source-mode maxima");
    let project = bundle.publishable().expect("valid source-mode maxima");
    let instrument = &project.song.instruments[0];
    assert_eq!(usize::from(instrument.polyphony), maximum.polyphony);
    let graph = instrument.graph.as_ref().expect("voice graph");
    let granular = graph
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Granular)
        .collect::<Vec<_>>();
    let spectral = graph
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Spectral)
        .collect::<Vec<_>>();
    assert_eq!(granular.len(), maximum.granular_sources_per_voice);
    assert_eq!(spectral.len(), maximum.spectral_sources_per_voice);
    for node in granular {
        assert_eq!(
            node.setting("max_grains"),
            Some(&SettingValue::Number(
                maximum.grains_per_source_voice as f64
            ))
        );
    }
    for node in spectral {
        assert_eq!(
            node.setting("max_peaks"),
            Some(&SettingValue::Number(
                maximum.spectral_peaks_per_source as f64
            ))
        );
    }

    let compiled = CompiledProject::compile_with_assets(project, &bundle.assets)
        .expect("compile admitted source-mode maxima");
    let actual = &maximum.actual_admitted_resources;
    let resources = &compiled.mix_graph_inspection().resources;
    assert!(resources.admitted);
    assert!(resources.work_units_per_frame <= resources.work_budget_per_frame);
    assert!(
        resources.publication_transition_work_units_per_frame <= resources.work_budget_per_frame
    );
    assert!(resources.publication_peak_bytes <= resources.publication_budget_bytes);
    assert_eq!(actual.graph_voices, resources.graph_voices);
    assert_eq!(actual.graph_node_instances, resources.graph_node_instances);
    assert_eq!(actual.shared_graphs, resources.shared_graphs);
    assert_eq!(
        actual.shared_graph_work_units_per_frame,
        resources.shared_graph_work_units_per_frame
    );
    assert_eq!(actual.control_targets, resources.control_targets);
    assert_eq!(actual.work_units_per_frame, resources.work_units_per_frame);
    assert_eq!(
        actual.publication_transition_work_units_per_frame,
        resources.publication_transition_work_units_per_frame
    );
    assert_eq!(actual.admitted, resources.admitted);
    assert_eq!(actual.feature_domain, "pipewire-enabled production build");
    if cfg!(feature = "pipewire") {
        assert_eq!(actual.runtime_bytes, resources.runtime_bytes);
        assert_eq!(
            actual.publication_peak_bytes,
            resources.publication_peak_bytes
        );
    }

    let mut rejected = project.clone();
    rejected.song.instruments[0].polyphony =
        u16::try_from(maximum.first_rejected_polyphony).expect("bounded polyphony");
    let rejection = validate_project(&rejected);
    assert!(
        rejection.as_slice().iter().any(|diagnostic| {
            diagnostic.code == "CRV093" && diagnostic.message.contains("work units/frame")
        }),
        "unexpected rejection: {rejection:?}"
    );
    assert!(
        CompiledProject::compile_with_assets(&rejected, &bundle.assets).is_err(),
        "first rejected polyphony unexpectedly admitted"
    );
}
