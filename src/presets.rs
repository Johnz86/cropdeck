use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::LazyLock;

use serde::Deserialize;
use thiserror::Error;

use crate::crop::{AspectRatio, SourceSize};

const EMBEDDED_CATALOG_JSON: &str = include_str!("../aspect_ratio_catalog.json");
const SUPPORTED_SCHEMA_VERSION: &str = "1.0.0";
const COMMON_RATIO_LABELS: [&str; 6] = ["1:1", "2:3", "3:2", "4:5", "9:16", "16:9"];

static EMBEDDED_CATALOG: LazyLock<Result<AspectRatioCatalog, CatalogError>> =
    LazyLock::new(|| AspectRatioCatalog::from_json(EMBEDDED_CATALOG_JSON));

pub fn embedded_catalog() -> Result<&'static AspectRatioCatalog, CatalogError> {
    EMBEDDED_CATALOG.as_ref().map_err(Clone::clone)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Orientation {
    Portrait,

    Landscape,

    Square,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum SizeTier {
    #[serde(rename = "XS")]
    Xs,

    #[serde(rename = "S")]
    S,

    #[serde(rename = "M")]
    M,

    #[serde(rename = "L")]
    L,

    #[serde(rename = "XL")]
    Xl,

    #[serde(rename = "XXL")]
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
}

impl fmt::Display for SizeTier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TierDefinition {
    id: SizeTier,
    longest_side: u32,
}

impl TierDefinition {
    #[must_use]
    pub const fn id(self) -> SizeTier {
        self.id
    }

    #[must_use]
    pub const fn longest_side(self) -> u32 {
        self.longest_side
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresetDimensions {
    tier: SizeTier,
    width: u32,
    height: u32,
    longest_side: u32,
    effective_ratio: AspectRatio,
}

impl PresetDimensions {
    #[must_use]
    pub fn for_ratio(tier: TierDefinition, ratio: AspectRatio) -> Self {
        let longest_side = tier.longest_side;
        let (width, height) = match ratio.width().cmp(&ratio.height()) {
            std::cmp::Ordering::Less => (
                aligned_derived_side(longest_side, ratio.width(), ratio.height()),
                longest_side,
            ),
            std::cmp::Ordering::Greater => (
                longest_side,
                aligned_derived_side(longest_side, ratio.height(), ratio.width()),
            ),
            std::cmp::Ordering::Equal => (longest_side, longest_side),
        };
        let effective_ratio = AspectRatio::new(width, height)
            .expect("derived preset dimensions must remain non-zero");
        Self {
            tier: tier.id,
            width,
            height,
            longest_side,
            effective_ratio,
        }
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
    pub const fn longest_side(self) -> u32 {
        self.longest_side
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
    presets: Vec<PresetDimensions>,
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

    #[must_use]
    pub fn presets(&self) -> &[PresetDimensions] {
        &self.presets
    }

    #[must_use]
    pub fn preset(&self, tier: SizeTier) -> Option<&PresetDimensions> {
        self.presets.iter().find(|preset| preset.tier == tier)
    }

    pub fn fitting_presets(
        &self,
        source: SourceSize,
    ) -> impl DoubleEndedIterator<Item = &PresetDimensions> {
        self.presets
            .iter()
            .filter(move |preset| preset.fits(source))
    }

    #[must_use]
    pub fn nearest_fitting_preset(
        &self,
        source: SourceSize,
        target_area: u64,
    ) -> Option<&PresetDimensions> {
        self.fitting_presets(source)
            .min_by_key(|preset| preset.area().abs_diff(target_area))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AspectRatioCatalog {
    tiers: Vec<TierDefinition>,
    ratios: Vec<RatioPreset>,
}

impl AspectRatioCatalog {
    pub fn from_json(json: &str) -> Result<Self, CatalogError> {
        let raw: RawCatalog =
            serde_json::from_str(json).map_err(|error| CatalogError::Parse(error.to_string()))?;
        validate_metadata(&raw)?;
        let tiers = validate_tiers(&raw.tiers)?;
        let ratios = validate_ratios(raw.aspect_ratios, raw.presets, &tiers)?;
        Ok(Self { tiers, ratios })
    }

    #[must_use]
    pub fn tiers(&self) -> &[TierDefinition] {
        &self.tiers
    }

    #[must_use]
    pub fn ratios(&self) -> &[RatioPreset] {
        &self.ratios
    }

    pub fn ratios_for(&self, orientation: Orientation) -> impl Iterator<Item = &RatioPreset> {
        self.ratios
            .iter()
            .filter(move |preset| preset.orientation == orientation)
    }

    pub fn common_ratios(&self) -> impl Iterator<Item = &RatioPreset> {
        COMMON_RATIO_LABELS
            .into_iter()
            .filter_map(|label| self.ratio_by_label(label))
    }

    #[must_use]
    pub fn ratio(&self, ratio: AspectRatio) -> Option<&RatioPreset> {
        self.ratios.iter().find(|preset| preset.ratio == ratio)
    }

    #[must_use]
    pub fn ratio_by_label(&self, label: &str) -> Option<&RatioPreset> {
        self.ratios.iter().find(|preset| preset.label == label)
    }

    #[must_use]
    pub fn dimensions_for(&self, ratio: AspectRatio, tier: SizeTier) -> Option<PresetDimensions> {
        if let Some(dimensions) = self.ratio(ratio).and_then(|preset| preset.preset(tier)) {
            return Some(*dimensions);
        }
        self.tier(tier)
            .copied()
            .map(|definition| PresetDimensions::for_ratio(definition, ratio))
    }

    #[must_use]
    pub fn fitting_dimensions(
        &self,
        ratio: AspectRatio,
        source: SourceSize,
    ) -> Vec<PresetDimensions> {
        self.tiers
            .iter()
            .filter_map(|tier| self.dimensions_for(ratio, tier.id))
            .filter(|dimensions| dimensions.fits(source))
            .collect()
    }

    #[must_use]
    pub fn nearest_fitting_dimensions(
        &self,
        ratio: AspectRatio,
        source: SourceSize,
        target_area: u64,
    ) -> Option<PresetDimensions> {
        self.fitting_dimensions(ratio, source)
            .into_iter()
            .min_by_key(|dimensions| dimensions.area().abs_diff(target_area))
    }

    fn tier(&self, id: SizeTier) -> Option<&TierDefinition> {
        self.tiers.iter().find(|tier| tier.id == id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CatalogError {
    #[error("could not parse aspect-ratio catalog: {0}")]
    Parse(String),

    #[error("invalid aspect-ratio catalog: {0}")]
    Invalid(String),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCatalog {
    schema_version: String,
    generated_at_utc: String,
    notes: Vec<String>,
    tiers: Vec<RawTier>,
    aspect_ratios: Vec<String>,
    presets: HashMap<String, HashMap<SizeTier, RawDimensions>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTier {
    tier: SizeTier,
    longest_side_px: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDimensions {
    width: u32,
    height: u32,
    longest_side_px: u32,
}

fn validate_metadata(raw: &RawCatalog) -> Result<(), CatalogError> {
    if raw.schema_version != SUPPORTED_SCHEMA_VERSION {
        return invalid(format!(
            "unsupported schema version {}, expected {SUPPORTED_SCHEMA_VERSION}",
            raw.schema_version
        ));
    }
    if raw.generated_at_utc.is_empty() {
        return invalid(String::from("generated_at_utc must not be empty"));
    }
    if raw.notes.is_empty() || raw.notes.iter().any(String::is_empty) {
        return invalid(String::from("notes must contain only non-empty entries"));
    }
    Ok(())
}

fn validate_tiers(raw_tiers: &[RawTier]) -> Result<Vec<TierDefinition>, CatalogError> {
    if raw_tiers.len() != SizeTier::ALL.len() {
        return invalid(format!(
            "expected {} tiers, received {}",
            SizeTier::ALL.len(),
            raw_tiers.len()
        ));
    }

    raw_tiers.iter().zip(SizeTier::ALL).try_fold(
        Vec::with_capacity(raw_tiers.len()),
        |mut tiers, (raw, expected)| {
            if raw.tier != expected {
                return invalid(format!(
                    "tier {} is out of order; expected {}",
                    raw.tier, expected
                ));
            }
            if raw.longest_side_px == 0 || !raw.longest_side_px.is_multiple_of(8) {
                return invalid(format!(
                    "tier {} longest side must be a non-zero multiple of 8",
                    raw.tier
                ));
            }
            if tiers.last().is_some_and(|previous: &TierDefinition| {
                previous.longest_side >= raw.longest_side_px
            }) {
                return invalid(String::from("tier longest sides must increase"));
            }
            tiers.push(TierDefinition {
                id: raw.tier,
                longest_side: raw.longest_side_px,
            });
            Ok(tiers)
        },
    )
}

fn validate_ratios(
    ratio_labels: Vec<String>,
    mut raw_presets: HashMap<String, HashMap<SizeTier, RawDimensions>>,
    tiers: &[TierDefinition],
) -> Result<Vec<RatioPreset>, CatalogError> {
    if ratio_labels.is_empty() {
        return invalid(String::from("aspect_ratios must not be empty"));
    }

    let mut seen = HashSet::with_capacity(ratio_labels.len());
    let mut ratios = Vec::with_capacity(ratio_labels.len());
    for label in ratio_labels {
        let ratio = parse_ratio(&label)?;
        if !seen.insert(ratio) {
            return invalid(format!("duplicate ratio {label}"));
        }
        let dimensions = raw_presets
            .remove(&label)
            .ok_or_else(|| CatalogError::Invalid(format!("missing presets for ratio {label}")))?;
        ratios.push(build_ratio_preset(label, ratio, dimensions, tiers)?);
    }
    if let Some(unknown) = raw_presets.keys().next() {
        return invalid(format!("presets reference unknown ratio {unknown}"));
    }
    Ok(ratios)
}

fn build_ratio_preset(
    label: String,
    ratio: AspectRatio,
    mut dimensions: HashMap<SizeTier, RawDimensions>,
    tiers: &[TierDefinition],
) -> Result<RatioPreset, CatalogError> {
    let mut presets = Vec::with_capacity(tiers.len());
    for tier in tiers {
        let raw = dimensions.remove(&tier.id).ok_or_else(|| {
            CatalogError::Invalid(format!("ratio {label} is missing tier {}", tier.id))
        })?;
        presets.push(validate_dimensions(&label, ratio, *tier, raw)?);
    }
    if let Some(unknown) = dimensions.keys().next() {
        return invalid(format!("ratio {label} contains unknown tier {unknown}"));
    }
    let orientation = match ratio.width().cmp(&ratio.height()) {
        std::cmp::Ordering::Less => Orientation::Portrait,
        std::cmp::Ordering::Greater => Orientation::Landscape,
        std::cmp::Ordering::Equal => Orientation::Square,
    };
    Ok(RatioPreset {
        label,
        ratio,
        orientation,
        presets,
    })
}

fn validate_dimensions(
    label: &str,
    nominal_ratio: AspectRatio,
    tier: TierDefinition,
    raw: RawDimensions,
) -> Result<PresetDimensions, CatalogError> {
    if raw.width == 0 || raw.height == 0 {
        return invalid(format!("ratio {label} tier {} must be non-zero", tier.id));
    }
    if !raw.width.is_multiple_of(8) || !raw.height.is_multiple_of(8) {
        return invalid(format!(
            "ratio {label} tier {} dimensions must be multiples of 8",
            tier.id
        ));
    }
    if raw.longest_side_px != tier.longest_side || raw.width.max(raw.height) != tier.longest_side {
        return invalid(format!(
            "ratio {label} tier {} exceeds or disagrees with its longest-side bound",
            tier.id
        ));
    }
    if !is_close_to_nominal(raw.width, raw.height, nominal_ratio) {
        return invalid(format!(
            "ratio {label} tier {} dimensions differ excessively from the nominal ratio",
            tier.id
        ));
    }
    let effective_ratio = AspectRatio::new(raw.width, raw.height).map_err(|error| {
        CatalogError::Invalid(format!(
            "ratio {label} tier {} is invalid: {error}",
            tier.id
        ))
    })?;
    Ok(PresetDimensions {
        tier: tier.id,
        width: raw.width,
        height: raw.height,
        longest_side: raw.longest_side_px,
        effective_ratio,
    })
}

fn parse_ratio(label: &str) -> Result<AspectRatio, CatalogError> {
    let (width, height) = label
        .split_once(':')
        .ok_or_else(|| CatalogError::Invalid(format!("invalid ratio label {label}")))?;
    if width.is_empty() || height.is_empty() || height.contains(':') {
        return invalid(format!("invalid ratio label {label}"));
    }
    let width = width
        .parse::<u32>()
        .map_err(|_| CatalogError::Invalid(format!("invalid ratio label {label}")))?;
    let height = height
        .parse::<u32>()
        .map_err(|_| CatalogError::Invalid(format!("invalid ratio label {label}")))?;
    AspectRatio::new(width, height)
        .map_err(|error| CatalogError::Invalid(format!("invalid ratio label {label}: {error}")))
}

fn is_close_to_nominal(width: u32, height: u32, ratio: AspectRatio) -> bool {
    let left = i128::from(width) * i128::from(ratio.height());
    let right = i128::from(height) * i128::from(ratio.width());
    let tolerance = i128::from(4 * ratio.width().max(ratio.height()));
    (left - right).abs() <= tolerance
}

fn aligned_derived_side(longest_side: u32, numerator: u32, denominator: u32) -> u32 {
    let scaled = u64::from(longest_side) * u64::from(numerator);
    let alignment_denominator = u64::from(denominator) * 8;
    let alignment_units = (scaled + u64::from(denominator) * 4) / alignment_denominator;
    let aligned = alignment_units.saturating_mul(8).max(8);
    u32::try_from(aligned).unwrap_or(u32::MAX).min(longest_side)
}

fn invalid<T>(reason: String) -> Result<T, CatalogError> {
    Err(CatalogError::Invalid(reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(width: u32, height: u32) -> SourceSize {
        SourceSize::new(width, height).expect("test source should be non-empty")
    }

    fn changed_catalog(change: impl FnOnce(&mut serde_json::Value)) -> String {
        let mut value: serde_json::Value =
            serde_json::from_str(EMBEDDED_CATALOG_JSON).expect("embedded JSON should parse");
        change(&mut value);
        serde_json::to_string(&value).expect("changed catalog should serialize")
    }

    #[test]
    fn embedded_catalog_is_valid_and_complete() {
        let catalog = embedded_catalog().expect("embedded catalog should be valid");

        assert_eq!(catalog.tiers().len(), SizeTier::ALL.len());
        assert_eq!(catalog.ratios().len(), 29);
        assert_eq!(catalog.ratios_for(Orientation::Portrait).count(), 14);
        assert_eq!(catalog.ratios_for(Orientation::Landscape).count(), 14);
        assert_eq!(catalog.ratios_for(Orientation::Square).count(), 1);
    }

    #[test]
    fn tiers_expose_ordered_labels_and_bounds() {
        let catalog = embedded_catalog().expect("embedded catalog should be valid");

        let observed = catalog
            .tiers()
            .iter()
            .map(|tier| (tier.id().to_string(), tier.longest_side()))
            .collect::<Vec<_>>();

        assert_eq!(observed.first(), Some(&(String::from("XS"), 512)));
        assert_eq!(observed.last(), Some(&(String::from("XXL"), 2_560)));
    }

    #[test]
    fn common_ratios_use_recommended_toolbar_order() {
        let catalog = embedded_catalog().expect("embedded catalog should be valid");

        let labels = catalog
            .common_ratios()
            .map(RatioPreset::label)
            .collect::<Vec<_>>();

        assert_eq!(labels, COMMON_RATIO_LABELS);
    }

    #[test]
    fn ratio_lookup_preserves_nominal_label_and_effective_dimensions() {
        let catalog = embedded_catalog().expect("embedded catalog should be valid");
        let nominal = AspectRatio::new(2, 3).expect("ratio should be valid");

        let ratio = catalog.ratio(nominal).expect("2:3 should exist");
        let dimensions = ratio.preset(SizeTier::Xs).expect("XS should exist");

        assert_eq!(ratio.label(), "2:3");
        assert_eq!(ratio.ratio(), nominal);
        assert_eq!(ratio.orientation(), Orientation::Portrait);
        assert_eq!((dimensions.width(), dimensions.height()), (344, 512));
        assert_eq!(dimensions.tier(), SizeTier::Xs);
        assert_eq!(dimensions.longest_side(), 512);
        assert_eq!(dimensions.effective_ratio().to_string(), "43:64");
        assert!((dimensions.megapixels() - 0.176_128).abs() < f64::EPSILON);
    }

    #[test]
    fn source_filter_retains_only_fitting_presets() {
        let catalog = embedded_catalog().expect("embedded catalog should be valid");
        let ratio = catalog.ratio_by_label("9:16").expect("9:16 should exist");

        let fitting = ratio
            .fitting_presets(source(1_000, 1_600))
            .map(|preset| preset.tier())
            .collect::<Vec<_>>();

        assert_eq!(
            fitting,
            [SizeTier::Xs, SizeTier::S, SizeTier::M, SizeTier::L]
        );
        assert!(ratio.presets()[0].fits(source(288, 512)));
        assert!(!ratio.presets()[0].fits(source(287, 512)));
    }

    #[test]
    fn custom_ratios_derive_aligned_dimensions_for_every_tier() {
        let catalog = embedded_catalog().expect("embedded catalog should be valid");
        let ratio = AspectRatio::new(17, 23).expect("ratio should be valid");

        let medium = catalog
            .dimensions_for(ratio, SizeTier::M)
            .expect("tier should exist");
        let all_fitting = catalog.fitting_dimensions(ratio, source(1_000, 1_500));

        assert_eq!((medium.width(), medium.height()), (760, 1_024));
        assert_eq!(medium.area(), 778_240);
        assert_eq!(all_fitting.len(), 3);
        assert!(all_fitting.iter().all(|dimensions| {
            dimensions.width().is_multiple_of(8) && dimensions.height().is_multiple_of(8)
        }));
    }

    #[test]
    fn nearest_fitting_dimensions_tracks_crop_area() {
        let catalog = embedded_catalog().expect("embedded catalog should be valid");
        let ratio = catalog.ratio_by_label("1:1").expect("square should exist");
        let target_area = 900_u64 * 900;

        let ratio_nearest = ratio
            .nearest_fitting_preset(source(2_000, 2_000), target_area)
            .expect("a tier should fit");
        let catalog_nearest = catalog
            .nearest_fitting_dimensions(ratio.ratio(), source(2_000, 2_000), target_area)
            .expect("a tier should fit");

        assert_eq!(ratio_nearest.tier(), SizeTier::S);
        assert_eq!(catalog_nearest, *ratio_nearest);
        assert!(
            catalog
                .nearest_fitting_dimensions(ratio.ratio(), source(100, 100), target_area)
                .is_none()
        );
    }

    #[test]
    fn parser_rejects_invalid_json_and_schema() {
        assert!(matches!(
            AspectRatioCatalog::from_json("{"),
            Err(CatalogError::Parse(_))
        ));
        let changed = changed_catalog(|value| value["schema_version"] = "2.0.0".into());

        assert!(matches!(
            AspectRatioCatalog::from_json(&changed),
            Err(CatalogError::Invalid(_))
        ));
    }

    #[test]
    fn parser_rejects_bad_tier_order_and_dimensions() {
        let reordered = changed_catalog(|value| {
            value["tiers"]
                .as_array_mut()
                .expect("tiers should be an array")
                .swap(0, 1);
        });
        let unaligned =
            changed_catalog(|value| value["presets"]["1:1"]["XS"]["width"] = 510.into());

        assert!(AspectRatioCatalog::from_json(&reordered).is_err());
        assert!(AspectRatioCatalog::from_json(&unaligned).is_err());
    }

    #[test]
    fn parser_rejects_missing_and_unknown_ratio_references() {
        let missing = changed_catalog(|value| {
            value["presets"]
                .as_object_mut()
                .expect("presets should be an object")
                .remove("1:3");
        });
        let unknown = changed_catalog(|value| {
            let presets = value["presets"]
                .as_object_mut()
                .expect("presets should be an object");
            let copy = presets["1:1"].clone();
            presets.insert(String::from("8:9"), copy);
        });

        assert!(AspectRatioCatalog::from_json(&missing).is_err());
        assert!(AspectRatioCatalog::from_json(&unknown).is_err());
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
