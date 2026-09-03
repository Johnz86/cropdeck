use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2,
};

use crate::crop::AspectRatio;
use crate::presets::{AspectRatioCatalog, Orientation, SizeTier, embedded_catalog};

use super::crop_commands::{crop_area, matching_tier};
use super::{CropDeckApp, CropSizePreference};

const QUICK_RATIO_COUNT: usize = 9;
const CATALOG_POPUP_WIDTH: f32 = 520.0;

pub(super) const CONTROL_HEIGHT: f32 = 34.0;
const RAIL_SEGMENT_WIDTH: f32 = 34.0;
const CUSTOM_FIELD_WIDTH: f32 = 52.0;

pub(super) fn bar_button<'a>(text: impl Into<egui::WidgetText>) -> egui::Button<'a> {
    egui::Button::new(text).min_size(Vec2::new(0.0, CONTROL_HEIGHT))
}

fn segment_corners(index: usize, count: usize, radius: u8) -> CornerRadius {
    CornerRadius {
        nw: if index == 0 { radius } else { 0 },
        sw: if index == 0 { radius } else { 0 },
        ne: if index + 1 == count { radius } else { 0 },
        se: if index + 1 == count { radius } else { 0 },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FormatAction {
    SetRatio(AspectRatio),
    SetTier(SizeTier),
    SetMaximum,
    ApplyCustom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FormatBarDensity {
    Full,
    NoMegapixels,
    NoCustom,
    Compact,
}

impl FormatBarDensity {
    pub(super) fn for_width(available_width: f32) -> Self {
        if available_width >= 1_280.0 {
            Self::Full
        } else if available_width >= 1_220.0 {
            Self::NoMegapixels
        } else if available_width >= 1_080.0 {
            Self::NoCustom
        } else {
            Self::Compact
        }
    }

    const fn shows_megapixels(self) -> bool {
        matches!(self, Self::Full)
    }

    const fn shows_custom_inline(self) -> bool {
        matches!(self, Self::Full | Self::NoMegapixels)
    }

    const fn shows_quick_ratios(self) -> bool {
        !matches!(self, Self::Compact)
    }
}

#[derive(Debug, Clone, Copy)]
struct TileMetrics {
    size: Vec2,
    glyph: Vec2,
    glyph_center_y: f32,
    label_offset: f32,
    label_font: f32,
    badge_font: f32,
}

const CHIP: TileMetrics = TileMetrics {
    size: Vec2::new(46.0, CONTROL_HEIGHT),
    glyph: Vec2::new(18.0, 12.0),
    glyph_center_y: 11.0,
    label_offset: 8.0,
    label_font: 11.0,
    badge_font: 8.0,
};

const TILE: TileMetrics = TileMetrics {
    size: Vec2::new(64.0, 56.0),
    glyph: Vec2::new(28.0, 22.0),
    glyph_center_y: 20.0,
    label_offset: 11.0,
    label_font: 12.0,
    badge_font: 9.0,
};

impl CropDeckApp {
    pub(super) fn format_bar(
        &mut self,
        ui: &mut egui::Ui,
        density: FormatBarDensity,
    ) -> Option<FormatAction> {
        let ratio = self.config.aspect_ratio();
        let source = self.source_size();
        let crop = self.workspace.crop;
        let catalog = embedded_catalog().ok();
        let mut action = None;
        let mut custom_width = self.custom_ratio_width;
        let mut custom_height = self.custom_ratio_height;

        if density.shows_quick_ratios() {
            for (index, preset) in AspectRatio::PRESETS
                .into_iter()
                .take(QUICK_RATIO_COUNT)
                .enumerate()
            {
                let tile = ratio_tile(
                    ui,
                    &ratio_display_label(preset),
                    preset,
                    preset == ratio,
                    Some(index + 1),
                    CHIP,
                );
                if tile.clicked() {
                    action = Some(FormatAction::SetRatio(preset));
                }
            }
            if ratio_shortcut(ratio).is_none() {
                ratio_tile(ui, &ratio_display_label(ratio), ratio, true, None, CHIP);
            }
        }

        let trigger_label = if density.shows_quick_ratios() {
            String::from("All ratios")
        } else {
            ratio_display_label(ratio)
        };
        let trigger = ui
            .add(bar_button(trigger_label))
            .on_hover_text("Choose any catalog ratio");
        egui::Popup::menu(&trigger)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .width(CATALOG_POPUP_WIDTH)
            .show(|ui| {
                let maximum_height = (ui.ctx().content_rect().height() - 32.0).max(220.0);
                egui::ScrollArea::vertical()
                    .max_height(maximum_height)
                    .show(ui, |ui| {
                        if let Some(catalog) = catalog {
                            for orientation in [
                                Orientation::Portrait,
                                Orientation::Landscape,
                                Orientation::Square,
                            ] {
                                if let Some(selected) =
                                    ratio_section(ui, catalog, orientation, ratio)
                                {
                                    action = Some(FormatAction::SetRatio(selected));
                                    ui.close();
                                }
                            }
                        }
                        if !density.shows_custom_inline() {
                            ui.separator();
                            ui.horizontal(|ui| {
                                if custom_ratio_fields(ui, &mut custom_width, &mut custom_height) {
                                    action = Some(FormatAction::ApplyCustom);
                                }
                            });
                        }
                    });
            });

        ui.separator();
        let dimensions = SizeTier::ALL
            .map(|tier| catalog.and_then(|catalog| catalog.dimensions_for(ratio, tier)));
        let active_tier = crop.and_then(|crop| matching_tier(ratio, crop));
        let segment_count = SizeTier::ALL.len() + 1;
        let radius = ui.visuals().widgets.inactive.corner_radius.nw;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (index, (tier, dimensions)) in SizeTier::ALL.into_iter().zip(dimensions).enumerate()
            {
                let fits = dimensions
                    .zip(source)
                    .is_some_and(|(dimensions, source)| dimensions.fits(source));
                let selected = match self.size_preference {
                    CropSizePreference::Tier(preferred) => preferred == tier,
                    CropSizePreference::Automatic => active_tier == Some(tier),
                    CropSizePreference::Maximum => false,
                };
                let response = ui.add_enabled(
                    fits,
                    bar_button(tier.label())
                        .min_size(Vec2::new(RAIL_SEGMENT_WIDTH, CONTROL_HEIGHT))
                        .corner_radius(segment_corners(index, segment_count, radius))
                        .selected(selected),
                );
                let response = match dimensions {
                    Some(dimensions) => response.on_hover_text(format!(
                        "{} x {}  ({:.1} MP)",
                        dimensions.width(),
                        dimensions.height(),
                        dimensions.megapixels()
                    )),
                    None => response,
                };
                if !fits {
                    let reason = source.map_or(
                        "Open an image to choose an exact size",
                        |_source| "This preset is larger than the source image",
                    );
                    response.clone().on_disabled_hover_text(reason);
                }
                if response.clicked() {
                    action = Some(FormatAction::SetTier(tier));
                }
            }
            let maximum_selected = self.size_preference == CropSizePreference::Maximum;
            if ui
                .add_enabled(
                    source.is_some(),
                    bar_button("Max")
                        .min_size(Vec2::new(RAIL_SEGMENT_WIDTH + 6.0, CONTROL_HEIGHT))
                        .corner_radius(segment_corners(segment_count - 1, segment_count, radius))
                        .selected(maximum_selected),
                )
                .on_hover_text("Use the largest crop that fits the source")
                .clicked()
            {
                action = Some(FormatAction::SetMaximum);
            }
        });

        if let Some(crop) = crop {
            ui.separator();
            ui.monospace(format!("{} × {}", crop.width(), crop.height()));
            if density.shows_megapixels() {
                ui.weak(format!("{:.1} MP", crop_area(crop) as f64 / 1_000_000.0));
            }
        }

        if density.shows_custom_inline() {
            ui.separator();
            if custom_ratio_fields(ui, &mut custom_width, &mut custom_height) {
                action = Some(FormatAction::ApplyCustom);
            }
        }

        self.custom_ratio_width = custom_width;
        self.custom_ratio_height = custom_height;
        if action == Some(FormatAction::ApplyCustom)
            && AspectRatio::new(custom_width, custom_height).ok() == Some(ratio)
        {
            action = None;
        }
        action
    }
}

fn custom_ratio_fields(ui: &mut egui::Ui, width: &mut u32, height: &mut u32) -> bool {
    ui.label("Custom")
        .on_hover_text("Type a ratio and press Enter to apply it. Esc reverts.");
    let field_size = Vec2::new(CUSTOM_FIELD_WIDTH, CONTROL_HEIGHT);
    let width_response = ui.add_sized(field_size, egui::DragValue::new(width).range(1..=100_000));
    ui.label(":");
    let height_response = ui.add_sized(field_size, egui::DragValue::new(height).range(1..=100_000));
    [width_response, height_response]
        .iter()
        .any(|response| response.lost_focus() || response.drag_stopped())
}

fn ratio_display_label(ratio: AspectRatio) -> String {
    embedded_catalog()
        .ok()
        .and_then(|catalog| catalog.ratio(ratio))
        .map_or_else(|| ratio.to_string(), |preset| preset.label().to_owned())
}

fn ratio_shortcut(ratio: AspectRatio) -> Option<usize> {
    AspectRatio::PRESETS
        .iter()
        .take(QUICK_RATIO_COUNT)
        .position(|preset| *preset == ratio)
        .map(|index| index + 1)
}

fn ratio_section(
    ui: &mut egui::Ui,
    catalog: &AspectRatioCatalog,
    orientation: Orientation,
    selected: AspectRatio,
) -> Option<AspectRatio> {
    let mut selection = None;
    ui.label(
        egui::RichText::new(orientation.to_string())
            .small()
            .strong(),
    );
    egui::Grid::new(("ratio_grid", orientation))
        .num_columns(7)
        .show(ui, |ui| {
            for (index, preset) in catalog.ratios_for(orientation).enumerate() {
                if ratio_tile(
                    ui,
                    preset.label(),
                    preset.ratio(),
                    preset.ratio() == selected,
                    ratio_shortcut(preset.ratio()),
                    TILE,
                )
                .clicked()
                {
                    selection = Some(preset.ratio());
                }
                if (index + 1).is_multiple_of(7) {
                    ui.end_row();
                }
            }
            ui.end_row();
        });
    selection
}

fn ratio_tile(
    ui: &mut egui::Ui,
    label: &str,
    ratio: AspectRatio,
    selected: bool,
    shortcut: Option<usize>,
    metrics: TileMetrics,
) -> egui::Response {
    icon_tile(
        ui,
        label,
        selected,
        shortcut,
        metrics,
        |painter, area, color| {
            let ratio_width = ratio.width() as f32;
            let ratio_height = ratio.height() as f32;
            let scale = (area.width() / ratio_width).min(area.height() / ratio_height);
            let glyph = Rect::from_center_size(
                area.center(),
                Vec2::new(ratio_width * scale, ratio_height * scale),
            );
            painter.rect_stroke(glyph, 2.0, Stroke::new(1.5, color), StrokeKind::Inside);
        },
    )
}

pub(super) fn file_tile(ui: &mut egui::Ui) -> egui::Response {
    icon_tile(ui, "File", false, None, CHIP, paint_folder_glyph)
}

fn paint_folder_glyph(painter: &egui::Painter, area: Rect, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    let tab_height = (area.height() * 0.25).round();
    let body = Rect::from_min_max(Pos2::new(area.left(), area.top() + tab_height), area.max);
    painter.rect_stroke(body, 2.0, stroke, StrokeKind::Inside);
    let tab_width = (area.width() * 0.4).round();
    painter.add(Shape::line(
        vec![
            Pos2::new(area.left() + 1.0, body.top()),
            Pos2::new(area.left() + 1.0, area.top() + 1.0),
            Pos2::new(area.left() + tab_width, area.top() + 1.0),
            Pos2::new(area.left() + tab_width + tab_height, body.top()),
        ],
        stroke,
    ));
}

fn icon_tile(
    ui: &mut egui::Ui,
    label: &str,
    selected: bool,
    shortcut: Option<usize>,
    metrics: TileMetrics,
    paint_glyph: impl FnOnce(&egui::Painter, Rect, Color32),
) -> egui::Response {
    let (rectangle, response) = ui.allocate_exact_size(metrics.size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, label)
    });
    let visuals = ui.style().interact_selectable(&response, selected);
    let painter = ui.painter();
    painter.rect_filled(rectangle, visuals.corner_radius, visuals.weak_bg_fill);
    painter.rect_stroke(
        rectangle,
        visuals.corner_radius,
        visuals.bg_stroke,
        StrokeKind::Inside,
    );
    let glyph_area = Rect::from_center_size(
        Pos2::new(
            rectangle.center().x,
            rectangle.top() + metrics.glyph_center_y,
        ),
        metrics.glyph,
    );
    paint_glyph(painter, glyph_area, visuals.fg_stroke.color);
    painter.text(
        Pos2::new(
            rectangle.center().x,
            rectangle.bottom() - metrics.label_offset,
        ),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(metrics.label_font),
        visuals.text_color(),
    );
    if let Some(shortcut) = shortcut {
        painter.text(
            Pos2::new(rectangle.right() - 4.0, rectangle.top() + 3.0),
            Align2::RIGHT_TOP,
            shortcut.to_string(),
            FontId::proportional(metrics.badge_font),
            visuals.text_color().gamma_multiply(0.65),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_ratio_shortcuts_keep_the_existing_mapping() {
        for (index, ratio) in AspectRatio::PRESETS.into_iter().take(9).enumerate() {
            assert_eq!(ratio_shortcut(ratio), Some(index + 1));
        }
        assert_eq!(ratio_shortcut(AspectRatio::SOCIAL_1_91_1), None);
    }

    #[test]
    fn format_bar_collapses_in_stages_as_the_window_narrows() {
        assert_eq!(FormatBarDensity::for_width(1_600.0), FormatBarDensity::Full);
        assert_eq!(
            FormatBarDensity::for_width(1_250.0),
            FormatBarDensity::NoMegapixels
        );
        assert_eq!(
            FormatBarDensity::for_width(1_100.0),
            FormatBarDensity::NoCustom
        );
        assert_eq!(
            FormatBarDensity::for_width(800.0),
            FormatBarDensity::Compact
        );

        assert!(FormatBarDensity::Full.shows_megapixels());
        assert!(FormatBarDensity::NoMegapixels.shows_custom_inline());
        assert!(!FormatBarDensity::NoCustom.shows_custom_inline());
        assert!(FormatBarDensity::NoCustom.shows_quick_ratios());
        assert!(!FormatBarDensity::Compact.shows_quick_ratios());
    }

    #[test]
    fn segmented_control_rounds_only_its_outer_corners() {
        let first = segment_corners(0, 3, 4);
        let middle = segment_corners(1, 3, 4);
        let last = segment_corners(2, 3, 4);

        assert_eq!((first.nw, first.sw, first.ne, first.se), (4, 4, 0, 0));
        assert_eq!(middle, CornerRadius::ZERO);
        assert_eq!((last.nw, last.sw, last.ne, last.se), (0, 0, 4, 4));
    }
}
