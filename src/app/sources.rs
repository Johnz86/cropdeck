use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::config::RecentSource;
use crate::crop::{CropRect, SourceSize};
use crate::filesystem::{DialogKind, FilesystemEvent, PathFacts, is_missing_source};
use crate::image_io::{ImageIoError, ImageQueue, ScanDepth, SourceImage, sort_paths_naturally};
use crate::loader::LoadPriority;
use crate::viewport::TiledTexture;

use super::path_field::{PathCommit, PathRole};
use super::workspace::{ResizeWheelState, ZoomMode};
use super::{ActiveScan, CropDeckApp, CropSizePreference, SCAN_MERGE_MINIMUM, display_file_name};

impl CropDeckApp {
    pub(super) fn open_image_dialog(&mut self) {
        self.open_dialog(DialogKind::SourceImage);
    }

    pub(super) fn open_folder_dialog(&mut self) {
        self.open_dialog(DialogKind::SourceFolder);
    }

    pub(super) fn open_dialog(&mut self, kind: DialogKind) {
        if self.dialog_in_flight.is_some() {
            return;
        }
        let Some(filesystem) = self.filesystem.as_ref() else {
            self.report_error(String::from("Filesystem worker is unavailable"));
            return;
        };
        match filesystem.open_dialog(kind) {
            Ok(()) => self.dialog_in_flight = Some(kind),
            Err(error) => self.report_error(format!("Could not open the file dialog: {error}")),
        }
    }

    pub(super) fn open_source(&mut self, path: PathBuf) {
        let depth = if self.config.recursive_scan() {
            ScanDepth::Recursive
        } else {
            ScanDepth::FolderOnly
        };
        let Some(filesystem) = self.filesystem.as_ref() else {
            self.report_error(String::from("Filesystem worker is unavailable"));
            return;
        };
        match filesystem.scan(path.clone(), depth) {
            Ok(generation) => {
                self.queue = None;
                self.source = None;
                self.texture = None;
                self.awaited = None;
                self.capture_plan = None;
                self.pending_paths.clear();
                self.source_field.set_committed(Some(&path));
                self.notify(format!("Scanning {}", display_file_name(&path)));
                self.scan = Some(ActiveScan {
                    root: path,
                    generation,
                    is_folder: false,
                });
            }
            Err(error) => self.report_error(format!(
                "Could not open {}: {error}",
                display_file_name(&path)
            )),
        }
    }

    pub(super) fn forget_recent_source(&mut self, path: &Path) {
        self.config.remove_recent_source(path);
        self.recent_existence.forget(path);
        self.notify(format!(
            "Removed {} from recent sources",
            display_file_name(path)
        ));
    }

    pub(super) fn poll_filesystem(&mut self) {
        let Some(filesystem) = self.filesystem.as_mut() else {
            return;
        };
        for event in filesystem.poll() {
            match event {
                FilesystemEvent::ScanBatch {
                    generation,
                    is_folder,
                    paths,
                } => self.apply_scan_batch(generation, is_folder, paths),
                FilesystemEvent::ScanFinished { generation, total } => {
                    self.finish_scan(generation, total);
                }
                FilesystemEvent::ScanFailed { generation, error } => {
                    self.fail_scan(generation, &error);
                }
                FilesystemEvent::Probed { path, facts } => self.apply_probe(&path, facts),
                FilesystemEvent::DialogClosed { kind, path } => {
                    self.apply_dialog_result(kind, path);
                }
            }
        }
        self.refresh_recent_existence();
        self.submit_field_probes();
        self.apply_field_commits();
    }

    fn apply_field_commits(&mut self) {
        if let Some(PathCommit::Use(path)) = self.source_field.take_commit() {
            self.open_source(path);
        }
        match self.destination_field.take_commit() {
            Some(PathCommit::Clear) => {
                self.config.export_mut().set_destination(None);
                self.capture_plan = None;
            }
            Some(PathCommit::Use(path)) => {
                self.config.export_mut().set_destination(Some(path));
                self.capture_plan = None;
            }
            None => {}
        }
    }

    fn scan_matches(&self, generation: u64) -> bool {
        self.scan
            .as_ref()
            .is_some_and(|scan| scan.generation == generation)
    }

    fn apply_scan_batch(&mut self, generation: u64, is_folder: bool, paths: Vec<PathBuf>) {
        if !self.scan_matches(generation) {
            return;
        }
        if let Some(scan) = self.scan.as_mut() {
            scan.is_folder = is_folder;
        }
        if self.queue.is_some() {
            self.pending_paths.extend(paths);
            let threshold = self
                .queue
                .as_ref()
                .map_or(SCAN_MERGE_MINIMUM, |queue| queue.len() / 8)
                .max(SCAN_MERGE_MINIMUM);
            if self.pending_paths.len() >= threshold {
                self.flush_pending_paths();
            }
            return;
        }
        let Some(root) = self.scan.as_ref().map(|scan| scan.root.clone()) else {
            return;
        };
        let count = paths.len();
        let Some(queue) = ImageQueue::from_batch(paths) else {
            return;
        };
        self.queue = Some(queue);
        let recent = if is_folder {
            RecentSource::folder(root.clone(), count)
        } else {
            RecentSource::image(root.clone())
        };
        self.recent_existence.record(root, true);
        self.config.push_recent_source(recent);
        self.request_current();
    }

    fn flush_pending_paths(&mut self) {
        if self.pending_paths.is_empty() {
            return;
        }
        let mut batch = std::mem::take(&mut self.pending_paths);
        sort_paths_naturally(&mut batch);
        let inserted = self
            .queue
            .as_mut()
            .map_or(0, |queue| queue.merge_sorted(&batch));
        if inserted > 0 {
            self.prefetch_neighbours();
        }
    }

    fn finish_scan(&mut self, generation: u64, total: usize) {
        if !self.scan_matches(generation) {
            return;
        }
        self.flush_pending_paths();
        let Some(scan) = self.scan.take() else {
            return;
        };
        if scan.is_folder {
            self.config.set_recent_image_count(&scan.root, total);
            self.notify(format!(
                "{total} images in {}",
                display_file_name(&scan.root)
            ));
        }
    }

    fn fail_scan(&mut self, generation: u64, error: &ImageIoError) {
        if !self.scan_matches(generation) {
            return;
        }
        self.pending_paths.clear();
        let Some(scan) = self.scan.take() else {
            return;
        };
        self.recent_existence
            .record(scan.root.clone(), !is_missing_source(error));
        self.report_error(format!(
            "Could not open {}: {error}",
            display_file_name(&scan.root)
        ));
    }

    fn apply_probe(&mut self, path: &Path, facts: PathFacts) {
        self.recent_existence
            .record(path.to_path_buf(), facts.exists);
        let source_parent = self.source_field.apply_probe(path, facts, PathRole::Source);
        let destination_parent =
            self.destination_field
                .apply_probe(path, facts, PathRole::Destination);
        if let Some(filesystem) = self.filesystem.as_mut() {
            for parent in [source_parent, destination_parent].into_iter().flatten() {
                let _submitted = filesystem.probe(&parent);
            }
        }
    }

    fn apply_dialog_result(&mut self, kind: DialogKind, path: Option<PathBuf>) {
        self.dialog_in_flight = None;
        let Some(path) = path else {
            return;
        };
        match kind {
            DialogKind::SourceImage | DialogKind::SourceFolder => {
                self.source_field.set_committed(Some(&path));
                self.open_source(path);
            }
            DialogKind::ExportDestination => {
                self.destination_field.set_committed(Some(&path));
                self.config.export_mut().set_destination(Some(path));
                self.capture_plan = None;
            }
        }
    }

    fn refresh_recent_existence(&mut self) {
        if !self.recent_existence.take_due(Instant::now()) {
            return;
        }
        let paths: Vec<PathBuf> = self
            .config
            .recent_sources()
            .iter()
            .map(|entry| entry.path().to_path_buf())
            .collect();
        let Some(filesystem) = self.filesystem.as_mut() else {
            return;
        };
        for path in &paths {
            let _submitted = filesystem.probe(path);
        }
    }

    fn submit_field_probes(&mut self) {
        let now = Instant::now();
        let due = [
            self.source_field.due_probe(now),
            self.destination_field.due_probe(now),
        ];
        let Some(filesystem) = self.filesystem.as_mut() else {
            return;
        };
        for path in due.into_iter().flatten() {
            let _submitted = filesystem.probe(&path);
        }
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
        if self.scan.is_some() {
            context.request_repaint_after(Duration::from_millis(100));
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
