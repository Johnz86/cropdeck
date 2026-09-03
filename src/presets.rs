use std::fmt;
use std::sync::LazyLock;

use crate::crop::{AspectRatio, SourceSize};

const CATALOG_RATIOS: [(u32, u32); 29] = [
    (1, 3),
    (1, 2),
    (9, 21),
    (9, 19),
    (4, 7),
    (2, 3),
    (9, 16),
    (5, 8),
    (13, 19),
    (3, 4),
    (7, 9),
    (4, 5),
    (5, 6),
    (10, 11),
    (3, 1),
    (2, 1),
    (21, 9),
    (19, 9),
    (7, 4),
    (3, 2),
    (16, 9),
    (8, 5),
    (19, 13),
    (4, 3),
    (9, 7),
    (5, 4),
    (6, 5),
    (11, 10),
    (1, 1),
];

static CATALOG: LazyLock<AspectRatioCatalog> = LazyLock::new(AspectRatioCatalog::built_in);

#[must_use]
pub fn catalog() -> &'static AspectRatioCatalog {
    &CATALOG
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Orientation {
    Portrait,
    Landscape,
    Square,
}

impl Orientation {
    #[must_use]
    pub fn of(ratio: AspectRatio) -> Self {
        match ratio.width().cmp(&ratio.height()) {
            std::cmp::Ordering::Less => Self::Portrait,
            std::cmp::Ordering::Greater => Self::Landscape,
            std::cmp::Ordering::Equal => Self::Square,
        }
    }
}

impl fmt::Display for Orientation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Portrait => formatter.write_str("Portrait"),
            Self::Landscape => formatter.write_str("Landscape"),
            Self::Square => formatter.write_str("Square"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SizeTier {
    Xs,
    S,
    M,
    L,
    Xl,
    Xxl,
}

impl SizeTier {
    pub const ALL: [Self; 6] = [Self::Xs, Self::S, Self::M, Self::L, Self::Xl, Self::Xxl];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Xs => "XS",
            Self::S => "S",
            Self::M => "M",
            Self::L => "L",
            Self::Xl => "XL",
            Self::Xxl => "XXL",
        }
    }

    #[must_use]
    pub const fn longest_side(self) -> u32 {
        match self {
            Self::Xs => 512,
            Self::S => 768,
            Self::M => 1_024,
            Self::L => 1_536,
            Self::Xl => 2_048,
            Self::Xxl => 2_560,
        }
    }
}

impl fmt::Display for SizeTier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresetDimensions {
    tier: SizeTier,
    width: u32,
    height: u32,
    effective_ratio: AspectRatio,
}

impl PresetDimensions {
    #[must_use]
    pub fn for_ratio(tier: SizeTier, ratio: AspectRatio) -> Self {
        let longest_side = tier.longest_side();
        let (width, height) = match Orientation::of(ratio) {
            Orientation::Portrait => (
                aligned_derived_side(longest_side, ratio.width(), ratio.height()),
                longest_side,
            ),
            Orientation::Landscape => (
                longest_side,
                aligned_derived_side(longest_side, ratio.height(), ratio.width()),
            ),
            Orientation::Square => (longest_side, longest_side),
        };
        let effective_ratio = AspectRatio::new(width, height)
            .expect("derived preset dimensions must remain non-zero");
        Self {
            tier,
            width,
            height,
            effective_ratio,
        }
    }

    pub fn fitting(
        ratio: AspectRatio,
        source: SourceSize,
    ) -> impl DoubleEndedIterator<Item = Self> {
        SizeTier::ALL
            .into_iter()
            .map(move |tier| Self::for_ratio(tier, ratio))
            .filter(move |dimensions| dimensions.fits(source))
    }

    #[must_use]
    pub fn nearest_fitting(
        ratio: AspectRatio,
        source: SourceSize,
        target_area: u64,
    ) -> Option<Self> {
        Self::fitting(ratio, source)
            .min_by_key(|dimensions| dimensions.area().abs_diff(target_area))
    }

    #[must_use]
    pub const fn tier(self) -> SizeTier {
        self.tier
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
    pub const fn effective_ratio(self) -> AspectRatio {
        self.effective_ratio
    }

    #[must_use]
    pub fn megapixels(self) -> f64 {
        f64::from(self.width) * f64::from(self.height) / 1_000_000.0
    }

    #[must_use]
    pub const fn area(self) -> u64 {
        self.width as u64 * self.height as u64
    }

    #[must_use]
    pub const fn fits(self, source: SourceSize) -> bool {
        self.width <= source.width() && self.height <= source.height()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RatioPreset {
    label: String,
    ratio: AspectRatio,
    orientation: Orientation,
}

impl RatioPreset {
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    #[must_use]
    pub const fn ratio(&self) -> AspectRatio {
        self.ratio
    }

    #[must_use]
    pub const fn orientation(&self) -> Orientation {
        self.orientation
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AspectRatioCatalog {
    ratios: Vec<RatioPreset>,
}

impl AspectRatioCatalog {
    fn built_in() -> Self {
        let ratios = CATALOG_RATIOS
            .into_iter()
            .map(|(width, height)| {
                let ratio = AspectRatio::new(width, height).expect("catalog ratios are non-zero");
                RatioPreset {
                    label: ratio.to_string(),
                    ratio,
                    orientation: Orientation::of(ratio),
                }
            })
            .collect();
        Self { ratios }
    }

    pub fn ratios_for(&self, orientation: Orientation) -> impl Iterator<Item = &RatioPreset> {
        self.ratios
            .iter()
            .filter(move |preset| preset.orientation == orientation)
    }
}

fn aligned_derived_side(longest_side: u32, numerator: u32, denominator: u32) -> u32 {
    let scaled = u64::from(longest_side) * u64::from(numerator);
    let alignment_denominator = u64::from(denominator) * 8;
    let alignment_units = (scaled + u64::from(denominator) * 4) / alignment_denominator;
    let aligned = alignment_units.saturating_mul(8).max(8);
    u32::try_from(aligned).unwrap_or(u32::MAX).min(longest_side)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(width: u32, height: u32) -> SourceSize {
        SourceSize::new(width, height).expect("test source should be non-empty")
    }

    fn ratio(width: u32, height: u32) -> AspectRatio {
        AspectRatio::new(width, height).expect("ratio should be valid")
    }

    #[test]
    fn built_in_catalog_groups_every_ratio_by_orientation() {
        let catalog = catalog();

        assert_eq!(catalog.ratios_for(Orientation::Portrait).count(), 14);
        assert_eq!(catalog.ratios_for(Orientation::Landscape).count(), 14);
        assert_eq!(catalog.ratios_for(Orientation::Square).count(), 1);
        let first = catalog
            .ratios_for(Orientation::Portrait)
            .next()
            .expect("portrait ratios exist");
        assert_eq!(first.label(), "1:3");
        assert_eq!(first.ratio(), ratio(1, 3));
        assert_eq!(first.orientation(), Orientation::Portrait);
    }

    #[test]
    fn tiers_fix_the_longest_side_and_align_the_other_to_eight() {
        assert_eq!(SizeTier::Xs.longest_side(), 512);
        assert_eq!(SizeTier::Xxl.longest_side(), 2_560);

        let portrait = PresetDimensions::for_ratio(SizeTier::Xs, ratio(2, 3));
        assert_eq!((portrait.width(), portrait.height()), (344, 512));
        assert_eq!(portrait.tier(), SizeTier::Xs);
        assert_eq!(portrait.effective_ratio().to_string(), "43:64");
        assert!((portrait.megapixels() - 0.176_128).abs() < f64::EPSILON);

        let landscape = PresetDimensions::for_ratio(SizeTier::M, ratio(16, 9));
        assert_eq!((landscape.width(), landscape.height()), (1_024, 576));

        let custom = PresetDimensions::for_ratio(SizeTier::M, ratio(17, 23));
        assert_eq!((custom.width(), custom.height()), (760, 1_024));
        assert_eq!(custom.area(), 778_240);
    }

    #[test]
    fn fitting_dimensions_respect_the_source_bounds() {
        let fitting = PresetDimensions::fitting(ratio(9, 16), source(1_000, 1_600))
            .map(PresetDimensions::tier)
            .collect::<Vec<_>>();

        assert_eq!(
            fitting,
            [SizeTier::Xs, SizeTier::S, SizeTier::M, SizeTier::L]
        );
        let smallest = PresetDimensions::for_ratio(SizeTier::Xs, ratio(9, 16));
        assert!(smallest.fits(source(288, 512)));
        assert!(!smallest.fits(source(287, 512)));
        assert_eq!(
            PresetDimensions::fitting(ratio(17, 23), source(1_000, 1_500)).count(),
            3
        );
    }

    #[test]
    fn nearest_fitting_dimensions_track_the_target_area() {
        let square = ratio(1, 1);
        let target_area = 900_u64 * 900;

        let nearest = PresetDimensions::nearest_fitting(square, source(2_000, 2_000), target_area)
            .expect("a tier should fit");

        assert_eq!(nearest.tier(), SizeTier::S);
        assert!(PresetDimensions::nearest_fitting(square, source(100, 100), target_area).is_none());
    }

    #[test]
    fn orientation_and_tier_display_are_human_readable() {
        assert_eq!(Orientation::Portrait.to_string(), "Portrait");
        assert_eq!(Orientation::Landscape.to_string(), "Landscape");
        assert_eq!(Orientation::Square.to_string(), "Square");
        assert_eq!(SizeTier::Xl.label(), "XL");
        assert_eq!(SizeTier::Xxl.to_string(), "XXL");
    }
}
