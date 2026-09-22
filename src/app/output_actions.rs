use std::path::{Path, PathBuf};

use crate::clipboard::{ClipboardReceipt, ClipboardRequest};
use crate::export::OutputSize;
use crate::file_manager::FileManagerError;

use super::{CropDeckApp, display_file_name};

pub(super) fn copy_notice(receipt: ClipboardReceipt) -> String {
    format!(
        "Copied {}x{} crop to the clipboard",
        receipt.width(),
        receipt.height()
    )
}

pub(super) fn reveal_notice(path: &Path) -> String {
    format!("Revealed {}", display_file_name(path))
}

impl CropDeckApp {
    pub(super) fn can_copy_crop(&self) -> bool {
        self.workspace.crop.is_some() && self.source.is_some() && self.clipboard.is_some()
    }

    pub(super) fn can_reveal_export(&self) -> bool {
        self.last_export.is_some() && self.filesystem.is_some()
    }

    pub(super) fn copy_crop(&mut self) {
        let (Some(crop), Some(source)) = (self.workspace.crop, self.source.clone()) else {
            return;
        };
        let output_size = match self
            .config
            .export()
            .output_size()
            .map(|(width, height)| OutputSize::new(width, height))
            .transpose()
        {
            Ok(size) => size,
            Err(error) => {
                self.report_error(format!("Invalid output dimensions: {error}"));
                return;
            }
        };
        let Some(clipboard) = self.clipboard.as_ref() else {
            self.report_error(String::from("Clipboard worker is unavailable"));
            return;
        };
        match clipboard.copy(ClipboardRequest::new(source, crop, output_size)) {
            Ok(()) => self.notify("Copying the crop to the clipboard"),
            Err(error) => self.report_error(format!("Could not copy the crop: {error}")),
        }
    }

    pub(super) fn poll_clipboard(&mut self) {
        let Some(clipboard) = self.clipboard.as_ref() else {
            return;
        };
        let mut outcomes = Vec::new();
        let mut worker_error = None;
        loop {
            match clipboard.poll_result() {
                Ok(Some(outcome)) => outcomes.push(outcome),
                Ok(None) => break,
                Err(error) => {
                    worker_error = Some(format!("Clipboard worker error: {error}"));
                    break;
                }
            }
        }
        for outcome in outcomes {
            match outcome {
                Ok(receipt) => self.notify(copy_notice(receipt)),
                Err(error) => self.report_error(format!("Could not copy the crop: {error}")),
            }
        }
        if let Some(error) = worker_error {
            self.report_error(error);
        }
    }

    pub(super) fn reveal_last_export(&mut self) {
        let Some(path) = self.last_export.clone() else {
            return;
        };
        let Some(filesystem) = self.filesystem.as_ref() else {
            self.report_error(String::from("Filesystem worker is unavailable"));
            return;
        };
        if let Err(error) = filesystem.reveal(path) {
            self.report_error(format!("Could not reveal the export: {error}"));
        }
    }

    pub(super) fn apply_reveal_result(
        &mut self,
        path: PathBuf,
        outcome: Result<(), FileManagerError>,
    ) {
        match outcome {
            Ok(()) => self.notify(reveal_notice(&path)),
            Err(error) => self.report_error(format!("Could not reveal the export: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_notice_reports_the_clipboard_dimensions() {
        assert_eq!(
            copy_notice(ClipboardReceipt::new(3, 5)),
            "Copied 3x5 crop to the clipboard"
        );
    }

    #[test]
    fn a_reveal_notice_names_only_the_exported_file() {
        assert_eq!(
            reveal_notice(Path::new("/comics/exports/page1_001.webp")),
            "Revealed page1_001.webp"
        );
    }
}
