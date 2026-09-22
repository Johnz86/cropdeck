use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, FontId, text::LayoutJob};

use crate::config::{RecentSource, SourceKind};

use super::format_bar::{FormatBarDensity, bar_button, file_tile};
use super::workspace::{ZOOM_FACTOR, ZoomMode};
use super::{CropDeckApp, StatusLevel, StatusMessage, display_file_name};

const RECENT_ENTRY_WIDTH: f32 = 360.0;
const RECENT_PROBE_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecentAvailability {
    #[default]
    Unknown,
    Present,
    Missing,
}

#[derive(Debug, Default)]
pub(super) struct RecentExistence {
    states: HashMap<PathBuf, bool>,
    last_refresh: Option<Instant>,
    visible: bool,
}

impl RecentExistence {
    pub(super) fn availability(&self, path: &Path) -> RecentAvailability {
        match self.states.get(path) {
            None => RecentAvailability::Unknown,
            Some(true) => RecentAvailability::Present,
            Some(false) => RecentAvailability::Missing,
        }
    }

    pub(super) fn record(&mut self, path: PathBuf, exists: bool) {
        self.states.insert(path, exists);
    }

    pub(super) fn forget(&mut self, path: &Path) {
        self.states.remove(path);
    }

    pub(super) fn clear(&mut self) {
        self.states.clear();
    }

    pub(super) fn mark_visible(&mut self) {
        self.visible = true;
    }

    pub(super) fn take_due(&mut self, now: Instant) -> bool {
        if !std::mem::take(&mut self.visible) {
            return false;
        }
        if self
            .last_refresh
            .is_some_and(|last| now.duration_since(last) < RECENT_PROBE_INTERVAL)
        {
            return false;
        }
        self.last_refresh = Some(now);
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SourceAction {
    OpenImage,
    OpenFolder,
    OpenRecent(PathBuf),
    ForgetRecent(PathBuf),
    ClearRecent,
    Settings,
    About,
}

impl CropDeckApp {
    pub(super) fn toolbar(&mut self, root_ui: &mut egui::Ui) {
        egui::Panel::top("cropdeck_toolbar").show(root_ui, |ui| {
            let density = FormatBarDensity::for_width(ui.ctx().content_rect().width());
            ui.horizontal(|ui| {
                if let Some(action) = self.file_menu(ui) {
                    self.apply_source_action(action);
                }
                ui.separator();
                if let Some(action) = self.format_bar(ui, density) {
                    self.apply_format_action(action);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let capture = ui.add_enabled(
                        self.source.is_some(),
                        bar_button(egui::RichText::new("Capture").strong())
                            .shortcut_text("Space")
                            .fill(ui.visuals().selection.bg_fill),
                    );
                    if capture
                        .on_hover_text("Export the crop and advance")
                        .clicked()
                    {
                        self.capture_and_advance();
                    }
                });
            });
        });
    }

    pub(super) fn apply_source_action(&mut self, action: SourceAction) {
        match action {
            SourceAction::OpenImage => self.open_image_dialog(),
            SourceAction::OpenFolder => self.open_folder_dialog(),
            SourceAction::OpenRecent(path) => self.open_source(path),
            SourceAction::ForgetRecent(path) => self.forget_recent_source(&path),
            SourceAction::ClearRecent => {
                self.config.clear_recent_sources();
                self.recent_existence.clear();
            }
            SourceAction::Settings => self.settings_open = true,
            SourceAction::About => self.about_open = true,
        }
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) -> Option<SourceAction> {
        let mut action = None;
        let trigger = file_tile(ui).on_hover_text("Open sources, settings, and about");
        egui::Popup::menu(&trigger).show(|ui| {
            ui.set_min_width(220.0);
            for (label, shortcut, candidate) in [
                ("Open image...", "Ctrl+O", SourceAction::OpenImage),
                ("Open folder...", "Ctrl+Shift+O", SourceAction::OpenFolder),
            ] {
                if ui
                    .add(egui::Button::new(label).shortcut_text(shortcut))
                    .clicked()
                {
                    action = Some(candidate);
                }
            }
            ui.menu_button("Open recent", |ui| {
                if let Some(recent_action) = self.recent_menu(ui) {
                    action = Some(recent_action);
                }
            });
            ui.separator();
            if ui
                .add(egui::Button::new("Settings...").shortcut_text("Ctrl+,"))
                .clicked()
            {
                action = Some(SourceAction::Settings);
            }
            if ui.button("About CropDeck").clicked() {
                action = Some(SourceAction::About);
            }
        });
        if action.is_some() {
            egui::Popup::close_all(ui.ctx());
        }
        action
    }

    fn recent_menu(&mut self, ui: &mut egui::Ui) -> Option<SourceAction> {
        ui.set_min_width(RECENT_ENTRY_WIDTH);
        self.recent_existence.mark_visible();
        let recent = self.config.recent_sources();
        if recent.is_empty() {
            ui.add_enabled(false, egui::Button::new("No recent sources"));
            return None;
        }
        let mut action = None;
        for entry in recent {
            let availability = self.recent_existence.availability(entry.path());
            if let Some(entry_action) =
                recent_entry_button(ui, entry, availability, RECENT_ENTRY_WIDTH)
            {
                action = Some(entry_action);
            }
        }
        ui.separator();
        if ui.button("Clear recent").clicked() {
            action = Some(SourceAction::ClearRecent);
        }
        action
    }

    fn zoom_from_toolbar(&mut self, factor: f32, available_width: f32) {
        if let Some(source) = self.source_size() {
            self.workspace.zoom =
                self.workspace
                    .zoom
                    .zoomed(source.width(), available_width, factor);
        }
    }

    fn zoom_label(&self, available_width: f32) -> String {
        let scale = self.source_size().map_or(1.0, |source| {
            self.workspace.zoom.scale(source.width(), available_width)
        });
        format!("{:.0}%", scale * 100.0)
    }

    fn visible_status(&mut self, context: &egui::Context) -> Option<&StatusMessage> {
        let remaining = self
            .status
            .as_ref()
            .and_then(|status| status.remaining(Instant::now()));
        match remaining {
            Some(Duration::ZERO) => {
                self.status = None;
            }
            Some(remaining) => context.request_repaint_after(remaining),
            None => {}
        }
        self.status.as_ref()
    }

    pub(super) fn status_bar(&mut self, root_ui: &mut egui::Ui) {
        let viewport_available_width = (root_ui.available_width() - 14.0).max(1.0);
        egui::Panel::bottom("cropdeck_status").show(root_ui, |ui| {
            ui.horizontal(|ui| {
                if self.queue.as_ref().is_some_and(|queue| queue.len() > 1) {
                    if ui
                        .add_enabled(self.can_navigate(false), egui::Button::new("Previous"))
                        .on_hover_text("Previous source (Q)")
                        .clicked()
                    {
                        self.navigate(false);
                    }
                    ui.label(self.queue_position());
                    if ui
                        .add_enabled(self.can_navigate(true), egui::Button::new("Next"))
                        .on_hover_text("Next source (E)")
                        .clicked()
                    {
                        self.navigate(true);
                    }
                    ui.separator();
                }
                if let Some(source) = &self.source {
                    ui.label(display_file_name(source.path()));
                    ui.separator();
                }
                if let Some(crop) = self.workspace.crop {
                    ui.monospace(format!("x {}  y {}", crop.x(), crop.y()));
                    ui.separator();
                }
                if !self.workspace.history.is_empty() {
                    ui.label(format!("captured {}", self.workspace.history.len()));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.source.is_some() {
                        if ui.button("+").on_hover_text("Zoom in (+)").clicked() {
                            self.zoom_from_toolbar(ZOOM_FACTOR, viewport_available_width);
                        }
                        ui.label(self.zoom_label(viewport_available_width));
                        if ui.button("-").on_hover_text("Zoom out (-)").clicked() {
                            self.zoom_from_toolbar(1.0 / ZOOM_FACTOR, viewport_available_width);
                        }
                        if ui
                            .button("Fit width")
                            .on_hover_text("Fit the image width (F)")
                            .clicked()
                        {
                            self.workspace.zoom = ZoomMode::FitWidth;
                        }
                        ui.separator();
                    }
                    if let Some(scan) = self.scan.as_ref() {
                        ui.add(egui::Spinner::new().size(12.0));
                        ui.weak(format!("scanning {}", display_file_name(&scan.root)));
                        ui.separator();
                    }
                    if self.pending_exports > 0 {
                        ui.weak(format!("exporting {}", self.pending_exports));
                        ui.separator();
                    }
                    let error_color = ui.visuals().error_fg_color;
                    if let Some(status) = self.visible_status(ui.ctx()) {
                        let text = egui::RichText::new(&status.text);
                        let text = match status.level {
                            StatusLevel::Error => text.color(error_color),
                            StatusLevel::Info => text,
                        };
                        ui.add(egui::Label::new(text).truncate());
                    }
                });
            });
        });
    }
}

pub(super) fn recent_entry_detail(
    availability: RecentAvailability,
    kind: SourceKind,
    image_count: Option<usize>,
    parent: String,
) -> String {
    match (availability, kind, image_count) {
        (RecentAvailability::Missing, SourceKind::Folder | SourceKind::Image, _) => {
            format!("{parent} · not found")
        }
        (
            RecentAvailability::Unknown | RecentAvailability::Present,
            SourceKind::Folder,
            Some(count),
        ) if count > 0 => format!("{parent} · {count} images"),
        (
            RecentAvailability::Unknown | RecentAvailability::Present,
            SourceKind::Folder,
            Some(_) | None,
        ) => format!("{parent} · folder"),
        (RecentAvailability::Unknown | RecentAvailability::Present, SourceKind::Image, _) => parent,
    }
}

pub(super) fn recent_entry_action(availability: RecentAvailability, path: PathBuf) -> SourceAction {
    match availability {
        RecentAvailability::Missing => SourceAction::ForgetRecent(path),
        RecentAvailability::Unknown | RecentAvailability::Present => SourceAction::OpenRecent(path),
    }
}

pub(super) fn recent_entry_button(
    ui: &mut egui::Ui,
    entry: &RecentSource,
    availability: RecentAvailability,
    width: f32,
) -> Option<SourceAction> {
    let parent = entry
        .path()
        .parent()
        .map(|parent| parent.display().to_string())
        .unwrap_or_default();
    let detail = recent_entry_detail(availability, entry.kind(), entry.image_count(), parent);
    let visuals = ui.visuals();
    let name_color = if availability == RecentAvailability::Missing {
        visuals.weak_text_color()
    } else {
        visuals.text_color()
    };
    let mut text = LayoutJob::default();
    text.wrap.max_width = width;
    text.append(
        &display_file_name(entry.path()),
        0.0,
        egui::TextFormat {
            font_id: FontId::proportional(14.0),
            color: name_color,
            ..Default::default()
        },
    );
    text.append(
        &format!("\n{detail}"),
        0.0,
        egui::TextFormat {
            font_id: FontId::proportional(11.0),
            color: visuals.weak_text_color(),
            ..Default::default()
        },
    );
    let response = ui.add(egui::Button::new(text).min_size(egui::Vec2::new(width, 0.0)));
    if !response.clicked() {
        return None;
    }
    Some(recent_entry_action(availability, entry.path().to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parent() -> String {
        String::from("/comics")
    }

    #[test]
    fn an_unprobed_entry_renders_and_acts_as_present() {
        let unknown = recent_entry_detail(
            RecentAvailability::Unknown,
            SourceKind::Folder,
            Some(12),
            parent(),
        );
        let present = recent_entry_detail(
            RecentAvailability::Present,
            SourceKind::Folder,
            Some(12),
            parent(),
        );

        assert_eq!(unknown, present);
        assert_eq!(
            recent_entry_action(RecentAvailability::Unknown, PathBuf::from("/comics/ch1")),
            SourceAction::OpenRecent(PathBuf::from("/comics/ch1"))
        );
    }

    #[test]
    fn a_missing_entry_reports_not_found_and_offers_forget() {
        assert_eq!(
            recent_entry_detail(
                RecentAvailability::Missing,
                SourceKind::Folder,
                Some(12),
                parent()
            ),
            "/comics · not found"
        );
        assert_eq!(
            recent_entry_action(RecentAvailability::Missing, PathBuf::from("/comics/ch1")),
            SourceAction::ForgetRecent(PathBuf::from("/comics/ch1"))
        );
    }

    #[test]
    fn a_folder_without_a_usable_count_falls_back_to_a_plain_label() {
        assert_eq!(
            recent_entry_detail(
                RecentAvailability::Present,
                SourceKind::Folder,
                Some(0),
                parent()
            ),
            "/comics · folder"
        );
        assert_eq!(
            recent_entry_detail(
                RecentAvailability::Present,
                SourceKind::Folder,
                None,
                parent()
            ),
            "/comics · folder"
        );
    }

    #[test]
    fn existence_refreshes_only_when_visible_and_due() {
        let mut existence = RecentExistence::default();
        let now = Instant::now();

        assert!(!existence.take_due(now));
        existence.mark_visible();
        assert!(existence.take_due(now));
        existence.mark_visible();
        assert!(!existence.take_due(now + Duration::from_secs(1)));
        existence.mark_visible();
        assert!(existence.take_due(now + RECENT_PROBE_INTERVAL));
    }

    #[test]
    fn recorded_paths_report_their_availability_and_can_be_forgotten() {
        let mut existence = RecentExistence::default();
        let path = PathBuf::from("/comics/ch1");

        assert_eq!(existence.availability(&path), RecentAvailability::Unknown);
        existence.record(path.clone(), true);
        assert_eq!(existence.availability(&path), RecentAvailability::Present);
        existence.record(path.clone(), false);
        assert_eq!(existence.availability(&path), RecentAvailability::Missing);
        existence.forget(&path);
        assert_eq!(existence.availability(&path), RecentAvailability::Unknown);
    }
}
