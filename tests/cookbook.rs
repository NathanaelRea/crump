//! Machine-checkable contracts for the shared human/Agent production cookbook.
//!
//! These tests prove source support metadata, runnable current-capability
//! examples, deterministic engine behavior, and failure-atomic lifecycle use.
//! They do not provide musical-quality or proprietary-device equivalence evidence.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use crump::{
    engine::{CompiledProject, ProcessingMode, RenderOptions, render_project},
    history::{ChangeOrigin, HistoryStore, Publication},
    project::load_project,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

#[derive(Debug, Deserialize)]
struct Matrix {
    schema_version: u32,
    source: String,
    support_counts: BTreeMap<String, usize>,
    recipes: Vec<Recipe>,
}

#[derive(Debug, Deserialize)]
struct Recipe {
    id: usize,
    slug: String,
    title: String,
    support: String,
    source_assumptions: Vec<String>,
    current_primitives: Vec<String>,
    workaround: String,
    missing_primitives: Vec<String>,
    packaging_layer: String,
    minimum_crump_version: String,
    cookbook_page: Option<String>,
    fixture: Option<String>,
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn load_matrix() -> Matrix {
    let path = repository_root().join("docs/cookbook/feasibility.json");
    let bytes = fs::read(path).expect("read cookbook matrix");
    serde_json::from_slice(&bytes).expect("parse cookbook matrix")
}

#[test]
fn feasibility_matrix_is_complete_exact_and_linked() {
    let root = repository_root();
    let matrix = load_matrix();
    assert_eq!(matrix.schema_version, 1);
    assert_eq!(matrix.source, "https://www.youtube.com/watch?v=D5dc7mQ30hU");
    assert_eq!(matrix.recipes.len(), 100);
    assert_eq!(
        matrix.support_counts,
        BTreeMap::from([
            ("approximation".to_owned(), 21),
            ("blocked".to_owned(), 14),
            ("direct".to_owned(), 64),
            ("intentional-non-goal".to_owned(), 1),
        ])
    );
    let identity = matrix
        .recipes
        .iter()
        .map(|recipe| format!("{}\t{}\n", recipe.id, recipe.title))
        .collect::<String>();
    let identity_hash = Sha256::digest(identity.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        identity_hash, "8698cadf553059f870db4609c18e65c077f924a41f82c8d6ab9efe0721f5d57e",
        "the self-contained canonical recipe identity list changed without review"
    );
    let support_values = ["direct", "approximation", "blocked", "intentional-non-goal"];
    let source_values = [
        "native-instrument",
        "sample",
        "vocal",
        "drums",
        "track",
        "return",
        "group",
        "master",
    ];
    let packaging_values = [
        "project-recipe",
        "effect-module-candidate",
        "native-capability",
        "plugin-candidate",
        "intentional-non-goal",
    ];
    let selected_required: BTreeMap<usize, &[&str]> = BTreeMap::from([
        (4, &["eq", "saturation", "send-return-routing"][..]),
        (10, &["delay", "saturation"][..]),
        (17, &["eq", "automation"][..]),
        (35, &["sampler", "eq", "reverb", "delay"][..]),
        (42, &["reverb", "automation"][..]),
        (44, &["delay", "send-return-routing"][..]),
        (48, &["eq", "delay", "send-return-routing"][..]),
        (84, &["eq", "reverb"][..]),
    ]);
    let headings = [
        "## 1. Musical intent and suitable material",
        "## 2. Source kinds",
        "## 3. Required capabilities and minimum version",
        "## 4. Current behavior and limits",
        "## 5. Ordered source/routing/processor actions",
        "## 6. Tempo-relative calculations and physical-unit ranges",
        "## 7. Automation or Modulation shape",
        "## 8. Audition and A/B procedure",
        "## 9. Failure modes and gain staging",
        "## 10. Listening questions",
        "## 11. Validation and publication",
        "## 12. Reuse boundary",
    ];
    let mut ids = BTreeSet::new();
    let mut slugs = BTreeSet::new();
    let mut counted = BTreeMap::<String, usize>::new();
    let mut linked = 0;
    for (index, recipe) in matrix.recipes.iter().enumerate() {
        assert_eq!(recipe.id, index + 1);
        assert!(ids.insert(recipe.id));
        assert!(
            slugs.insert(recipe.slug.as_str()),
            "duplicate slug {}",
            recipe.slug
        );
        assert!(!recipe.title.trim().is_empty());
        assert!(support_values.contains(&recipe.support.as_str()));
        assert!(!recipe.source_assumptions.is_empty());
        assert!(
            recipe
                .source_assumptions
                .iter()
                .all(|value| source_values.contains(&value.as_str()))
        );
        assert!(!recipe.current_primitives.is_empty());
        assert!(!recipe.workaround.trim().is_empty());
        assert!(
            !recipe
                .missing_primitives
                .iter()
                .any(|primitive| primitive == "direct-audio-transform-or-checked-generation")
        );
        assert!(packaging_values.contains(&recipe.packaging_layer.as_str()));
        assert_eq!(recipe.minimum_crump_version, env!("CARGO_PKG_VERSION"));
        match recipe.support.as_str() {
            "direct" => assert!(recipe.missing_primitives.is_empty()),
            _ => assert!(!recipe.missing_primitives.is_empty()),
        }
        *counted.entry(recipe.support.clone()).or_default() += 1;

        match (&recipe.cookbook_page, &recipe.fixture) {
            (Some(page), Some(fixture)) => {
                linked += 1;
                assert_eq!(recipe.support, "direct");
                let page_source = fs::read_to_string(root.join(page)).expect("linked recipe page");
                assert!(page_source.contains(&format!("recipe_id: {}", recipe.id)));
                assert!(page_source.contains(&format!("slug: {}", recipe.slug)));
                assert!(page_source.contains(&format!("support: {}", recipe.support)));
                assert!(page_source.contains(&format!(
                    "minimum_crump_version: {}",
                    recipe.minimum_crump_version
                )));
                assert!(page_source.contains(&format!("fixture: {fixture}")));
                for heading in headings {
                    assert!(
                        page_source.contains(heading),
                        "recipe {} page is missing `{heading}`",
                        recipe.id
                    );
                }
                for primitive in selected_required
                    .get(&recipe.id)
                    .expect("linked recipe capability contract")
                    .iter()
                {
                    assert!(
                        recipe
                            .current_primitives
                            .iter()
                            .any(|value| value == primitive),
                        "recipe {} omits selected capability `{primitive}`",
                        recipe.id
                    );
                }
                assert!(root.join(fixture).join("project.crump").is_file());
            }
            (None, None) => {}
            _ => panic!("recipe {} must link both page and fixture", recipe.id),
        }
    }
    assert_eq!(linked, 8);
    assert_eq!(counted, matrix.support_counts);
    let non_goal = matrix
        .recipes
        .iter()
        .find(|recipe| recipe.support == "intentional-non-goal")
        .expect("one intentional non-goal");
    assert_eq!(non_goal.id, 88);
}

fn process_prefix(compiled: &CompiledProject, quantum: usize, frames: usize) -> Vec<[f32; 2]> {
    let mut runtime = compiled.runtime();
    let mut output = Vec::with_capacity(frames);
    let mut block = vec![[0.0_f32; 2]; quantum];
    while output.len() < frames {
        let count = (frames - output.len()).min(quantum);
        let processed = runtime
            .process_block(&mut block[..count], ProcessingMode::Offline)
            .expect("process cookbook fixture");
        assert_eq!(processed, count);
        output.extend_from_slice(&block[..count]);
    }
    output
}

#[test]
fn selected_fixtures_compile_sound_and_group_deterministically() {
    let root = repository_root();
    let matrix = load_matrix();
    let fixtures = matrix
        .recipes
        .iter()
        .filter_map(|recipe| recipe.fixture.as_deref())
        .collect::<Vec<_>>();
    let mut covers_return = false;
    let mut covers_group = false;
    let mut covers_master = false;
    let mut covers_automation = false;
    let mut covers_sample = false;
    for fixture in fixtures {
        let bundle = load_project(root.join(fixture)).expect("load cookbook fixture");
        assert!(
            !bundle.diagnostics.has_errors(),
            "{fixture}: {:?}",
            bundle.diagnostics.as_slice()
        );
        let project = bundle.project.as_ref().expect("parsed cookbook Project");
        let compiled = CompiledProject::compile_with_assets(project, &bundle.assets)
            .expect("compile cookbook fixture");
        assert!(compiled.mix_graph_inspection().resources.admitted);
        let at_64 = process_prefix(&compiled, 64, 48_000);
        assert_eq!(at_64, process_prefix(&compiled, 128, 48_000), "{fixture}");
        assert_eq!(at_64, process_prefix(&compiled, 256, 48_000), "{fixture}");
        assert!(at_64.iter().flatten().all(|sample| sample.is_finite()));
        assert!(
            at_64.iter().flatten().any(|sample| sample.abs() > 1.0e-5),
            "{fixture} must produce audible output"
        );
        covers_return |= !project.song.returns.is_empty();
        covers_group |= !project.song.groups.is_empty();
        covers_master |= project
            .song
            .master
            .as_ref()
            .is_some_and(|master| !master.inserts.is_empty());
        covers_automation |= !project.song.automations.is_empty();
        covers_sample |= !project.assets.is_empty();
    }
    assert!(covers_return && covers_group && covers_master && covers_automation && covers_sample);
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create fixture copy directory");
    for entry in fs::read_dir(source).expect("read fixture directory") {
        let entry = entry.expect("fixture directory entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type().expect("fixture entry type").is_dir() {
            copy_tree(&source_path, &destination_path);
        } else {
            fs::copy(source_path, destination_path).expect("copy fixture file");
        }
    }
}

#[test]
fn selected_fixtures_publish_reopen_and_render_deterministically() {
    let root = repository_root();
    let matrix = load_matrix();
    for recipe in matrix
        .recipes
        .iter()
        .filter(|recipe| recipe.fixture.is_some())
    {
        let fixture = recipe.fixture.as_deref().expect("selected fixture");
        let temporary = tempdir().expect("temporary cookbook lifecycle root");
        let project_root = temporary.path().join("project");
        copy_tree(&root.join(fixture), &project_root);
        let candidate = load_project(&project_root).expect("load copied cookbook fixture");
        let store = HistoryStore::open(&project_root).expect("open fixture History");
        let entry = store
            .publish(
                &candidate,
                Publication::new(
                    ChangeOrigin::system("cookbook test"),
                    format!("Publish recipe {} fixture", recipe.id),
                    "Disposable lifecycle verification",
                ),
            )
            .expect("publish copied cookbook fixture");
        assert_eq!(entry.revision, candidate.revision);
        drop(store);

        let reopened = HistoryStore::open(&project_root)
            .expect("reopen fixture History")
            .active_bundle()
            .expect("load Active fixture")
            .expect("fixture has Active Revision");
        assert_eq!(reopened.revision, candidate.revision);
        let first = render_project(
            &reopened,
            &RenderOptions::new(temporary.path().join("render-a")),
        )
        .expect("render copied cookbook fixture");
        let second = render_project(
            &reopened,
            &RenderOptions::new(temporary.path().join("render-b")),
        )
        .expect("repeat copied cookbook render");
        assert_eq!(first.manifest.wav_sha256, second.manifest.wav_sha256);
        assert_eq!(first.manifest_sha256, second.manifest_sha256);
        assert_eq!(first.manifest.project_revision, candidate.revision);
        assert_eq!(first.manifest.analysis.non_finite_sample_count, 0);
    }
}
