use crate::crop::{AspectRatio, CropRect, CropResizeDirection, SourceSize};
use crate::presets::{PresetDimensions, SizeTier};

use super::format_bar::FormatAction;
use super::workspace::ResizeWheelState;
use super::{CropDeckApp, CropSizePreference};

pub(super) const FINE_RESIZE_PIXELS: u32 = 16;
const COMMON_RATIOS: [AspectRatio; 6] = [
    AspectRatio::SQUARE,
    AspectRatio::PORTRAIT_2_3,
    AspectRatio::LANDSCAPE_3_2,
    AspectRatio::PORTRAIT_4_5,
    AspectRatio::PORTRAIT_9_16,
    AspectRatio::LANDSCAPE_16_9,
];

impl CropDeckApp {
    pub(super) fn source_size(&self) -> Option<SourceSize> {
        let dimensions = self.source.as_ref()?.dimensions();
        SourceSize::new(dimensions.width(), dimensions.height()).ok()
    }

    pub(super) fn apply_ratio(&mut self, ratio: AspectRatio) {
        self.config.set_aspect_ratio(ratio);
        if let (Some(crop), Some(source)) = (self.workspace.crop, self.source_size()) {
            let resized = resize_for_ratio(crop, ratio, source, self.size_preference);
            self.workspace.effective_ratio = crop_effective_ratio(resized);
            self.workspace.crop = Some(resized);
            self.workspace.ensure_crop_visible = true;
            self.workspace.resize_wheel = ResizeWheelState::default();
        }
    }

    pub(super) fn cycle_ratio(&mut self, reverse: bool) {
        self.apply_ratio(next_common_ratio(self.config.aspect_ratio(), reverse));
    }

    pub(super) fn move_crop(&mut self, delta_x: i64, delta_y: i64) {
        if let (Some(crop), Some(source)) = (self.workspace.crop, self.source_size()) {
            self.workspace.crop = Some(crop.moved_by(delta_x, delta_y, source));
            self.workspace.ensure_crop_visible = true;
        }
    }

    pub(super) fn resize_crop(&mut self, delta: i64) {
        if let (Some(crop), Some(source)) = (self.workspace.crop, self.source_size()) {
            let resized = crop.resized_by(delta, self.workspace.effective_ratio, source);
            self.workspace.crop = Some(resized);
            self.workspace.effective_ratio = crop_effective_ratio(resized);
            self.size_preference = CropSizePreference::Automatic;
            self.workspace.ensure_crop_visible = true;
            self.workspace.resize_wheel = ResizeWheelState::default();
        }
    }

    pub(super) fn snap_crop(&mut self, direction: CropResizeDirection) {
        if let (Some(crop), Some(source)) = (self.workspace.crop, self.source_size()) {
            let resized = crop.snapped_size(direction, self.config.aspect_ratio(), source);
            self.workspace.crop = Some(resized);
            self.workspace.effective_ratio = crop_effective_ratio(resized);
            self.size_preference = matching_tier(self.config.aspect_ratio(), resized)
                .map_or(CropSizePreference::Automatic, CropSizePreference::Tier);
            self.workspace.ensure_crop_visible = true;
            self.workspace.resize_wheel = ResizeWheelState {
                accumulator: 0.0,
                latched_width: Some(resized.width()),
            };
        }
    }

    pub(super) fn apply_format_action(&mut self, action: FormatAction) {
        match action {
            FormatAction::SetRatio(ratio) => self.apply_ratio(ratio),
            FormatAction::SetTier(tier) => {
                self.size_preference = CropSizePreference::Tier(tier);
                let dimensions = PresetDimensions::for_ratio(tier, self.config.aspect_ratio());
                if let Some(source) = self.source_size()
                    && dimensions.fits(source)
                {
                    self.apply_dimensions(dimensions, source);
                }
            }
            FormatAction::SetMaximum => {
                self.size_preference = CropSizePreference::Maximum;
                if let (Some(crop), Some(source)) = (self.workspace.crop, self.source_size()) {
                    let maximum = CropRect::largest_centered(source, self.config.aspect_ratio());
                    let resized =
                        crop.resized_to_dimensions(maximum.width(), maximum.height(), source);
                    self.workspace.crop = Some(resized);
                    self.workspace.effective_ratio = crop_effective_ratio(resized);
                    self.workspace.ensure_crop_visible = true;
                    self.workspace.resize_wheel = ResizeWheelState::default();
                }
            }
            FormatAction::ApplyCustom => {
                match AspectRatio::new(self.custom_ratio_width, self.custom_ratio_height) {
                    Ok(ratio) => self.apply_ratio(ratio),
                    Err(error) => self.report_error(error.to_string()),
                }
            }
        }
    }

    fn apply_dimensions(&mut self, dimensions: PresetDimensions, source: SourceSize) {
        if let Some(crop) = self.workspace.crop {
            let resized =
                crop.resized_to_dimensions(dimensions.width(), dimensions.height(), source);
            self.workspace.crop = Some(resized);
            self.workspace.effective_ratio = dimensions.effective_ratio();
            self.workspace.ensure_crop_visible = true;
            self.workspace.resize_wheel = ResizeWheelState {
                accumulator: 0.0,
                latched_width: Some(dimensions.width()),
            };
        }
    }
}

fn preferred_dimensions(
    preference: CropSizePreference,
    ratio: AspectRatio,
    source: SourceSize,
    target_area: u64,
) -> Option<PresetDimensions> {
    match preference {
        CropSizePreference::Automatic => {
            PresetDimensions::nearest_fitting(ratio, source, target_area)
        }
        CropSizePreference::Tier(tier) => Some(PresetDimensions::for_ratio(tier, ratio))
            .filter(|dimensions| dimensions.fits(source))
            .or_else(|| PresetDimensions::fitting(ratio, source).last()),
        CropSizePreference::Maximum => None,
    }
}

fn resize_for_ratio(
    crop: CropRect,
    ratio: AspectRatio,
    source: SourceSize,
    preference: CropSizePreference,
) -> CropRect {
    if preference == CropSizePreference::Maximum {
        let maximum = CropRect::largest_centered(source, ratio);
        return crop.resized_to_dimensions(maximum.width(), maximum.height(), source);
    }
    preferred_dimensions(preference, ratio, source, crop_area(crop)).map_or_else(
        || crop.with_ratio(ratio, source),
        |dimensions| crop.resized_to_dimensions(dimensions.width(), dimensions.height(), source),
    )
}

pub(super) fn matching_tier(ratio: AspectRatio, crop: CropRect) -> Option<SizeTier> {
    SizeTier::ALL.into_iter().find(|tier| {
        let dimensions = PresetDimensions::for_ratio(*tier, ratio);
        dimensions.width() == crop.width() && dimensions.height() == crop.height()
    })
}

fn next_common_ratio(current: AspectRatio, reverse: bool) -> AspectRatio {
    let current_index = COMMON_RATIOS.iter().position(|ratio| *ratio == current);
    match (current_index, reverse) {
        (Some(0), true) | (None, true) => COMMON_RATIOS[COMMON_RATIOS.len() - 1],
        (Some(index), true) => COMMON_RATIOS[index - 1],
        (Some(index), false) => COMMON_RATIOS[(index + 1) % COMMON_RATIOS.len()],
        (None, false) => COMMON_RATIOS[0],
    }
}

pub(super) fn crop_effective_ratio(crop: CropRect) -> AspectRatio {
    AspectRatio::new(crop.width(), crop.height())
        .expect("a valid crop always has non-zero dimensions")
}

pub(super) const fn crop_area(crop: CropRect) -> u64 {
    crop.width() as u64 * crop.height() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_ratio_cycle_supports_reverse_and_custom_entry() {
        let custom = AspectRatio::new(13, 19).expect("ratio should be valid");

        assert_eq!(
            next_common_ratio(AspectRatio::SQUARE, false),
            AspectRatio::PORTRAIT_2_3
        );
        assert_eq!(
            next_common_ratio(AspectRatio::SQUARE, true),
            AspectRatio::LANDSCAPE_16_9
        );
        assert_eq!(next_common_ratio(custom, false), AspectRatio::SQUARE);
        assert_eq!(next_common_ratio(custom, true), AspectRatio::LANDSCAPE_16_9);
    }

    #[test]
    fn ratio_change_applies_exact_catalog_pixels_for_preferred_tier() {
        let source = SourceSize::new(1_024, 1_024).expect("source should be valid");
        let crop = CropRect::new(256, 256, 512, 512, source).expect("crop should fit");

        let resized = resize_for_ratio(
            crop,
            AspectRatio::PORTRAIT_2_3,
            source,
            CropSizePreference::Tier(SizeTier::Xs),
        );

        assert_eq!((resized.width(), resized.height()), (344, 512));
        assert_eq!(
            crop_effective_ratio(resized),
            AspectRatio::new(43, 64).expect("effective ratio should be valid")
        );
        assert_eq!(
            matching_tier(AspectRatio::PORTRAIT_2_3, resized),
            Some(SizeTier::Xs)
        );
    }

    #[test]
    fn unavailable_preferred_tier_falls_back_to_largest_that_fits() {
        let source = SourceSize::new(700, 700).expect("source should be valid");
        let crop = CropRect::new(50, 50, 600, 600, source).expect("crop should fit");

        let resized = resize_for_ratio(
            crop,
            AspectRatio::SQUARE,
            source,
            CropSizePreference::Tier(SizeTier::Xxl),
        );

        assert_eq!((resized.width(), resized.height()), (512, 512));
    }
}
