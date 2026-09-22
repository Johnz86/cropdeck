use eframe::egui::{self, CornerRadius, Vec2};

use crate::crop::{AspectRatio, CropRect, SourceSize};
use crate::presets::{PresetDimensions, SizeTier};

use super::crop_commands::{crop_area, matching_tier};
use super::{CropDeckApp, CropSizePreference};

const SEGMENT_PADDING: Vec2 = Vec2::new(5.0, 0.0);
const RESOLUTION_SPACING: f32 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SizeChoice {
    Tier(SizeTier),
    Maximum,
}

fn segment_corners(index: usize, count: usize, radius: u8) -> CornerRadius {
    CornerRadius {
        nw: if index == 0 { radius } else { 0 },
        sw: if index == 0 { radius } else { 0 },
        ne: if index + 1 == count { radius } else { 0 },
        se: if index + 1 == count { radius } else { 0 },
    }
}

fn fitting_tiers(ratio: AspectRatio, source: SourceSize) -> Vec<PresetDimensions> {
    PresetDimensions::fitting(ratio, source).collect()
}

fn is_selected(
    choice: SizeChoice,
    preference: CropSizePreference,
    active_tier: Option<SizeTier>,
) -> bool {
    match (choice, preference) {
        (SizeChoice::Tier(tier), CropSizePreference::Tier(preferred)) => tier == preferred,
        (SizeChoice::Tier(tier), CropSizePreference::Automatic) => active_tier == Some(tier),
        (SizeChoice::Maximum, CropSizePreference::Maximum) => true,
        (SizeChoice::Tier(_), CropSizePreference::Maximum)
        | (SizeChoice::Maximum, CropSizePreference::Automatic | CropSizePreference::Tier(_)) => {
            false
        }
    }
}

impl CropDeckApp {
    pub(super) fn size_rail(&mut self, ui: &mut egui::Ui, crop: CropRect) {
        let Some(source) = self.source_size() else {
            return;
        };
        let ratio = self.config.aspect_ratio();
        let tiers = fitting_tiers(ratio, source);
        let active_tier = matching_tier(ratio, crop);
        let segment_count = tiers.len() + 1;
        let radius = ui.visuals().widgets.inactive.corner_radius.nw;
        let mut choice = None;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.spacing_mut().button_padding = SEGMENT_PADDING;
            let segments = tiers
                .iter()
                .map(|dimensions| {
                    (
                        SizeChoice::Tier(dimensions.tier()),
                        dimensions.tier().label(),
                        format!(
                            "{} x {}  ({:.1} MP)",
                            dimensions.width(),
                            dimensions.height(),
                            dimensions.megapixels()
                        ),
                    )
                })
                .chain(std::iter::once((
                    SizeChoice::Maximum,
                    "Max",
                    String::from("Use the largest crop that fits the source"),
                )));
            for (index, (candidate, label, tooltip)) in segments.enumerate() {
                let selected = is_selected(candidate, self.size_preference, active_tier);
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new(label).small())
                            .corner_radius(segment_corners(index, segment_count, radius))
                            .selected(selected),
                    )
                    .on_hover_text(tooltip)
                    .clicked()
                {
                    choice = Some(candidate);
                }
            }
        });
        match choice {
            Some(SizeChoice::Tier(tier)) => self.apply_tier(tier),
            Some(SizeChoice::Maximum) => self.apply_maximum(),
            None => {}
        }
        let crop = self.workspace.crop.unwrap_or(crop);
        ui.add_space(ui.spacing().item_spacing.x);
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = RESOLUTION_SPACING;
            ui.monospace(format!("{}×{}", crop.width(), crop.height()));
            ui.weak(format!("{:.1} MP", crop_area(crop) as f64 / 1_000_000.0));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segmented_control_rounds_only_its_outer_corners() {
        let first = segment_corners(0, 3, 4);
        let middle = segment_corners(1, 3, 4);
        let last = segment_corners(2, 3, 4);

        assert_eq!((first.nw, first.sw, first.ne, first.se), (4, 4, 0, 0));
        assert_eq!(middle, CornerRadius::ZERO);
        assert_eq!((last.nw, last.sw, last.ne, last.se), (0, 0, 4, 4));
    }

    #[test]
    fn tiers_larger_than_the_source_are_left_out_of_the_rail() {
        let small = SourceSize::new(700, 700).expect("source should be valid");
        let large = SourceSize::new(8_000, 8_000).expect("source should be valid");

        let small_tiers = fitting_tiers(AspectRatio::SQUARE, small);
        let large_tiers = fitting_tiers(AspectRatio::SQUARE, large);

        assert!(small_tiers.iter().all(|dimensions| dimensions.fits(small)));
        assert!(small_tiers.len() < large_tiers.len());
        assert_eq!(large_tiers.len(), SizeTier::ALL.len());
    }

    #[test]
    fn selection_follows_the_preference_or_the_matching_tier() {
        let tier = SizeChoice::Tier(SizeTier::Xs);

        assert!(is_selected(
            tier,
            CropSizePreference::Tier(SizeTier::Xs),
            None
        ));
        assert!(is_selected(
            tier,
            CropSizePreference::Automatic,
            Some(SizeTier::Xs)
        ));
        assert!(!is_selected(
            tier,
            CropSizePreference::Maximum,
            Some(SizeTier::Xs)
        ));
        assert!(is_selected(
            SizeChoice::Maximum,
            CropSizePreference::Maximum,
            None
        ));
        assert!(!is_selected(
            SizeChoice::Maximum,
            CropSizePreference::Automatic,
            None
        ));
    }
}
