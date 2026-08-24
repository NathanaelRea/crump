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
    dsl::parse_module,
    engine::mix_graph::{
        MAX_PUBLICATION_RUNTIME_BYTES, MAX_RESIDENT_PUBLICATION_RUNTIMES, MAX_WORK_UNITS_PER_FRAME,
    },
    media::{MAX_MEDIA_ASSET_BYTES, MAX_MEDIA_ASSETS, MAX_PROJECT_MEDIA_BYTES},
    validate::{
        MAX_GRAPH_NODES, MAX_GRAPH_VOICES, MAX_MODULATION_OPERATORS_PER_ROUTE,
        MAX_MODULATION_ROUTES,
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
    decisions: Vec<Decision>,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    path: String,
    sha256: String,
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
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.authority, "non-normative-design-snapshot");
    assert_eq!(
        manifest.production_compatibility,
        "unsupported-by-crump-module-1"
    );
    assert_eq!(
        manifest.proposal_contract_status,
        "active-gap-closure-implementation"
    );
    assert_eq!(manifest.fixture_language, "crump-module 2-alpha");
    assert!(
        root.join(&manifest.current_compatibility_contract)
            .is_file()
    );
    assert!(root.join(&manifest.current_architecture_decision).is_file());
    let current_contract = fs::read_to_string(root.join(&manifest.current_compatibility_contract))
        .expect("read current native synthesis compatibility contract");
    for fact in [
        "`crump-module 1`",
        "shared post-voice graph declaration",
        "Instrument-owned modulation-source or modulation-route matrix",
        "graph wavetable, granular, multisample, or spectral source Node",
    ] {
        assert!(
            current_contract.contains(fact),
            "current compatibility contract is missing {fact:?}"
        );
    }
    assert!(!current_contract.contains("crump-module 2-alpha"));
    assert_eq!(manifest.fixtures.len(), 7);
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
    assert_eq!(limits.multisample_zones_per_instrument, 256);
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
    assert_eq!(MAX_MEDIA_ASSETS, 256);
    assert_eq!(MAX_MEDIA_ASSET_BYTES, 64 * 1_048_576);
    assert_eq!(MAX_PROJECT_MEDIA_BYTES, 256 * 1_048_576);
}

#[test]
fn fixtures_are_readable_unique_and_deliberately_not_production_source_v1() {
    let root = root();
    let manifest = manifest();
    for fixture in &manifest.fixtures {
        let path = root.join(&fixture.path);
        let bytes = fs::read(&path).expect("read alpha source fixture");
        assert_eq!(sha256(&bytes), fixture.sha256);
        let source = String::from_utf8(bytes).expect("alpha source fixture is UTF-8");
        assert!(source.starts_with("crump-module 2-alpha\n"));
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

        let parsed = parse_module(&path, &source);
        assert!(
            parsed.diagnostics.has_errors(),
            "reserved alpha fixture unexpectedly entered production source-v1 parsing"
        );
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
        assert_eq!(asset.license, "MPL-2.0");
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
            .filter(|line| line.starts_with("    route route-"))
            .count(),
        64
    );
    assert_eq!(
        source
            .lines()
            .filter(|line| line.starts_with("    source lfo-"))
            .count(),
        16
    );
    assert_eq!(
        source
            .lines()
            .filter(|line| line.starts_with("  parameter macro-"))
            .count(),
        8
    );
    assert_eq!(
        source
            .lines()
            .filter(|line| line.starts_with("    node envelope-"))
            .count(),
        8
    );
    assert_eq!(
        source
            .lines()
            .filter(|line| line.starts_with("    node filter-"))
            .count(),
        8
    );
    assert_eq!(
        source
            .lines()
            .filter(|line| line.starts_with("    node source-") && !line.contains("source-sum"))
            .count(),
        8
    );

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
