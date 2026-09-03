use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui;

use crate::export::{ExportOptions, ExportRequest};
use crate::naming::{FilenameContext, FilenameTemplate, OutputDimensions};

use super::workspace::HistoryEntry;
use super::{CropDeckApp, display_file_name};

impl CropDeckApp {
    pub(super) fn capture_and_advance(&mut self) {
        let (Some(crop), Some(source)) = (self.workspace.crop, self.source_size()) else {
            return;
        };
        let Some(source_image) = self.source.as_ref() else {
            return;
        };
        let Some(export_queue) = self.export_queue.as_ref() else {
            self.report_error(String::from("Export worker is unavailable"));
            return;
        };
        let options = match ExportOptions::from_settings(self.config.export()) {
            Ok(options) => options,
            Err(error) => {
                self.report_error(format!("Invalid export settings: {error}"));
                return;
            }
        };
        let (output_width, output_height) = self
            .config
            .export()
            .output_size()
            .unwrap_or((crop.width(), crop.height()));
        let dimensions = match OutputDimensions::new(output_width, output_height) {
            Ok(dimensions) => dimensions,
            Err(error) => {
                self.report_error(format!("Invalid output dimensions: {error}"));
                return;
            }
        };
        let template = match FilenameTemplate::parse(self.config.export().filename_template()) {
            Ok(template) => template,
            Err(error) => {
                self.report_error(format!("Invalid filename template: {error}"));
                return;
            }
        };
        let filename_context = match FilenameContext::from_source_path(
            source_image.path(),
            self.next_export_index,
            self.config.aspect_ratio(),
            dimensions,
        ) {
            Ok(context) => context,
            Err(error) => {
                self.report_error(format!("Could not build export filename: {error}"));
                return;
            }
        };
        let destination_directory = self
            .config
            .export()
            .destination()
            .map(Path::to_owned)
            .or_else(|| source_image.path().parent().map(Path::to_owned))
            .unwrap_or_else(|| PathBuf::from("."));
        if let Err(error) = fs::create_dir_all(&destination_directory) {
            self.report_error(format!(
                "Could not create destination {}: {error}",
                destination_directory.display()
            ));
            return;
        }
        let resolved = match template.resolve_available(
            &destination_directory,
            self.config.export().format().extension(),
            &filename_context,
        ) {
            Ok(resolved) => resolved,
            Err(error) => {
                self.report_error(format!("Could not resolve output name: {error}"));
                return;
            }
        };
        let index = resolved.index();
        let request = ExportRequest::new(source_image.clone(), crop, resolved.into_path(), options);
        if let Err(error) = export_queue.try_submit(request) {
            self.report_error(format!("Could not queue crop: {error}"));
            return;
        }
        self.next_export_index = index.saturating_add(1);
        self.pending_exports += 1;
        self.workspace.history.push(HistoryEntry { crop, index });
        let advancement =
            i64::from(crop.height()) * i64::from(self.config.capture_advance_percent()) / 100;
        self.workspace.crop = Some(crop.moved_by(0, advancement, source));
        self.workspace.ensure_crop_visible = true;
        self.notify(format!("Crop {index:03} queued for export"));
    }

    pub(super) fn poll_exports(&mut self, context: &egui::Context) {
        let Some(queue) = self.export_queue.as_ref() else {
            return;
        };
        let mut outcomes = Vec::new();
        let mut worker_error = None;
        loop {
            match queue.poll_result() {
                Ok(Some(result)) => outcomes.push(result.into_outcome()),
                Ok(None) => break,
                Err(error) => {
                    worker_error = Some(format!("Export worker error: {error}"));
                    break;
                }
            }
        }
        for outcome in outcomes {
            self.pending_exports = self.pending_exports.saturating_sub(1);
            match outcome {
                Ok(receipt) => self.notify(format!(
                    "Exported {} ({}x{})",
                    display_file_name(receipt.destination()),
                    receipt.width(),
                    receipt.height()
                )),
                Err(error) => self.report_error(format!("Export failed: {error}")),
            }
        }
        if let Some(error) = worker_error {
            self.report_error(error);
        }
        if self.pending_exports > 0 {
            context.request_repaint_after(Duration::from_millis(50));
        }
    }
}
