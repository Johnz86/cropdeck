use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::presets::PresetDimensions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSize {
    width: u32,
    height: u32,
}

impl SourceSize {
    pub fn new(width: u32, height: u32) -> Result<Self, CropError> {
        if width == 0 || height == 0 {
            return Err(CropError::EmptySource);
        }
        Ok(Self { width, height })
    }

    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropSize {
    width: u32,
    height: u32,
}

impl CropSize {
    const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CropResizeDirection {
    Smaller,

    Larger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct AspectRatio {
    width: u32,
    height: u32,
}

impl AspectRatio {
    pub const SQUARE: Self = Self::new_unchecked(1, 1);

    pub const PORTRAIT_2_3: Self = Self::new_unchecked(2, 3);

    pub const LANDSCAPE_3_2: Self = Self::new_unchecked(3, 2);

    pub const LANDSCAPE_4_3: Self = Self::new_unchecked(4, 3);

    pub const PORTRAIT_3_4: Self = Self::new_unchecked(3, 4);

    pub const PORTRAIT_4_5: Self = Self::new_unchecked(4, 5);

    pub const LANDSCAPE_16_9: Self = Self::new_unchecked(16, 9);

    pub const PORTRAIT_9_16: Self = Self::new_unchecked(9, 16);

    pub const LANDSCAPE_21_9: Self = Self::new_unchecked(7, 3);

    pub const SOCIAL_1_91_1: Self = Self::new_unchecked(191, 100);

    pub const PRESETS: [Self; 10] = [
        Self::SQUARE,
        Self::PORTRAIT_2_3,
        Self::LANDSCAPE_3_2,
        Self::LANDSCAPE_4_3,
        Self::PORTRAIT_3_4,
        Self::PORTRAIT_4_5,
        Self::LANDSCAPE_16_9,
        Self::PORTRAIT_9_16,
        Self::LANDSCAPE_21_9,
        Self::SOCIAL_1_91_1,
    ];

    pub fn new(width: u32, height: u32) -> Result<Self, CropError> {
        if width == 0 || height == 0 {
            return Err(CropError::InvalidAspectRatio { width, height });
        }
        let divisor = greatest_common_divisor(width, height);
        Ok(Self {
            width: width / divisor,
            height: height / divisor,
        })
    }

    const fn new_unchecked(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }

    #[must_use]
    pub fn token(self) -> String {
        format!("{}x{}", self.width, self.height)
    }

    fn dimensions_within(
        self,
        requested_width: u32,
        maximum_width: u32,
        maximum_height: u32,
    ) -> (u32, u32) {
        let width = requested_width.clamp(1, maximum_width);
        let mut height = scale_rounded(width, self.height, self.width).max(1);
        if height <= maximum_height {
            return (width, height);
        }

        height = maximum_height;
        let width = scale_rounded(height, self.width, self.height)
            .max(1)
            .min(maximum_width);
        (width, height)
    }
}

impl Default for AspectRatio {
    fn default() -> Self {
        Self::PORTRAIT_9_16
    }
}

impl fmt::Display for AspectRatio {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.width, self.height)
    }
}

impl<'de> Deserialize<'de> for AspectRatio {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Components {
            width: u32,
            height: u32,
        }

        let components = Components::deserialize(deserializer)?;
        Self::new(components.width, components.height).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CropRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl CropRect {
    pub fn new(
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        source: SourceSize,
    ) -> Result<Self, CropError> {
        if width == 0 || height == 0 {
            return Err(CropError::EmptyCrop);
        }
        let rectangle = Self {
            x,
            y,
            width,
            height,
        };
        if !rectangle.is_within(source) {
            return Err(CropError::OutOfBounds {
                rectangle,
                bounds: source,
            });
        }
        Ok(rectangle)
    }

    #[must_use]
    pub fn largest_centered(source: SourceSize, ratio: AspectRatio) -> Self {
        let (width, height) = ratio.dimensions_within(source.width, source.width, source.height);
        Self {
            x: (source.width - width) / 2,
            y: (source.height - height) / 2,
            width,
            height,
        }
    }

    #[must_use]
    pub fn with_aspect_ratio(
        x: u32,
        y: u32,
        requested_width: u32,
        ratio: AspectRatio,
        source: SourceSize,
    ) -> Self {
        let (width, height) = ratio.dimensions_within(requested_width, source.width, source.height);
        Self {
            x: x.min(source.width - width),
            y: y.min(source.height - height),
            width,
            height,
        }
    }

    #[must_use]
    pub const fn x(self) -> u32 {
        self.x
    }

    #[must_use]
    pub const fn y(self) -> u32 {
        self.y
    }

    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }

    #[must_use]
    pub const fn right(self) -> u32 {
        self.x + self.width
    }

    #[must_use]
    pub const fn bottom(self) -> u32 {
        self.y + self.height
    }

    #[must_use]
    pub fn is_within(self, source: SourceSize) -> bool {
        self.width > 0
            && self.height > 0
            && self.width <= source.width
            && self.height <= source.height
            && self.x <= source.width.saturating_sub(self.width)
            && self.y <= source.height.saturating_sub(self.height)
    }

    #[must_use]
    pub fn moved_by(self, delta_x: i64, delta_y: i64, source: SourceSize) -> Self {
        let maximum_x = source.width.saturating_sub(self.width);
        let maximum_y = source.height.saturating_sub(self.height);
        Self {
            x: add_signed_clamped(self.x, delta_x, maximum_x),
            y: add_signed_clamped(self.y, delta_y, maximum_y),
            width: self.width.min(source.width),
            height: self.height.min(source.height),
        }
    }

    #[must_use]
    pub fn positioned_at(self, x: i64, y: i64, source: SourceSize) -> Self {
        self.moved_by(
            x.saturating_sub(i64::from(self.x)),
            y.saturating_sub(i64::from(self.y)),
            source,
        )
    }

    #[must_use]
    pub fn resized_to_width(
        self,
        requested_width: u32,
        ratio: AspectRatio,
        source: SourceSize,
    ) -> Self {
        let center_x = u64::from(self.x) * 2 + u64::from(self.width);
        let center_y = u64::from(self.y) * 2 + u64::from(self.height);
        let (width, height) = ratio.dimensions_within(requested_width, source.width, source.height);
        let x = centered_origin(center_x, width, source.width);
        let y = centered_origin(center_y, height, source.height);
        Self {
            x,
            y,
            width,
            height,
        }
    }

    #[must_use]
    pub fn resized_to_dimensions(
        self,
        requested_width: u32,
        requested_height: u32,
        source: SourceSize,
    ) -> Self {
        let center_x = u64::from(self.x) * 2 + u64::from(self.width);
        let center_y = u64::from(self.y) * 2 + u64::from(self.height);
        let effective_ratio =
            AspectRatio::new_unchecked(requested_width.max(1), requested_height.max(1));
        let (width, height) =
            effective_ratio.dimensions_within(requested_width.max(1), source.width, source.height);
        Self {
            x: centered_origin(center_x, width, source.width),
            y: centered_origin(center_y, height, source.height),
            width,
            height,
        }
    }

    #[must_use]
    pub fn resized_by(self, width_delta: i64, ratio: AspectRatio, source: SourceSize) -> Self {
        let requested_width = add_signed_clamped(self.width, width_delta, source.width).max(1);
        self.resized_to_width(requested_width, ratio, source)
    }

    #[must_use]
    pub fn snapped_size(
        self,
        direction: CropResizeDirection,
        ratio: AspectRatio,
        source: SourceSize,
    ) -> Self {
        let sizes = common_crop_sizes(ratio, source);
        let selected = match direction {
            CropResizeDirection::Smaller => sizes.iter().rev().find(|size| size.width < self.width),
            CropResizeDirection::Larger => sizes.iter().find(|size| size.width > self.width),
        };
        selected.map_or(self, |size| {
            self.resized_to_dimensions(size.width, size.height, source)
        })
    }

    #[must_use]
    pub fn with_ratio(self, ratio: AspectRatio, source: SourceSize) -> Self {
        self.resized_to_width(self.width, ratio, source)
    }
}

#[must_use]
pub fn common_crop_sizes(ratio: AspectRatio, source: SourceSize) -> Vec<CropSize> {
    let mut sizes = PresetDimensions::fitting(ratio, source)
        .map(|dimensions| CropSize::new(dimensions.width(), dimensions.height()))
        .collect::<Vec<_>>();
    let maximum = CropRect::largest_centered(source, ratio);
    sizes.push(CropSize::new(maximum.width, maximum.height));
    sizes.sort_unstable_by_key(|size| (u64::from(size.width) * u64::from(size.height), size.width));
    sizes.dedup();
    sizes
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CropError {
    #[error("source dimensions must be non-zero")]
    EmptySource,

    #[error("aspect ratio must be non-zero, received {width}:{height}")]
    InvalidAspectRatio { width: u32, height: u32 },

    #[error("crop dimensions must be non-zero")]
    EmptyCrop,

    #[error("crop {rectangle:?} is outside source bounds {bounds:?}")]
    OutOfBounds {
        rectangle: CropRect,
        bounds: SourceSize,
    },
}

const fn greatest_common_divisor(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

fn scale_rounded(value: u32, numerator: u32, denominator: u32) -> u32 {
    let scaled = u64::from(value) * u64::from(numerator);
    let rounded = (scaled + u64::from(denominator) / 2) / u64::from(denominator);
    u32::try_from(rounded).unwrap_or(u32::MAX)
}

fn add_signed_clamped(value: u32, delta: i64, maximum: u32) -> u32 {
    let changed = i128::from(value) + i128::from(delta);
    changed.clamp(0, i128::from(maximum)) as u32
}

fn centered_origin(doubled_center: u64, extent: u32, source_extent: u32) -> u32 {
    let origin = doubled_center.saturating_sub(u64::from(extent)) / 2;
    u32::try_from(origin)
        .unwrap_or(u32::MAX)
        .min(source_extent - extent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_ratio_is_reduced_and_serialized_canonically() {
        let ratio = AspectRatio::new(832, 1216).expect("ratio should be valid");

        let encoded = serde_json::to_string(&ratio).expect("ratio should serialize");

        assert_eq!(
            ratio,
            AspectRatio::new(13, 19).expect("ratio should be valid")
        );
        assert_eq!(ratio.to_string(), "13:19");
        assert_eq!(ratio.token(), "13x19");
        assert_eq!(encoded, r#"{"width":13,"height":19}"#);
    }

    #[test]
    fn zero_ratio_is_rejected_by_constructor_and_deserializer() {
        assert_eq!(
            AspectRatio::new(0, 9),
            Err(CropError::InvalidAspectRatio {
                width: 0,
                height: 9
            })
        );
        assert!(serde_json::from_str::<AspectRatio>(r#"{"width":9,"height":0}"#).is_err());
    }

    #[test]
    fn crop_construction_checks_bounds() {
        let source = SourceSize::new(100, 200).expect("source should be valid");

        let error = CropRect::new(75, 0, 50, 50, source).expect_err("crop should not fit");

        assert!(matches!(error, CropError::OutOfBounds { .. }));
    }

    #[test]
    fn ratio_crop_is_sized_and_clamped_to_source() {
        let source = SourceSize::new(100, 100).expect("source should be valid");

        let crop = CropRect::with_aspect_ratio(90, 90, 80, AspectRatio::LANDSCAPE_16_9, source);

        assert_eq!((crop.x(), crop.y()), (20, 55));
        assert_eq!((crop.width(), crop.height()), (80, 45));
        assert!(crop.is_within(source));
    }

    #[test]
    fn tall_ratio_is_reduced_to_fit_height() {
        let source = SourceSize::new(100, 100).expect("source should be valid");

        let crop = CropRect::with_aspect_ratio(0, 0, 100, AspectRatio::PORTRAIT_9_16, source);

        assert_eq!((crop.width(), crop.height()), (56, 100));
    }

    #[test]
    fn movement_clamps_at_every_source_edge() {
        let source = SourceSize::new(100, 200).expect("source should be valid");
        let crop = CropRect::new(20, 30, 40, 50, source).expect("crop should fit");

        let upper_left = crop.moved_by(i64::MIN, i64::MIN, source);
        let lower_right = crop.moved_by(i64::MAX, i64::MAX, source);

        assert_eq!((upper_left.x(), upper_left.y()), (0, 0));
        assert_eq!((lower_right.x(), lower_right.y()), (60, 150));
    }

    #[test]
    fn positioning_places_the_origin_exactly_or_clamps_it_inside_the_source() {
        let source = SourceSize::new(1_000, 3_000).expect("source should be valid");
        let crop = CropRect::new(100, 200, 400, 600, source).expect("crop should fit");

        let exact = crop.positioned_at(250, 1_500, source);
        let past_edges = crop.positioned_at(i64::MAX, i64::MAX, source);
        let before_edges = crop.positioned_at(i64::MIN, -5, source);

        assert_eq!((exact.x(), exact.y()), (250, 1_500));
        assert_eq!((exact.width(), exact.height()), (400, 600));
        assert_eq!((past_edges.x(), past_edges.y()), (600, 2_400));
        assert_eq!((before_edges.x(), before_edges.y()), (0, 0));
    }

    #[test]
    fn carried_crop_keeps_its_rectangle_when_the_next_source_fits_it() {
        let source = SourceSize::new(1_200, 5_000).expect("source should be valid");

        let crop = CropRect::with_aspect_ratio(300, 1_200, 512, AspectRatio::SQUARE, source);

        assert_eq!(
            (crop.x(), crop.y(), crop.width(), crop.height()),
            (300, 1_200, 512, 512)
        );
    }

    #[test]
    fn carried_crop_slides_inside_a_shorter_source_and_shrinks_for_a_smaller_one() {
        let shorter = SourceSize::new(1_024, 1_000).expect("source should be valid");
        let tiny = SourceSize::new(400, 300).expect("source should be valid");

        let slid = CropRect::with_aspect_ratio(300, 1_200, 512, AspectRatio::SQUARE, shorter);
        let shrunk = CropRect::with_aspect_ratio(300, 1_200, 512, AspectRatio::SQUARE, tiny);

        assert_eq!(
            (slid.x(), slid.y(), slid.width(), slid.height()),
            (300, 488, 512, 512)
        );
        assert_eq!(
            (shrunk.x(), shrunk.y(), shrunk.width(), shrunk.height()),
            (100, 0, 300, 300)
        );
    }

    #[test]
    fn resizing_preserves_center_until_an_edge_requires_clamping() {
        let source = SourceSize::new(200, 200).expect("source should be valid");
        let crop = CropRect::new(75, 75, 50, 50, source).expect("crop should fit");

        let resized = crop.resized_to_width(100, AspectRatio::SQUARE, source);

        assert_eq!((resized.x(), resized.y()), (50, 50));
        assert_eq!((resized.width(), resized.height()), (100, 100));
    }

    #[test]
    fn exact_dimension_resize_preserves_catalog_pixel_size() {
        let source = SourceSize::new(1_024, 1_024).expect("source should be valid");
        let crop = CropRect::new(256, 256, 512, 512, source).expect("crop should fit");

        let resized = crop.resized_to_dimensions(344, 512, source);

        assert_eq!((resized.width(), resized.height()), (344, 512));
        assert_eq!((resized.x(), resized.y()), (340, 256));
    }

    #[test]
    fn square_size_ladder_contains_generation_resolution_tiers() {
        let source = SourceSize::new(2_500, 3_000).expect("source should be valid");

        let sizes = common_crop_sizes(AspectRatio::SQUARE, source);

        for expected in [512, 1_024, 2_048] {
            assert!(sizes.contains(&CropSize::new(expected, expected)));
        }
        assert_eq!(
            sizes.last(),
            Some(&CropSize::new(source.width(), source.width()))
        );
    }

    #[test]
    fn snapped_size_moves_between_adjacent_ratio_presets() {
        let source = SourceSize::new(2_048, 4_096).expect("source should be valid");
        let crop = CropRect::new(640, 1_664, 768, 768, source).expect("crop should fit");

        let smaller = crop.snapped_size(CropResizeDirection::Smaller, AspectRatio::SQUARE, source);
        let larger = crop.snapped_size(CropResizeDirection::Larger, AspectRatio::SQUARE, source);

        assert_eq!((smaller.width(), smaller.height()), (512, 512));
        assert_eq!((larger.width(), larger.height()), (1_024, 1_024));
        assert_eq!(
            (
                smaller.x() + smaller.width() / 2,
                smaller.y() + smaller.height() / 2
            ),
            (1_024, 2_048)
        );
    }

    #[test]
    fn custom_ratio_ladder_uses_aligned_catalog_tiers() {
        let ratio = AspectRatio::new(832, 1_216).expect("ratio should be valid");
        let source = SourceSize::new(2_000, 3_000).expect("source should be valid");

        let sizes = common_crop_sizes(ratio, source);

        assert!(sizes.contains(&CropSize::new(704, 1_024)));
    }

    #[test]
    fn preset_ladder_retains_effective_aligned_dimensions() {
        let source = SourceSize::new(1_024, 1_024).expect("source should be valid");

        let sizes = common_crop_sizes(AspectRatio::PORTRAIT_2_3, source);

        assert!(sizes.contains(&CropSize::new(344, 512)));
    }
}
