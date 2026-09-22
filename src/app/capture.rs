use std::fs;
use std::path::{Path, PathBuf};

use crate::export::{ExportError, ExportOptions, ExportRequest};
use crate::naming::{FilenameContext, FilenameTemplate, OutputDimensions};

use super::workspace::HistoryEntry;
use super::{CropDeckApp, display_file_name};

#[derive(Debug)]
pub(super) struct CapturePlan {
    revision: u64,
    source: PathBuf,
    options: ExportOptions,
    template: FilenameTemplate,
    destination: PathBuf,
}

fn plan_is_current(plan: &CapturePlan, revision: u64, source: &Path) -> bool {
    plan.revision == revision && plan.source == source
}

impl CropDeckApp {
    fn build_capture_plan(&self, source: &Path) -> Result<CapturePlan, String> {
        let options = ExportOptions::from_settings(self.config.export())
            .map_err(|error| format!("Invalid export settings: {error}"))?;
        let template = FilenameTemplate::parse(self.config.export().filename_template())
            .map_err(|error| format!("Invalid filename template: {error}"))?;
        let destination = self
            .config
            .export()
            .destination()
            .map(Path::to_owned)
            .or_else(|| source.parent().map(Path::to_owned))
            .unwrap_or_else(|| PathBuf::from("."));
        fs::create_dir_all(&destination).map_err(|error| {
            format!(
                "Could not create destination {}: {error}",
                destination.display()
            )
        })?;

        Ok(CapturePlan {
            revision: self.config.revision(),
            source: source.to_path_buf(),
            options,
            template,
            destination,
        })
    }

    pub(super) fn capture_and_advance(&mut self) {
        let (Some(crop), Some(source)) = (self.workspace.crop, self.source_size()) else {
            return;
        };
        let Some(source_path) = self.source.as_ref().map(|image| image.path().to_path_buf()) else {
            return;
        };
        if self.export_queue.is_none() {
            self.report_error(String::from("Export worker is unavailable"));
            return;
        }
        let revision = self.config.revision();
        let reusable = self
            .capture_plan
            .take()
            .filter(|plan| plan_is_current(plan, revision, &source_path));
        let plan = match reusable {
            Some(plan) => plan,
            None => match self.build_capture_plan(&source_path) {
                Ok(plan) => plan,
                Err(message) => {
                    self.report_error(message);
                    return;
                }
            },
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
        let filename_context = match FilenameContext::from_source_path(
            &source_path,
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
        let resolved = match plan.template.resolve_available(
            &plan.destination,
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
        let Some(source_image) = self.source.as_ref() else {
            return;
        };
        let request = ExportRequest::new(
            source_image.clone(),
            crop,
            resolved.into_path(),
            plan.options,
        );
        let submitted = self
            .export_queue
            .as_ref()
            .map(|queue| queue.submit(request));
        match submitted {
            Some(Ok(_job_id)) => {}
            Some(Err(error)) => {
                self.report_error(format!("Could not queue crop: {error}"));
                return;
            }
            None => return,
        }
        self.capture_plan = Some(plan);
        self.next_export_index = index.saturating_add(1);
        self.pending_exports += 1;
        self.workspace.history.push(HistoryEntry { crop, index });
        let advancement =
            i64::from(crop.height()) * i64::from(self.config.capture_advance_percent()) / 100;
        self.workspace.crop = Some(crop.moved_by(0, advancement, source));
        self.workspace.ensure_crop_visible = true;
        self.notify(format!("Crop {index:03} queued for export"));
    }

    pub(super) fn poll_exports(&mut self) {
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
                Ok(receipt) => {
                    self.notify(format!(
                        "Exported {} ({}x{})",
                        display_file_name(receipt.destination()),
                        receipt.width(),
                        receipt.height()
                    ));
                    self.last_export = Some(receipt.destination().to_path_buf());
                }
                Err(error) => {
                    if matches!(
                        error,
                        ExportError::CreateDestination { .. }
                            | ExportError::WriteDestination { .. }
                    ) {
                        self.capture_plan = None;
                    }
                    self.report_error(format!("Export failed: {error}"));
                }
            }
        }
        if let Some(error) = worker_error {
            self.report_error(error);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::ExportFormat;
    use crate::export::{ExportQuality, OutputSize};

    use super::*;

    fn plan(revision: u64, source: &str) -> CapturePlan {
        CapturePlan {
            revision,
            source: PathBuf::from(source),
            options: ExportOptions::new(
                ExportFormat::WebP,
                ExportQuality::new(85).expect("fixture quality should be valid"),
                OutputSize::new(512, 512).ok(),
            ),
            template: FilenameTemplate::parse("{source}_{index:03}")
                .expect("fixture template should parse"),
            destination: PathBuf::from("/exports"),
        }
    }

    #[test]
    fn a_plan_is_reused_while_the_revision_and_source_match() {
        let plan = plan(7, "/comics/page1.webp");

        assert!(plan_is_current(&plan, 7, Path::new("/comics/page1.webp")));
    }

    #[test]
    fn a_plan_is_discarded_when_the_revision_advances() {
        let plan = plan(7, "/comics/page1.webp");

        assert!(!plan_is_current(&plan, 8, Path::new("/comics/page1.webp")));
    }

    #[test]
    fn a_plan_is_discarded_when_the_source_changes() {
        let plan = plan(7, "/comics/page1.webp");

        assert!(!plan_is_current(&plan, 7, Path::new("/comics/page2.webp")));
    }
}
