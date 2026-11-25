//! Renders running at the same time, or told to stop, behave like one render run alone.
//!
//! Every test drives the real executable as separate processes, the way a build script that
//! renders many parts in parallel does.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};

use rustix::process::{Pid, Signal, kill_process};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const MINIMAL: &str = include_str!("fixtures/minimal.crump");

fn crump(executable: &Path, project: &Path, data_home: &Path) -> Command {
    let mut command = Command::new(executable);
    command
        .arg("--project")
        .arg(project)
        .env("XDG_DATA_HOME", data_home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn executable() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_crump"))
}

/// Writes and publishes one Project under `root`, returning its directory.
fn published_project(root: &Path, name: &str, source: &str) -> PathBuf {
    let project = root.join(name);
    fs::create_dir(&project).expect("Project directory");
    fs::write(project.join("project.crump"), source).expect("Project source");
    let published = crump(&executable(), &project, &root.join("xdg-data"))
        .args([
            "--json",
            "publish",
            "--summary",
            "Concurrent render fixture",
        ])
        .output()
        .expect("publish");
    assert!(published.status.success(), "{published:?}");
    project
}

/// A Project that keeps rendering long enough to be signalled or outlive its executable.
fn long_source(bars: u32) -> String {
    MINIMAL.replacen("end 1bar", &format!("end {bars}bar"), 1)
}

fn json(output: &Output) -> Value {
    let stdout = String::from_utf8(output.stdout.clone()).expect("UTF-8 stdout");
    assert_eq!(stdout.lines().count(), 1, "one JSON response: {stdout:?}");
    serde_json::from_str(stdout.trim()).expect("JSON response")
}

fn sha256(path: &Path) -> String {
    Sha256::digest(fs::read(path).expect("read artifact"))
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn staging_directories(project: &Path) -> Vec<PathBuf> {
    fs::read_dir(project)
        .expect("Project entries")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().contains(".rendering-"))
        })
        .collect()
}

/// Waits until `child` is inside its frame loop, where its spools are growing.
fn wait_until_rendering(child: &mut Child, project: &Path) {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let rendering = staging_directories(project).iter().any(|staging| {
            fs::read_dir(staging).is_ok_and(|entries| {
                entries
                    .flatten()
                    .any(|entry| entry.metadata().is_ok_and(|metadata| metadata.len() > 0))
            })
        });
        if rendering {
            return;
        }
        if let Some(status) = child.try_wait().expect("poll render") {
            panic!("render exited with {status} before it could be observed");
        }
        assert!(Instant::now() < deadline, "render never started writing");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn concurrent_render_processes_all_succeed_with_the_output_of_a_lone_render() {
    let root = TempDir::new().expect("root");
    let data_home = root.path().join("xdg-data");
    let reference_project = published_project(root.path(), "reference", MINIMAL);
    let reference = crump(&executable(), &reference_project, &data_home)
        .args(["--json", "render", "--output", "render"])
        .args(["--stem", "track/lead:post_fader"])
        .output()
        .expect("lone render");
    assert!(reference.status.success(), "{reference:?}");
    let master = sha256(&reference_project.join("render/master.wav"));
    let stem = sha256(&reference_project.join("render/stem-track-lead-post_fader.wav"));

    let projects = (0..6)
        .map(|index| published_project(root.path(), &format!("part-{index}"), MINIMAL))
        .collect::<Vec<_>>();
    // Each Project renders twice at once: both to one shared destination, the way two build
    // steps for the same part would collide, with every Project sharing one data directory.
    let renders = projects
        .iter()
        .flat_map(|project| [project, project])
        .map(|project| {
            let child = crump(&executable(), project, &data_home)
                .args(["--json", "render", "--output", "render"])
                .args(["--stem", "track/lead:post_fader"])
                .spawn()
                .expect("spawn render");
            (project, child)
        })
        .collect::<Vec<_>>();
    for (project, child) in renders {
        let output = child.wait_with_output().expect("render exit");
        assert_eq!(
            output.status.code(),
            Some(0),
            "render in {} failed: stdout {:?} stderr {:?}",
            project.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(output.stderr.is_empty());
        let response = json(&output);
        assert_eq!(response["ok"], true);
        assert_eq!(response["data"]["render"]["manifest"]["wav_sha256"], master);
    }
    for project in &projects {
        assert_eq!(sha256(&project.join("render/master.wav")), master);
        assert_eq!(
            sha256(&project.join("render/stem-track-lead-post_fader.wav")),
            stem
        );
        assert_eq!(staging_directories(project), Vec::<PathBuf>::new());
    }
}

#[test]
fn a_terminated_render_reports_the_signal_and_leaves_nothing_behind() {
    let root = TempDir::new().expect("root");
    let data_home = root.path().join("xdg-data");
    let project = published_project(root.path(), "long", &long_source(800));
    for (signal, name, exit_code, json_mode) in [
        (Signal::TERM, "SIGTERM", 143, true),
        (Signal::INT, "SIGINT", 130, true),
        (Signal::HUP, "SIGHUP", 129, false),
    ] {
        let mut command = crump(&executable(), &project, &data_home);
        if json_mode {
            command.arg("--json");
        }
        let mut child = command
            .args(["render", "--output", "render"])
            .spawn()
            .expect("spawn render");
        wait_until_rendering(&mut child, &project);
        let process = Pid::from_raw(i32::try_from(child.id()).expect("process ID")).expect("PID");
        kill_process(process, signal).expect("signal render");
        let output = child.wait_with_output().expect("render exit");

        assert_eq!(output.status.code(), Some(exit_code), "{name}: {output:?}");
        let message =
            format!("render interrupted by {name}; nothing was published and staging was removed");
        if json_mode {
            assert!(output.stderr.is_empty(), "{name}: {output:?}");
            let response = json(&output);
            assert_eq!(response["ok"], false);
            assert_eq!(response["command"], "render");
            assert_eq!(response["error"]["code"], "interrupted");
            assert_eq!(response["error"]["message"], message);
        } else {
            assert!(output.stdout.is_empty(), "{name}: {output:?}");
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                format!("crump: interrupted: {message}\n")
            );
        }
        assert_eq!(staging_directories(&project), Vec::<PathBuf>::new());
        assert!(!project.join("render").exists());
    }
}

#[test]
fn a_render_finishes_after_its_executable_is_replaced_on_disk() {
    let root = TempDir::new().expect("root");
    let data_home = root.path().join("xdg-data");
    let project = published_project(root.path(), "outlives-build", &long_source(8));
    let installed = root.path().join("crump-under-rebuild");
    fs::copy(executable(), &installed).expect("install executable");
    let image = sha256(&installed);

    let mut child = crump(&installed, &project, &data_home)
        .args(["--json", "render", "--output", "render"])
        .spawn()
        .expect("spawn render");
    // What a rebuild does while renders are running: unlink the old file, write a new one.
    fs::remove_file(&installed).expect("remove executable");
    fs::write(&installed, b"a different build").expect("replace executable");
    wait_until_rendering(&mut child, &project);
    let output = child.wait_with_output().expect("render exit");

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let response = json(&output);
    assert_eq!(response["ok"], true);
    assert_eq!(
        response["data"]["render"]["manifest"]["executable_sha256"],
        image
    );
    assert!(project.join("render/master.wav").is_file());
}
