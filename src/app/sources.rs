use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui;

use crate::config::RecentSource;
use crate::crop::{CropRect, SourceSize};
use crate::image_io::{ImageQueue, ScanDepth, SourceImage};
use crate::loader::LoadPriority;
use crate::viewport::TiledTexture;

use super::workspace::{ResizeWheelState, ZoomMode};
use super::{CropDeckApp, CropSizePreference, display_file_name};

impl CropDeckApp {
    pub(super) fn open_image_dialog(&mut self) {
        let selection = rfd::FileDialog::new()
            .add_filter("Images", &["jpg", "jpeg", "png", "webp"])
            .pick_file();
        if let Some(path) = selection {
            self.open_source(path);
        }
    }

    pub(super) fn open_folder_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            self.open_source(path);
        }
    }

    pub(super) fn open_source(&mut self, path: PathBuf) {
        let queue = if path.is_dir() {
            let depth = if self.config.recursive_scan() {
                ScanDepth::Recursive
            } else {
                ScanDepth::FolderOnly
            };
            ImageQueue::from_folder(&path, depth)
        } else {
            ImageQueue::from_paths([path.clone()])
        };
        match queue {
            Ok(queue) => self.install_queue(queue, path),
            Err(error) => self.report_error(format!(
                "Could not open {}: {error}",
                display_file_name(&path)
            )),
        }
    }

    pub(super) fn forget_recent_source(&mut self, path: &Path) {
        self.config.remove_recent_source(path);
        self.notify(format!(
            "Removed {} from recent sources",
            display_file_name(path)
        ));
    }

    fn install_queue(&mut self, queue: ImageQueue, origin: PathBuf) {
        let recent = if origin.is_dir() {
            RecentSource::folder(origin, queue.len())
        } else {
            RecentSource::image(origin)
        };
        self.config.push_recent_source(recent);
        self.queue = Some(queue);
        self.request_current();
    }

    fn request_current(&mut self) {
        let Some(path) = self.queue.as_ref().map(|queue| queue.current().to_owned()) else {
            return;
        };
        let Some(loader) = self.loader.as_mut() else {
            self.report_error(String::from("Image loader is unavailable"));
            return;
        };
        match loader.request(path.clone(), LoadPriority::Current) {
            Ok(Some(source)) => {
                self.awaited = None;
                self.install_source(source);
            }
            Ok(None) => {
                self.awaited = Some(path.clone());
                self.notify(format!("Loading {}", display_file_name(&path)));
            }
            Err(error) => {
                self.awaited = None;
                self.report_error(format!("Could not request image: {error}"));
            }
        }
        self.prefetch_neighbours();
    }

    fn install_source(&mut self, source: SourceImage) {
        let path = source.path().to_owned();
        let dimensions = source.dimensions();
        self.texture = None;
        let texture = match TiledTexture::new(source.clone(), &path.to_string_lossy()) {
            Ok(texture) => texture,
            Err(error) => {
                self.report_error(format!("Could not prepare image: {error}"));
                return;
            }
        };
        let source_size = SourceSize::new(dimensions.width(), dimensions.height())
            .expect("decoded images always have non-zero dimensions");
        self.workspace.crop = Some(CropRect::with_aspect_ratio(
            0,
            0,
            source_size.width(),
            self.config.aspect_ratio(),
            source_size,
        ));
        self.workspace.effective_ratio = self.config.aspect_ratio();
        self.workspace.history.clear();
        self.workspace.zoom = ZoomMode::FitWidth;
        self.workspace.drag = None;
        self.workspace.requested_scroll_y = Some(0.0);
        self.workspace.ensure_crop_visible = false;
        self.workspace.resize_wheel = ResizeWheelState::default();
        self.next_export_index = 1;
        self.source = Some(source);
        self.texture = Some(texture);
        if self.size_preference != CropSizePreference::Automatic {
            self.apply_ratio(self.config.aspect_ratio());
        }
        self.notify(format!("Loaded {}", display_file_name(&path)));
    }

    fn prefetch_neighbours(&mut self) {
        let Some(queue) = self.queue.as_ref() else {
            return;
        };
        let index = queue.current_index();
        let mut paths = Vec::with_capacity(2);
        if let Some(path) = queue.paths().get(index + 1) {
            paths.push(path.clone());
        }
        if let Some(path) = index
            .checked_sub(1)
            .and_then(|previous| queue.paths().get(previous))
        {
            paths.push(path.clone());
        }
        let Some(loader) = self.loader.as_mut() else {
            return;
        };
        for path in paths {
            if let Err(error) = loader.request(path, LoadPriority::Prefetch) {
                self.report_error(format!("Could not prefetch image: {error}"));
                break;
            }
        }
    }

    pub(super) fn poll_loader(&mut self, context: &egui::Context) {
        let Some(loader) = self.loader.as_mut() else {
            return;
        };
        let results = loader.poll();
        for result in results {
            if self.awaited.as_deref() != Some(result.path()) {
                continue;
            }
            self.awaited.take();
            match result.into_outcome() {
                Ok(source) => self.install_source(source),
                Err(error) => self.report_error(format!("Could not load image: {error}")),
            }
        }
        let should_retry = self.awaited.as_ref().is_some_and(|path| {
            self.loader
                .as_ref()
                .is_some_and(|loader| !loader.is_loading(path))
        });
        if should_retry {
            self.request_current();
        }
        if self.awaited.is_some() {
            context.request_repaint_after(Duration::from_millis(50));
        }
    }

    pub(super) fn navigate(&mut self, forward: bool) {
        let moved = self.queue.as_mut().is_some_and(|queue| {
            if forward {
                queue.move_next().is_some()
            } else {
                queue.move_previous().is_some()
            }
        });
        if moved {
            self.request_current();
        }
    }

    pub(super) fn can_navigate(&self, forward: bool) -> bool {
        self.queue.as_ref().is_some_and(|queue| {
            if forward {
                queue.current_index() + 1 < queue.len()
            } else {
                queue.current_index() > 0
            }
        })
    }

    pub(super) fn queue_position(&self) -> String {
        self.queue.as_ref().map_or_else(
            || String::from("0 / 0"),
            |queue| format!("{} / {}", queue.current_index() + 1, queue.len()),
        )
    }
}
