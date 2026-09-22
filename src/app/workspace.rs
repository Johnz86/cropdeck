use std::time::Instant;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

use crate::crop::{AspectRatio, CropRect, SourceSize};
use crate::viewport::{SourceRect, ViewportTransform};

use super::interaction::{CropDrag, WorkspaceInteraction, interact_with_workspace};
use super::panels::{SourceAction, recent_entry_button};
use super::{CropDeckApp, display_file_name};

const LAUNCHER_BUTTON_SIZE: Vec2 = Vec2::new(300.0, 30.0);
const MINIMUM_ZOOM: f32 = 0.05;
const MAXIMUM_ZOOM: f32 = 8.0;
pub(super) const ZOOM_FACTOR: f32 = 1.2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum ZoomMode {
    FitWidth,
    Manual(f32),
}

impl ZoomMode {
    pub(super) fn scale(self, source_width: u32, available_width: f32) -> f32 {
        match self {
            Self::FitWidth => {
                ViewportTransform::fit_width(source_width, available_width).unwrap_or(1.0)
            }
            Self::Manual(scale) => scale.clamp(MINIMUM_ZOOM, MAXIMUM_ZOOM),
        }
    }

    pub(super) fn zoomed(self, source_width: u32, available_width: f32, factor: f32) -> Self {
        let scale = self.scale(source_width, available_width) * factor;
        Self::Manual(scale.clamp(MINIMUM_ZOOM, MAXIMUM_ZOOM))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct HistoryEntry {
    pub(super) crop: CropRect,
    pub(super) index: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(super) struct ResizeWheelState {
    pub(super) accumulator: f32,
    pub(super) latched_width: Option<u32>,
}

#[derive(Debug)]
pub(super) struct WorkspaceState {
    pub(super) crop: Option<CropRect>,
    pub(super) history: Vec<HistoryEntry>,
    pub(super) zoom: ZoomMode,
    pub(super) drag: Option<CropDrag>,
    pub(super) requested_scroll_y: Option<f32>,
    pub(super) ensure_crop_visible: bool,
    pub(super) resize_wheel: ResizeWheelState,
    pub(super) effective_ratio: AspectRatio,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            crop: None,
            history: Vec::new(),
            zoom: ZoomMode::FitWidth,
            drag: None,
            requested_scroll_y: Some(0.0),
            ensure_crop_visible: false,
            resize_wheel: ResizeWheelState::default(),
            effective_ratio: AspectRatio::default(),
        }
    }
}

impl CropDeckApp {
    pub(super) fn workspace(&mut self, root_ui: &mut egui::Ui) {
        let context = root_ui.ctx().clone();
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(22, 24, 29)))
            .show(root_ui, |ui| {
                let available_width = (ui.available_width() - 14.0).max(1.0);
                self.handle_shortcuts(&context, available_width);
                let (Some(source_size), Some(crop), true) = (
                    self.source_size(),
                    self.workspace.crop,
                    self.texture.is_some(),
                ) else {
                    self.launcher(ui);
                    return;
                };
                let Some(texture) = self.texture.as_mut() else {
                    return;
                };

                let scale = self
                    .workspace
                    .zoom
                    .scale(source_size.width(), available_width);
                let image_size = Vec2::new(
                    source_size.width() as f32 * scale,
                    source_size.height() as f32 * scale,
                );
                let mut scroll_area = egui::ScrollArea::both()
                    .id_salt("source_image_scroll")
                    .auto_shrink([false, false]);
                if let Some(scroll_y) = self.workspace.requested_scroll_y.take() {
                    scroll_area = scroll_area.vertical_scroll_offset(scroll_y);
                }

                let history = &self.workspace.history;
                let drag = &mut self.workspace.drag;
                let crop_state = &mut self.workspace.crop;
                let resize_wheel = &mut self.workspace.resize_wheel;
                let effective_ratio = &mut self.workspace.effective_ratio;
                let size_preference = &mut self.size_preference;
                let mut ensure_crop_visible =
                    std::mem::take(&mut self.workspace.ensure_crop_visible);
                let nominal_ratio = self.config.aspect_ratio();
                scroll_area.show_viewport(ui, |ui, viewport| {
                    let content_size = Vec2::new(viewport.width().max(image_size.x), image_size.y);
                    let (response, painter) =
                        ui.allocate_painter(content_size, Sense::click_and_drag());
                    let image_origin = Pos2::new(
                        response.rect.left() + (content_size.x - image_size.x) * 0.5,
                        response.rect.top(),
                    );
                    let transform = ViewportTransform::new(image_origin, scale)
                        .expect("workspace scale is always finite and positive");
                    texture.paint(&context, &painter, transform);
                    paint_overlays(&painter, transform, source_size, crop, history);
                    let mut interaction = WorkspaceInteraction {
                        source_size,
                        history,
                        nominal_ratio,
                        effective_ratio,
                        crop: crop_state,
                        drag,
                        resize_wheel,
                        size_preference,
                        ensure_crop_visible: &mut ensure_crop_visible,
                    };
                    interact_with_workspace(ui, &response, transform, &mut interaction);
                    if *interaction.ensure_crop_visible
                        && let Some(visible_crop) = *interaction.crop
                    {
                        ui.scroll_to_rect(
                            transform.source_rect_to_display(crop_to_source_rect(visible_crop)),
                            None,
                        );
                    }
                });
            });
    }
}

impl CropDeckApp {
    fn launcher(&mut self, ui: &mut egui::Ui) {
        let mut action = None;
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.25).max(24.0));
            ui.heading("Open an image or folder to start cropping");
            ui.weak("JPEG, PNG, and WebP sources are supported.");
            ui.add_space(16.0);
            for (label, shortcut, candidate) in [
                ("Open image...", "Ctrl+O", SourceAction::OpenImage),
                ("Open folder...", "Ctrl+Shift+O", SourceAction::OpenFolder),
            ] {
                if ui
                    .add_sized(
                        LAUNCHER_BUTTON_SIZE,
                        egui::Button::new(label).shortcut_text(shortcut),
                    )
                    .clicked()
                {
                    action = Some(candidate);
                }
            }
            if let Some(scan) = self.scan.as_ref() {
                ui.add_space(20.0);
                ui.add(egui::Spinner::new());
                ui.weak(format!("Scanning {}", display_file_name(&scan.root)));
                return;
            }
            ui.add_space(10.0);
            let paste = ui.add_sized(
                egui::Vec2::new(LAUNCHER_BUTTON_SIZE.x, LAUNCHER_BUTTON_SIZE.y * 0.6),
                egui::TextEdit::singleline(self.source_field.draft_mut())
                    .hint_text("or paste a folder or image path"),
            );
            if paste.changed() {
                self.source_field.mark_edited(Instant::now());
            }
            if paste.lost_focus() {
                self.source_field.request_commit();
            }
            let state = self.source_field.state();
            if state.is_rejected() {
                let error_color = ui.visuals().error_fg_color;
                ui.colored_label(error_color, state.note());
            }
            self.recent_existence.mark_visible();
            let recent = self.config.recent_sources();
            if !recent.is_empty() {
                ui.add_space(20.0);
                ui.label(egui::RichText::new("Recent").small().strong());
                for entry in recent {
                    let availability = self.recent_existence.availability(entry.path());
                    if let Some(entry_action) =
                        recent_entry_button(ui, entry, availability, LAUNCHER_BUTTON_SIZE.x)
                    {
                        action = Some(entry_action);
                    }
                }
            }
        });
        if let Some(action) = action {
            self.apply_source_action(action);
        }
    }
}

fn paint_overlays(
    painter: &egui::Painter,
    transform: ViewportTransform,
    source_size: SourceSize,
    crop: CropRect,
    history: &[HistoryEntry],
) {
    let image_rect = transform.source_rect_to_display(SourceRect {
        x: 0.0,
        y: 0.0,
        width: source_size.width() as f32,
        height: source_size.height() as f32,
    });
    let crop_rect = transform.source_rect_to_display(crop_to_source_rect(crop));
    let shade = Color32::from_black_alpha(115);
    for outside in outside_rectangles(image_rect, crop_rect) {
        painter.rect_filled(outside, 0.0, shade);
    }

    for entry in history {
        let rect = transform.source_rect_to_display(crop_to_source_rect(entry.crop));
        painter.rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(107, 195, 255, 150)),
            StrokeKind::Inside,
        );
        painter.text(
            rect.left_top() + Vec2::splat(5.0),
            Align2::LEFT_TOP,
            format!("{:03}", entry.index),
            FontId::monospace(12.0),
            Color32::from_rgb(185, 225, 255),
        );
    }
    painter.rect_stroke(
        crop_rect,
        0.0,
        Stroke::new(2.0, Color32::from_rgb(255, 194, 72)),
        StrokeKind::Inside,
    );
}

pub(super) fn crop_to_source_rect(crop: CropRect) -> SourceRect {
    SourceRect {
        x: crop.x() as f32,
        y: crop.y() as f32,
        width: crop.width() as f32,
        height: crop.height() as f32,
    }
}

fn outside_rectangles(image: Rect, crop: Rect) -> [Rect; 4] {
    [
        Rect::from_min_max(image.min, Pos2::new(image.right(), crop.top())),
        Rect::from_min_max(Pos2::new(image.left(), crop.bottom()), image.max),
        Rect::from_min_max(
            Pos2::new(image.left(), crop.top()),
            Pos2::new(crop.left(), crop.bottom()),
        ),
        Rect::from_min_max(
            Pos2::new(crop.right(), crop.top()),
            Pos2::new(image.right(), crop.bottom()),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_and_manual_zoom_are_bounded() {
        assert_eq!(ZoomMode::FitWidth.scale(1_600, 800.0), 0.5);
        assert_eq!(ZoomMode::Manual(0.001).scale(100, 100.0), MINIMUM_ZOOM);
        assert_eq!(ZoomMode::Manual(100.0).scale(100, 100.0), MAXIMUM_ZOOM);
    }

    #[test]
    fn zooming_fit_width_uses_current_viewport_scale() {
        assert_eq!(
            ZoomMode::FitWidth.zoomed(1_600, 800.0, 2.0),
            ZoomMode::Manual(1.0)
        );
    }

    #[test]
    fn crop_conversion_keeps_source_pixels() {
        let source = SourceSize::new(100, 200).expect("source should be valid");
        let crop = CropRect::new(10, 20, 30, 40, source).expect("crop should fit");

        assert_eq!(
            crop_to_source_rect(crop),
            SourceRect {
                x: 10.0,
                y: 20.0,
                width: 30.0,
                height: 40.0
            }
        );
    }

    #[test]
    fn dimming_rectangles_cover_only_outside_crop() {
        let image = Rect::from_min_max(Pos2::ZERO, Pos2::new(100.0, 200.0));
        let crop = Rect::from_min_max(Pos2::new(20.0, 50.0), Pos2::new(80.0, 150.0));

        let outside = outside_rectangles(image, crop);
        let outside_area: f32 = outside.iter().map(Rect::area).sum();

        assert_eq!(outside_area, image.area() - crop.area());
    }
}
