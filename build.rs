//! Computes `CRUMP_ENGINE_FINGERPRINT`, which keys disposable rendered-audio caches.
//!
//! A cache must not outlive a change to the code that produced its audio. Hashing the whole
//! executable would also throw caches away for every interface or CLI change, so this hashes
//! every source file except those that only present, edit, or load Projects, plus the toolchain,
//! target, profile, and locked dependency versions. Each excluded module only produces or
//! displays the Project model, and cache keys already hash that model.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use sha2::{Digest, Sha256};

/// Source paths, relative to `src/`, that cannot change rendered audio.
const EXCLUDED: &[&str] = &[
    "cli",
    "dsl",
    "edit",
    "workspace",
    "history.rs",
    "main.rs",
    "production_view.rs",
    "project.rs",
    "project_picker.rs",
];

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets the manifest"));
    let source = root.join("src");
    let mut files = Vec::new();
    collect(&source, &source, &mut files);
    files.sort();

    let mut hash = Sha256::new();
    for relative in &files {
        hash.update(relative.to_string_lossy().as_bytes());
        hash.update([0]);
        hash.update(fs::read(source.join(relative)).expect("readable source file"));
    }
    if let Ok(lock) = fs::read(root.join("Cargo.lock")) {
        hash.update(lock);
    }
    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    if let Ok(output) = Command::new(rustc).arg("-vV").output() {
        hash.update(output.stdout);
    }
    for variable in ["TARGET", "PROFILE", "CARGO_CFG_TARGET_FEATURE"] {
        hash.update(env::var(variable).unwrap_or_default());
        hash.update([0]);
    }
    let digest = hash.finalize();
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    println!("cargo:rustc-env=CRUMP_ENGINE_FINGERPRINT={hex}");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-changed=build.rs");
}

fn collect(source: &Path, directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("readable source directory") {
        let path = entry.expect("readable source entry").path();
        let relative = path.strip_prefix(source).expect("inside src").to_owned();
        if EXCLUDED
            .iter()
            .any(|excluded| relative == Path::new(excluded))
        {
            continue;
        }
        if path.is_dir() {
            collect(source, &path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(relative);
        }
    }
}
