//! Interactive Project discovery and selection for workspace startup.

use std::{
    ffi::OsStr,
    fs,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use thiserror::Error;

use crate::project::PROJECT_FILE;

const IGNORED_DIRECTORIES: [&str; 5] = [".crump", ".git", "node_modules", "target", "vendor"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProjectSelection {
    Selected(PathBuf),
    Cancelled,
}

#[derive(Debug, Error)]
pub(crate) enum ProjectPickerError {
    #[error("Could not search `{path}` for Crump Projects: {source}")]
    Search {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(
        "No `{PROJECT_FILE}` was found under `{}`; create one with `crump new PATH` or pass `--project PATH`",
        .0.display()
    )]
    NoProjects(PathBuf),
    #[error("Could not start fzf: {0}")]
    StartFzf(#[source] io::Error),
    #[error("Could not send Project choices to fzf: {0}")]
    WriteFzf(#[source] io::Error),
    #[error("Could not wait for fzf: {0}")]
    WaitFzf(#[source] io::Error),
    #[error("fzf returned an invalid Project selection")]
    InvalidFzfSelection,
    #[error("Could not read the Project selection: {0}")]
    Prompt(#[source] io::Error),
}

#[derive(Debug)]
struct ProjectChoice {
    root: PathBuf,
    label: String,
}

#[derive(Debug, PartialEq, Eq)]
enum FzfSelection {
    Selected(usize),
    Cancelled,
    Unavailable,
}

pub(crate) fn select_project(search_root: &Path) -> Result<ProjectSelection, ProjectPickerError> {
    let roots = discover_project_roots(search_root)?;
    if roots.is_empty() {
        return Err(ProjectPickerError::NoProjects(search_root.to_path_buf()));
    }
    if roots.len() == 1 {
        return Ok(ProjectSelection::Selected(roots[0].clone()));
    }

    let choices = roots
        .into_iter()
        .map(|root| ProjectChoice {
            label: project_label(search_root, &root),
            root,
        })
        .collect::<Vec<_>>();

    match select_with_fzf(&choices)? {
        FzfSelection::Selected(index) => Ok(ProjectSelection::Selected(
            choices
                .get(index)
                .ok_or(ProjectPickerError::InvalidFzfSelection)?
                .root
                .clone(),
        )),
        FzfSelection::Cancelled => Ok(ProjectSelection::Cancelled),
        FzfSelection::Unavailable => select_with_numbered_terminal(&choices),
    }
}

pub(crate) fn discover_project_roots(
    search_root: &Path,
) -> Result<Vec<PathBuf>, ProjectPickerError> {
    let mut pending = vec![search_root.to_path_buf()];
    let mut projects = Vec::new();

    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory).map_err(|source| ProjectPickerError::Search {
            path: directory.clone(),
            source,
        })?;
        let mut entries = entries.collect::<Result<Vec<_>, _>>().map_err(|source| {
            ProjectPickerError::Search {
                path: directory.clone(),
                source,
            }
        })?;
        entries.sort_by_key(fs::DirEntry::file_name);

        for entry in entries {
            let file_type = entry
                .file_type()
                .map_err(|source| ProjectPickerError::Search {
                    path: entry.path(),
                    source,
                })?;
            if file_type.is_file() && entry.file_name() == OsStr::new(PROJECT_FILE) {
                projects.push(directory.clone());
            } else if file_type.is_dir() && !ignored_directory(&entry.file_name()) {
                pending.push(entry.path());
            }
        }
    }

    projects.sort();
    projects.dedup();
    Ok(projects)
}

fn ignored_directory(name: &OsStr) -> bool {
    IGNORED_DIRECTORIES
        .iter()
        .any(|ignored| name == OsStr::new(ignored))
}

fn project_label(search_root: &Path, project_root: &Path) -> String {
    let relative = project_root
        .strip_prefix(search_root)
        .unwrap_or(project_root);
    if relative.as_os_str().is_empty() {
        ".".to_owned()
    } else {
        format!("./{}", relative.display())
    }
}

fn select_with_fzf(choices: &[ProjectChoice]) -> Result<FzfSelection, ProjectPickerError> {
    let mut child = match Command::new("fzf")
        .args([
            "--read0",
            "--print0",
            "--height=60%",
            "--layout=reverse",
            "--border",
            "--no-multi",
            "--delimiter=\t",
            "--with-nth=2..",
            "--prompt=Crump Project> ",
            "--header=Select a project.crump",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(FzfSelection::Unavailable);
        }
        Err(error) => return Err(ProjectPickerError::StartFzf(error)),
    };

    let write_result = child.stdin.take().map_or_else(
        || {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "fzf did not expose its input",
            ))
        },
        |mut input| {
            for (index, choice) in choices.iter().enumerate() {
                write!(input, "{}\t{}\0", index + 1, choice.label)?;
            }
            Ok(())
        },
    );
    let output = child
        .wait_with_output()
        .map_err(ProjectPickerError::WaitFzf)?;

    if matches!(output.status.code(), Some(1 | 130)) {
        return Ok(FzfSelection::Cancelled);
    }
    if !output.status.success() {
        return Ok(FzfSelection::Unavailable);
    }
    write_result.map_err(ProjectPickerError::WriteFzf)?;

    let index = parse_fzf_index(&output.stdout).ok_or(ProjectPickerError::InvalidFzfSelection)?;
    Ok(FzfSelection::Selected(index))
}

fn parse_fzf_index(output: &[u8]) -> Option<usize> {
    output.split(|byte| *byte == 0).find_map(|record| {
        let separator = record.iter().position(|byte| *byte == b'\t')?;
        std::str::from_utf8(&record[..separator])
            .ok()?
            .parse::<usize>()
            .ok()?
            .checked_sub(1)
    })
}

fn select_with_numbered_terminal(
    choices: &[ProjectChoice],
) -> Result<ProjectSelection, ProjectPickerError> {
    let stdin = io::stdin();
    let stderr = io::stderr();
    select_with_numbered_prompt(choices, &mut stdin.lock(), &mut stderr.lock())
}

fn select_with_numbered_prompt(
    choices: &[ProjectChoice],
    input: &mut dyn BufRead,
    output: &mut dyn Write,
) -> Result<ProjectSelection, ProjectPickerError> {
    writeln!(output, "Select a Crump Project:").map_err(ProjectPickerError::Prompt)?;
    for (index, choice) in choices.iter().enumerate() {
        writeln!(output, "  {}. {}", index + 1, choice.label)
            .map_err(ProjectPickerError::Prompt)?;
    }

    loop {
        write!(output, "Project [1-{}] (Enter to cancel): ", choices.len())
            .and_then(|()| output.flush())
            .map_err(ProjectPickerError::Prompt)?;
        let mut selection = String::new();
        if input
            .read_line(&mut selection)
            .map_err(ProjectPickerError::Prompt)?
            == 0
        {
            return Ok(ProjectSelection::Cancelled);
        }
        let selection = selection.trim();
        if selection.is_empty() || matches!(selection, "q" | "quit") {
            return Ok(ProjectSelection::Cancelled);
        }
        if let Ok(index) = selection.parse::<usize>()
            && let Some(choice) = index.checked_sub(1).and_then(|index| choices.get(index))
        {
            return Ok(ProjectSelection::Selected(choice.root.clone()));
        }
        writeln!(output, "Enter a number from 1 to {}.", choices.len())
            .map_err(ProjectPickerError::Prompt)?;
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn discovers_nested_projects_without_build_or_history_copies() {
        let directory = tempdir().expect("temporary search directory");
        for relative in ["alpha", "nested/beta", "target/generated", ".crump/history"] {
            let root = directory.path().join(relative);
            fs::create_dir_all(&root).expect("create candidate directory");
            fs::write(root.join(PROJECT_FILE), "crump 1\n").expect("write candidate Project");
        }

        let roots = discover_project_roots(directory.path()).expect("discover Projects");
        assert_eq!(
            roots,
            [
                directory.path().join("alpha"),
                directory.path().join("nested/beta")
            ]
        );
    }

    #[test]
    fn selects_a_single_nested_project_without_an_external_picker() {
        let directory = tempdir().expect("temporary search directory");
        let project = directory.path().join("examples/song");
        fs::create_dir_all(&project).expect("create Project directory");
        fs::write(project.join(PROJECT_FILE), "crump 1\n").expect("write Project");

        assert_eq!(
            select_project(directory.path()).expect("select Project"),
            ProjectSelection::Selected(project)
        );
    }

    #[test]
    fn parses_the_index_prefix_from_fzf_output() {
        assert_eq!(parse_fzf_index(b"2\t./nested/beta\0"), Some(1));
        assert_eq!(
            parse_fzf_index(b"query\0enter\02\t./nested/beta\0"),
            Some(1)
        );
        assert_eq!(parse_fzf_index(b"./nested/beta\0"), None);
    }

    #[test]
    fn numbered_fallback_reprompts_and_selects() {
        let choices = vec![
            ProjectChoice {
                root: PathBuf::from("alpha"),
                label: "./alpha".to_owned(),
            },
            ProjectChoice {
                root: PathBuf::from("beta"),
                label: "./beta".to_owned(),
            },
        ];
        let mut input = io::Cursor::new(b"nope\n2\n");
        let mut output = Vec::new();

        assert_eq!(
            select_with_numbered_prompt(&choices, &mut input, &mut output)
                .expect("numbered selection"),
            ProjectSelection::Selected(PathBuf::from("beta"))
        );
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("Enter a number")
        );
    }
}
