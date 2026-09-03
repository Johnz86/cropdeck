use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, FontId, text::LayoutJob};

use crate::config::{RecentSource, SourceKind};

use super::format_bar::{FormatBarDensity, bar_button, file_tile};
use super::workspace::{ZOOM_FACTOR, ZoomMode};
use super::{CropDeckApp, StatusLevel, StatusMessage, display_file_name};

const RECENT_ENTRY_WIDTH: f32 = 360.0;

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
            SourceAction::ClearRecent => self.config.clear_recent_sources(),
            SourceAction::Settings => self.settings_open = true,
            SourceAction::About => self.about_open = true,
        }
    }

    fn file_menu(&self, ui: &mut egui::Ui) -> Option<SourceAction> {
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

    fn recent_menu(&self, ui: &mut egui::Ui) -> Option<SourceAction> {
        ui.set_min_width(RECENT_ENTRY_WIDTH);
        let recent = self.config.recent_sources();
        if recent.is_empty() {
            ui.add_enabled(false, egui::Button::new("No recent sources"));
            return None;
        }
        let mut action = None;
        for entry in recent {
            if let Some(entry_action) = recent_entry_button(ui, entry, RECENT_ENTRY_WIDTH) {
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

pub(super) fn recent_entry_button(
    ui: &mut egui::Ui,
    entry: &RecentSource,
    width: f32,
) -> Option<SourceAction> {
    let exists = entry.path().exists();
    let parent = entry
        .path()
        .parent()
        .map(|parent| parent.display().to_string())
        .unwrap_or_default();
    let detail = match (exists, entry.kind(), entry.image_count()) {
        (false, _, _) => format!("{parent} · not found"),
        (true, SourceKind::Folder, Some(count)) if count > 0 => {
            format!("{parent} · {count} images")
        }
        (true, SourceKind::Folder, _) => format!("{parent} · folder"),
        (true, SourceKind::Image, _) => parent,
    };
    let visuals = ui.visuals();
    let name_color = if exists {
        visuals.text_color()
    } else {
        visuals.weak_text_color()
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
    let path = entry.path().to_owned();
    Some(if exists {
        SourceAction::OpenRecent(path)
    } else {
        SourceAction::ForgetRecent(path)
    })
}
