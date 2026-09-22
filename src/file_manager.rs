use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum FileManagerError {
    #[error("{0} has no parent folder to reveal")]
    NoParent(PathBuf),

    #[error("no file manager accepted {0}")]
    NoFileManager(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevealCommand {
    program: OsString,
    arguments: Vec<OsString>,
    awaits_exit: bool,
}

impl RevealCommand {
    #[must_use]
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    #[must_use]
    pub fn arguments(&self) -> &[OsString] {
        &self.arguments
    }

    fn run(&self) -> bool {
        let mut command = Command::new(&self.program);
        command
            .args(&self.arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if self.awaits_exit {
            command.status().is_ok_and(|status| status.success())
        } else {
            command.spawn().is_ok()
        }
    }
}

pub fn reveal(path: &Path) -> Result<(), FileManagerError> {
    let commands = reveal_commands(path)?;
    if commands.iter().any(RevealCommand::run) {
        return Ok(());
    }
    Err(FileManagerError::NoFileManager(path.to_path_buf()))
}

fn containing_folder(path: &Path) -> Result<&Path, FileManagerError> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| FileManagerError::NoParent(path.to_path_buf()))
}

#[cfg(unix)]
pub fn reveal_commands(path: &Path) -> Result<Vec<RevealCommand>, FileManagerError> {
    let folder = containing_folder(path)?;
    Ok(vec![
        RevealCommand {
            program: OsString::from("dbus-send"),
            arguments: vec![
                OsString::from("--session"),
                OsString::from("--print-reply"),
                OsString::from("--reply-timeout=2000"),
                OsString::from("--dest=org.freedesktop.FileManager1"),
                OsString::from("--type=method_call"),
                OsString::from("/org/freedesktop/FileManager1"),
                OsString::from("org.freedesktop.FileManager1.ShowItems"),
                OsString::from(format!("array:string:{}", file_uri(path))),
                OsString::from("string:"),
            ],
            awaits_exit: true,
        },
        RevealCommand {
            program: OsString::from("xdg-open"),
            arguments: vec![folder.as_os_str().to_owned()],
            awaits_exit: true,
        },
    ])
}

#[cfg(unix)]
fn file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;

    use percent_encoding::{AsciiSet, CONTROLS, percent_encode};

    const RESERVED: &AsciiSet = &CONTROLS
        .add(b' ')
        .add(b'"')
        .add(b'#')
        .add(b'%')
        .add(b'\'')
        .add(b'<')
        .add(b'>')
        .add(b'?')
        .add(b'[')
        .add(b'\\')
        .add(b']')
        .add(b'^')
        .add(b'`')
        .add(b'{')
        .add(b'|')
        .add(b'}');

    format!(
        "file://{}",
        percent_encode(path.as_os_str().as_bytes(), RESERVED)
    )
}

#[cfg(windows)]
pub fn reveal_commands(path: &Path) -> Result<Vec<RevealCommand>, FileManagerError> {
    containing_folder(path)?;
    let mut selection = OsString::from("/select,");
    selection.push(path.as_os_str());
    Ok(vec![RevealCommand {
        program: OsString::from("explorer.exe"),
        arguments: vec![selection],
        awaits_exit: false,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(command: &RevealCommand) -> Vec<String> {
        command
            .arguments()
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn a_path_without_a_parent_folder_is_refused() {
        let error =
            reveal_commands(Path::new("crop.png")).expect_err("a bare filename cannot be revealed");

        assert!(matches!(error, FileManagerError::NoParent(path) if path == Path::new("crop.png")));
    }

    #[cfg(unix)]
    #[test]
    fn the_first_unix_command_asks_the_file_manager_to_select_the_file() {
        let commands = reveal_commands(Path::new("/comics/exports/page 1.webp"))
            .expect("an absolute path should be revealable");

        assert_eq!(commands[0].program(), OsStr::new("dbus-send"));
        assert!(arguments(&commands[0]).contains(&String::from(
            "array:string:file:///comics/exports/page%201.webp"
        )));
        assert!(
            arguments(&commands[0])
                .contains(&String::from("org.freedesktop.FileManager1.ShowItems"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_unix_fallback_opens_the_containing_folder() {
        let commands = reveal_commands(Path::new("/comics/exports/page1.webp"))
            .expect("an absolute path should be revealable");

        assert_eq!(commands.len(), 2);
        assert_eq!(commands[1].program(), OsStr::new("xdg-open"));
        assert_eq!(
            arguments(&commands[1]),
            vec![String::from("/comics/exports")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_uri_encodes_characters_that_would_break_the_argument() {
        assert_eq!(
            file_uri(Path::new("/comics/a b#c'd.webp")),
            "file:///comics/a%20b%23c%27d.webp"
        );
        assert_eq!(
            file_uri(Path::new("/comics/\u{e4}.webp")),
            "file:///comics/%C3%A4.webp"
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_windows_command_selects_the_file_in_explorer() {
        let commands = reveal_commands(Path::new(r"C:\comics\exports\page 1.webp"))
            .expect("an absolute path should be revealable");

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].program(), OsStr::new("explorer.exe"));
        assert_eq!(
            arguments(&commands[0]),
            vec![String::from(r"/select,C:\comics\exports\page 1.webp")]
        );
    }
}
