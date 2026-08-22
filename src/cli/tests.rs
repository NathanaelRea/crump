use super::*;

#[test]
fn ctrl_c_stops_interactive_playback() {
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('c'),
        crossterm::event::KeyModifiers::CONTROL,
    );
    assert!(play_stop_key(key));
}

#[test]
fn project_slug_is_source_safe() {
    assert_eq!(slug_id("My First Track!"), "my-first-track");
    assert_eq!(slug_id("123"), "project-123");
}

#[test]
fn blank_project_parses_and_validates() {
    let document = crate::dsl::parse_project(
        Path::new("project.crump"),
        &blank_project_source("Test", "test"),
    );
    let project = document.project.unwrap();
    assert!(!crate::validate::validate_project(&project).has_errors());
}

#[test]
fn explicit_project_options_are_distinguished_from_the_default() {
    assert!(!project_option_is_present(&[OsString::from("crump")]));
    assert!(project_option_is_present(&[
        OsString::from("crump"),
        OsString::from("--project=fixtures/reference"),
    ]));
    assert!(project_option_is_present(&[
        OsString::from("crump"),
        OsString::from("workspace"),
        OsString::from("--project"),
        OsString::from("fixtures/reference"),
    ]));
}

#[test]
fn implicit_workspace_startup_only_picks_when_the_root_source_is_absent() {
    let directory = tempfile::tempdir().expect("temporary workspace root");
    let mut cli = Cli::try_parse_from(["crump"]).expect("parse CLI");
    cli.project = directory.path().to_path_buf();
    assert!(workspace_needs_project_picker(&cli));

    fs::write(
        directory.path().join(crate::project::PROJECT_FILE),
        "crump 1\n",
    )
    .expect("write root Project");
    assert!(!workspace_needs_project_picker(&cli));

    fs::remove_file(directory.path().join(crate::project::PROJECT_FILE))
        .expect("remove root Project");
    cli.project_explicit = true;
    assert!(!workspace_needs_project_picker(&cli));
}
