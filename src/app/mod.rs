mod capture;
mod crop_commands;
mod dialogs;
mod format_bar;
mod interaction;
mod panels;
mod shortcuts;
mod sources;
mod workspace;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::config::AppConfig;
use crate::desktop_integration;
use crate::export::ExportQueue;
use crate::image_io::{ImageQueue, SourceImage};
use crate::loader::ImageLoader;
use crate::presets::SizeTier;
use crate::viewport::TiledTexture;

use self::shortcuts::InputFocus;
use self::workspace::WorkspaceState;

const MEBIBYTE: usize = 1_024 * 1_024;
const STATUS_LIFETIME: Duration = Duration::from_secs(4);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum CropSizePreference {
    #[default]
    Automatic,
    Tier(SizeTier),
    Maximum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusLevel {
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StatusMessage {
    text: String,
    level: StatusLevel,
    shown_at: Instant,
}

impl StatusMessage {
    fn new(text: impl Into<String>, level: StatusLevel) -> Self {
        Self {
            text: text.into(),
            level,
            shown_at: Instant::now(),
        }
    }

    fn remaining(&self, now: Instant) -> Option<Duration> {
        match self.level {
            StatusLevel::Info => Some(STATUS_LIFETIME.saturating_sub(now - self.shown_at)),
            StatusLevel::Error => None,
        }
    }
}

pub struct CropDeckApp {
    config: AppConfig,
    queue: Option<ImageQueue>,
    source: Option<SourceImage>,
    texture: Option<TiledTexture>,
    workspace: WorkspaceState,
    settings_open: bool,
    about_open: bool,
    custom_ratio_width: u32,
    custom_ratio_height: u32,
    size_preference: CropSizePreference,
    output_width: u32,
    output_height: u32,
    preserve_output_size: bool,
    filename_template: String,
    filename_template_error: Option<String>,
    export_queue: Option<ExportQueue>,
    loader: Option<ImageLoader>,
    awaited: Option<PathBuf>,
    pending_exports: usize,
    next_export_index: u64,
    status: Option<StatusMessage>,
}

impl CropDeckApp {
    #[must_use]
    pub fn new(creation_context: &eframe::CreationContext<'_>) -> Self {
        let mut startup_errors = Vec::new();
        let config = AppConfig::load_or_default().unwrap_or_else(|error| {
            startup_errors.push(format!("Settings could not be loaded: {error}"));
            AppConfig::default()
        });
        let mut startup_notice = None;
        match desktop_integration::install_for_appimage() {
            Ok(true) => startup_notice = Some("CropDeck was added to the application menu"),
            Ok(false) => {}
            Err(error) => startup_errors.push(format!("Desktop integration failed: {error}")),
        }
        let ratio = config.aspect_ratio();
        let output_size = config.export().output_size();
        let export_queue = ExportQueue::new(8)
            .map_err(|error| startup_errors.push(format!("Export worker could not start: {error}")))
            .ok();
        let cache_budget_bytes = config.cache_budget_megabytes() as usize * MEBIBYTE;
        let loader = ImageLoader::new(2, creation_context.egui_ctx.clone(), cache_budget_bytes)
            .map_err(|error| startup_errors.push(format!("Image loader could not start: {error}")))
            .ok();
        let mut app = Self {
            custom_ratio_width: ratio.width(),
            custom_ratio_height: ratio.height(),
            size_preference: CropSizePreference::Automatic,
            output_width: output_size.map_or(1_024, |(width, _height)| width),
            output_height: output_size.map_or(1_024, |(_width, height)| height),
            preserve_output_size: output_size.is_none(),
            filename_template: config.export().filename_template().to_owned(),
            filename_template_error: None,
            export_queue,
            loader,
            awaited: None,
            pending_exports: 0,
            next_export_index: 1,
            config,
            queue: None,
            source: None,
            texture: None,
            workspace: WorkspaceState::default(),
            settings_open: false,
            about_open: false,
            status: None,
        };
        if let Some(notice) = startup_notice {
            app.notify(notice);
        }
        for error in startup_errors {
            app.report_error(error);
        }
        app
    }

    fn notify(&mut self, text: impl Into<String>) {
        self.status = Some(StatusMessage::new(text, StatusLevel::Info));
    }

    fn report_error(&mut self, text: impl Into<String>) {
        self.status = Some(StatusMessage::new(text, StatusLevel::Error));
    }

    fn input_focus(&self, context: &egui::Context) -> InputFocus {
        InputFocus::current(context, self.settings_open || self.about_open)
    }
}

impl eframe::App for CropDeckApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        self.poll_loader(&context);
        self.poll_exports(&context);
        self.toolbar(ui);
        self.status_bar(ui);
        self.workspace(ui);
        if self.settings_open {
            self.settings_dialog(&context);
        }
        if self.about_open {
            self.about_dialog(&context);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(loader) = self.loader.take()
            && let Err(error) = loader.shutdown()
        {
            self.report_error(format!("Image loader could not stop cleanly: {error}"));
        }
        if let Some(export_queue) = self.export_queue.take()
            && let Err(error) = export_queue.shutdown()
        {
            self.report_error(format!("Export worker could not stop cleanly: {error}"));
        }
        if let Err(error) = self.config.save() {
            self.report_error(format!("Settings could not be saved: {error}"));
        }
    }
}

fn display_file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_name_display_omits_parent_directories() {
        assert_eq!(
            display_file_name(Path::new("chapters/page10.webp")),
            "page10.webp"
        );
    }

    #[test]
    fn informational_status_expires_and_errors_persist() {
        let info = StatusMessage::new("Loaded page.webp", StatusLevel::Info);
        let error = StatusMessage::new("Export failed", StatusLevel::Error);
        let later = info.shown_at + STATUS_LIFETIME + Duration::from_millis(1);

        assert_eq!(info.remaining(info.shown_at), Some(STATUS_LIFETIME));
        assert_eq!(info.remaining(later), Some(Duration::ZERO));
        assert_eq!(error.remaining(later), None);
    }
}
