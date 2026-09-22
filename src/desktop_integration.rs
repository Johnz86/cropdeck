use std::borrow::Cow;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::BaseDirs;
use thiserror::Error;

pub const APP_ID: &str = "cropdeck";

pub const WINDOW_ICON_PNG: &[u8] =
    include_bytes!("../assets/linux/icons/hicolor/256x256/apps/cropdeck.png");

const DESKTOP_ENTRY_TEMPLATE: &str = include_str!("../assets/linux/cropdeck.desktop");
const APPIMAGE_ENV: &str = "APPIMAGE";

struct IconAsset {
    relative_path: &'static str,
    bytes: &'static [u8],
}

macro_rules! icon_asset {
    ($path:literal) => {
        IconAsset {
            relative_path: $path,
            bytes: include_bytes!(concat!("../assets/linux/icons/hicolor/", $path)),
        }
    };
}

const ICONS: [IconAsset; 8] = [
    icon_asset!("16x16/apps/cropdeck.png"),
    icon_asset!("32x32/apps/cropdeck.png"),
    icon_asset!("48x48/apps/cropdeck.png"),
    icon_asset!("64x64/apps/cropdeck.png"),
    icon_asset!("128x128/apps/cropdeck.png"),
    icon_asset!("256x256/apps/cropdeck.png"),
    icon_asset!("512x512/apps/cropdeck.png"),
    icon_asset!("scalable/apps/cropdeck.svg"),
];

#[derive(Debug, Error)]
pub enum DesktopIntegrationError {
    #[error("user data directory is unavailable")]
    DataDirectoryUnavailable,
    #[error("could not write {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub fn install_for_appimage() -> Result<bool, DesktopIntegrationError> {
    let Some(executable) = env::var_os(APPIMAGE_ENV).map(PathBuf::from) else {
        return Ok(false);
    };
    let base_dirs = BaseDirs::new().ok_or(DesktopIntegrationError::DataDirectoryUnavailable)?;
    install(base_dirs.data_local_dir(), &executable)
}

pub fn install(data_dir: &Path, executable: &Path) -> Result<bool, DesktopIntegrationError> {
    let icon_root = data_dir.join("icons").join("hicolor");
    let mut changed = false;
    for icon in &ICONS {
        changed |= write_if_changed(&icon_root.join(icon.relative_path), icon.bytes)?;
    }
    let entry_path = data_dir
        .join("applications")
        .join(format!("{APP_ID}.desktop"));
    changed |= write_if_changed(&entry_path, desktop_entry(executable).as_bytes())?;
    Ok(changed)
}

fn desktop_entry(executable: &Path) -> String {
    let exec_line = format!("Exec=\"{}\"", executable.display());
    let mut entry: String = DESKTOP_ENTRY_TEMPLATE
        .lines()
        .map(|line| {
            if line.starts_with("Exec=") {
                Cow::Owned(exec_line.clone())
            } else {
                Cow::Borrowed(line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    entry.push('\n');
    entry
}

fn write_if_changed(path: &Path, contents: &[u8]) -> Result<bool, DesktopIntegrationError> {
    if fs::read(path).is_ok_and(|existing| existing == contents) {
        return Ok(false);
    }
    let write_error = |source| DesktopIntegrationError::Write {
        path: path.to_owned(),
        source,
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(write_error)?;
    }
    fs::write(path, contents).map_err(write_error)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_writes_entry_and_icons_once() {
        let data_dir = tempfile::tempdir().expect("temporary directory");
        let executable = Path::new("/opt/apps/CropDeck.AppImage");

        let first = install(data_dir.path(), executable).expect("first install");
        let second = install(data_dir.path(), executable).expect("second install");

        assert!(first);
        assert!(!second);
        let entry = fs::read_to_string(data_dir.path().join("applications/cropdeck.desktop"))
            .expect("desktop entry");
        assert!(entry.contains("Exec=\"/opt/apps/CropDeck.AppImage\"\n"));
        assert!(entry.contains("Icon=cropdeck\n"));
        for icon in &ICONS {
            let path = data_dir
                .path()
                .join("icons/hicolor")
                .join(icon.relative_path);
            assert_eq!(fs::read(&path).expect("icon file"), icon.bytes);
        }
    }

    #[test]
    fn install_rewrites_entry_when_executable_moves() {
        let data_dir = tempfile::tempdir().expect("temporary directory");
        install(data_dir.path(), Path::new("/old/CropDeck.AppImage")).expect("first install");

        let changed = install(data_dir.path(), Path::new("/new dir/CropDeck.AppImage"))
            .expect("second install");

        assert!(changed);
        let entry = fs::read_to_string(data_dir.path().join("applications/cropdeck.desktop"))
            .expect("desktop entry");
        assert!(entry.contains("Exec=\"/new dir/CropDeck.AppImage\"\n"));
        assert!(!entry.contains("/old/"));
    }

    #[test]
    fn desktop_entry_keeps_template_fields() {
        let entry = desktop_entry(Path::new("/x/CropDeck.AppImage"));

        assert!(entry.starts_with("[Desktop Entry]\n"));
        assert!(entry.contains("Name=CropDeck\n"));
        assert!(entry.contains("StartupWMClass=cropdeck\n"));
        assert_eq!(entry.matches("Exec=").count(), 1);
    }
}
