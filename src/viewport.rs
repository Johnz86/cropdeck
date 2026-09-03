use std::ops::{Range, RangeInclusive};

use eframe::egui::{Color32, ColorImage, Context, Pos2, Rect, TextureHandle, TextureOptions, Vec2};
use thiserror::Error;

use crate::image_io::{ImageDimensions, SourceImage, SourcePixels};

const MAXIMUM_TILE_ROWS: u32 = 2_048;
const UPLOADS_PER_FRAME: usize = 2;
const RETAINED_TILE_MARGIN: usize = 2;
const HALF_RESOLUTION_FACTOR: u32 = 2;
const HALF_RESOLUTION_MAXIMUM_SCALE: f32 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SourcePoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SourceRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportTransform {
    image_origin: Pos2,
    scale: f32,
}

pub struct TiledTexture {
    source: SourceImage,
    full: TileLevel,
    half: Option<TileLevel>,
}

struct TileLevel {
    label: String,
    source_width: f32,
    source_height: u32,
    tiles: Vec<TextureTile>,
}

struct TextureTile {
    source_rows: Range<u32>,
    pixel_rows: Range<u32>,
    texture: Option<TextureHandle>,
}

trait TileRows {
    fn source_rows(&self) -> &Range<u32>;
}

impl TileRows for Range<u32> {
    fn source_rows(&self) -> &Range<u32> {
        self
    }
}

impl TileRows for TextureTile {
    fn source_rows(&self) -> &Range<u32> {
        &self.source_rows
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TiledTextureError {
    #[error("image dimensions must be greater than zero")]
    EmptyImage,
    #[error("image dimensions are too large for this platform")]
    DimensionsTooLarge,
}

impl TiledTexture {
    pub fn new(source: SourceImage, label: &str) -> Result<Self, TiledTextureError> {
        let dimensions = source.dimensions();
        let (width, height) = (dimensions.width(), dimensions.height());
        if width == 0 || height == 0 {
            return Err(TiledTextureError::EmptyImage);
        }
        usize::try_from(width).map_err(|_| TiledTextureError::DimensionsTooLarge)?;
        usize::try_from(height.div_ceil(MAXIMUM_TILE_ROWS))
            .map_err(|_| TiledTextureError::DimensionsTooLarge)?;

        let full = TileLevel::new(format!("{label}:full"), source.pixels(), dimensions, 1);
        let half = source.half_pixels().map(|pixels| {
            TileLevel::new(
                format!("{label}:half"),
                pixels,
                dimensions,
                HALF_RESOLUTION_FACTOR,
            )
        });
        Ok(Self { source, full, half })
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.source.dimensions().width()
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.source.dimensions().height()
    }

    pub fn paint(
        &mut self,
        context: &Context,
        painter: &eframe::egui::Painter,
        transform: ViewportTransform,
    ) {
        let half = self
            .half
            .as_mut()
            .zip(self.source.half_pixels())
            .filter(|_level| transform.scale() <= HALF_RESOLUTION_MAXIMUM_SCALE);
        if let Some((half, pixels)) = half {
            self.full.release_all();
            half.paint(pixels, context, painter, transform);
            return;
        }
        if let Some(half) = self.half.as_mut() {
            half.release_all();
        }
        self.full
            .paint(self.source.pixels(), context, painter, transform);
    }
}

impl TileLevel {
    fn new(
        label: String,
        pixels: &SourcePixels,
        source: ImageDimensions,
        source_rows_per_pixel_row: u32,
    ) -> Self {
        let pixel_ranges = tile_row_ranges(pixels.height(), MAXIMUM_TILE_ROWS);
        let last_index = pixel_ranges.len().saturating_sub(1);
        let tiles = pixel_ranges
            .into_iter()
            .enumerate()
            .map(|(index, pixel_rows)| {
                let source_end = if index == last_index {
                    source.height()
                } else {
                    pixel_rows.end * source_rows_per_pixel_row
                };
                TextureTile {
                    source_rows: pixel_rows.start * source_rows_per_pixel_row..source_end,
                    pixel_rows,
                    texture: None,
                }
            })
            .collect();
        Self {
            label,
            source_width: source.width() as f32,
            source_height: source.height(),
            tiles,
        }
    }

    fn release_all(&mut self) {
        self.tiles.iter_mut().for_each(|tile| tile.texture = None);
    }

    fn paint(
        &mut self,
        pixels: &SourcePixels,
        context: &Context,
        painter: &eframe::egui::Painter,
        transform: ViewportTransform,
    ) {
        let clip_rect = painter.clip_rect();
        let first_row = transform
            .display_to_source(Pos2::new(clip_rect.left(), clip_rect.top()))
            .y
            .floor()
            .clamp(0.0, self.source_height as f32) as u32;
        let last_row = transform
            .display_to_source(Pos2::new(clip_rect.left(), clip_rect.bottom()))
            .y
            .ceil()
            .clamp(0.0, self.source_height as f32) as u32;
        let Some(visible) = visible_tile_indices(&self.tiles, first_row..last_row) else {
            return;
        };
        let visible_start = *visible.start();
        let visible_end = *visible.end();
        let wanted_start = visible_start.saturating_sub(1);
        let wanted_end = visible_end.saturating_add(1).min(self.tiles.len() - 1);

        let before = visible_start.checked_sub(1);
        let after = (visible_end + 1 < self.tiles.len()).then_some(visible_end + 1);
        let upload_order = visible.clone().chain(before).chain(after);
        let mut uploads = 0;
        for index in upload_order {
            if uploads == UPLOADS_PER_FRAME {
                break;
            }
            if self.tiles[index].texture.is_some() {
                continue;
            }
            let image = tile_color_image(pixels, self.tiles[index].pixel_rows.clone());
            let texture = context.load_texture(
                format!("{}:{index}", self.label),
                image,
                TextureOptions::LINEAR,
            );
            self.tiles[index].texture = Some(texture);
            uploads += 1;
        }
        if self.tiles[wanted_start..=wanted_end]
            .iter()
            .any(|tile| tile.texture.is_none())
        {
            context.request_repaint();
        }

        let retained_start = visible_start.saturating_sub(RETAINED_TILE_MARGIN);
        let retained_end = visible_end
            .saturating_add(RETAINED_TILE_MARGIN)
            .min(self.tiles.len() - 1);
        self.tiles
            .iter_mut()
            .enumerate()
            .filter(|(index, _tile)| *index < retained_start || *index > retained_end)
            .map(|(_index, tile)| tile)
            .for_each(|tile| tile.texture = None);

        for index in visible {
            let tile = &self.tiles[index];
            let Some(texture) = tile.texture.as_ref() else {
                continue;
            };
            let tile_rect = transform.source_rect_to_display(SourceRect {
                x: 0.0,
                y: tile.source_rows.start as f32,
                width: self.source_width,
                height: tile.source_rows.len() as f32,
            });
            painter.image(
                texture.id(),
                tile_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }
}

fn tile_color_image(pixels: &SourcePixels, rows: Range<u32>) -> ColorImage {
    let width = pixels.width() as usize;
    let size = [width, rows.len()];
    match pixels {
        SourcePixels::Rgb(_) => ColorImage::from_rgb(size, pixels.rows(rows)),
        SourcePixels::Rgba(_) => ColorImage::from_rgba_unmultiplied(size, pixels.rows(rows)),
    }
}

fn tile_row_ranges(height: u32, maximum_rows: u32) -> Vec<Range<u32>> {
    if height == 0 || maximum_rows == 0 {
        return Vec::new();
    }

    let tile_count = height.div_ceil(maximum_rows);
    let mut ranges = Vec::with_capacity(tile_count as usize);
    let mut first_row = 0;
    while first_row < height {
        let end = first_row.saturating_add(maximum_rows).min(height);
        ranges.push(first_row..end);
        first_row = end;
    }
    ranges
}

fn visible_tile_indices<T: TileRows>(
    tiles: &[T],
    visible_rows: Range<u32>,
) -> Option<RangeInclusive<usize>> {
    if visible_rows.is_empty() {
        return None;
    }
    let first = tiles.partition_point(|tile| tile.source_rows().end <= visible_rows.start);
    let end = tiles.partition_point(|tile| tile.source_rows().start < visible_rows.end);
    (first < end).then_some(first..=end - 1)
}

impl ViewportTransform {
    #[must_use]
    pub fn new(image_origin: Pos2, scale: f32) -> Option<Self> {
        (scale.is_finite() && scale > 0.0).then_some(Self {
            image_origin,
            scale,
        })
    }

    #[must_use]
    pub fn fit_width(source_width: u32, available_width: f32) -> Option<f32> {
        (source_width > 0 && available_width.is_finite() && available_width > 0.0)
            .then_some((available_width / source_width as f32).min(1.0))
    }

    #[must_use]
    pub fn scale(self) -> f32 {
        self.scale
    }

    #[must_use]
    pub fn source_to_display(self, point: SourcePoint) -> Pos2 {
        self.image_origin + Vec2::new(point.x * self.scale, point.y * self.scale)
    }

    #[must_use]
    pub fn display_to_source(self, point: Pos2) -> SourcePoint {
        let offset = point - self.image_origin;
        SourcePoint {
            x: offset.x / self.scale,
            y: offset.y / self.scale,
        }
    }

    #[must_use]
    pub fn display_delta_to_source(self, delta: Vec2) -> Vec2 {
        delta / self.scale
    }

    #[must_use]
    pub fn source_rect_to_display(self, rect: SourceRect) -> Rect {
        let minimum = self.source_to_display(SourcePoint {
            x: rect.x,
            y: rect.y,
        });
        Rect::from_min_size(
            minimum,
            Vec2::new(rect.width * self.scale, rect.height * self.scale),
        )
    }
}

#[must_use]
pub fn edge_auto_scroll_velocity(
    pointer_y: f32,
    viewport: Rect,
    activation_distance: f32,
    maximum_velocity: f32,
) -> f32 {
    if !pointer_y.is_finite()
        || !activation_distance.is_finite()
        || activation_distance <= 0.0
        || !maximum_velocity.is_finite()
        || maximum_velocity <= 0.0
    {
        return 0.0;
    }

    let edge_distance = activation_distance.min(viewport.height() * 0.5);
    if edge_distance <= 0.0 {
        return 0.0;
    }

    let top_strength =
        ((viewport.top() + edge_distance - pointer_y) / edge_distance).clamp(0.0, 1.0);
    if top_strength > 0.0 {
        return -maximum_velocity * top_strength * top_strength;
    }

    let bottom_strength =
        ((pointer_y - (viewport.bottom() - edge_distance)) / edge_distance).clamp(0.0, 1.0);
    maximum_velocity * bottom_strength * bottom_strength
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transform() -> ViewportTransform {
        ViewportTransform::new(Pos2::new(10.0, 20.0), 0.5)
            .expect("the test transform scale is valid")
    }

    #[test]
    fn fit_width_rejects_invalid_dimensions() {
        assert_eq!(ViewportTransform::fit_width(0, 800.0), None);
        assert_eq!(ViewportTransform::fit_width(800, 0.0), None);
        assert_eq!(ViewportTransform::fit_width(800, f32::NAN), None);
    }

    #[test]
    fn fit_width_returns_display_points_per_source_pixel() {
        assert_eq!(ViewportTransform::fit_width(1_600, 800.0), Some(0.5));
    }

    #[test]
    fn fit_width_does_not_enlarge_a_smaller_source() {
        assert_eq!(ViewportTransform::fit_width(640, 1_280.0), Some(1.0));
    }

    #[test]
    fn source_and_display_positions_round_trip() {
        let source = SourcePoint { x: 123.0, y: 456.0 };
        let display = transform().source_to_display(source);

        assert_eq!(display, Pos2::new(71.5, 248.0));
        assert_eq!(transform().display_to_source(display), source);
    }

    #[test]
    fn source_rect_maps_size_and_origin() {
        let rect = transform().source_rect_to_display(SourceRect {
            x: 20.0,
            y: 40.0,
            width: 200.0,
            height: 300.0,
        });

        assert_eq!(rect.min, Pos2::new(20.0, 40.0));
        assert_eq!(rect.size(), Vec2::new(100.0, 150.0));
    }

    #[test]
    fn display_delta_maps_to_source_pixels() {
        assert_eq!(
            transform().display_delta_to_source(Vec2::new(5.0, -10.0)),
            Vec2::new(10.0, -20.0)
        );
    }

    #[test]
    fn edge_auto_scroll_is_zero_in_center() {
        let viewport = Rect::from_min_max(Pos2::ZERO, Pos2::new(800.0, 600.0));

        assert_eq!(edge_auto_scroll_velocity(300.0, viewport, 80.0, 900.0), 0.0);
    }

    #[test]
    fn edge_auto_scroll_is_quadratic_and_directional() {
        let viewport = Rect::from_min_max(Pos2::ZERO, Pos2::new(800.0, 600.0));

        assert_eq!(
            edge_auto_scroll_velocity(0.0, viewport, 80.0, 800.0),
            -800.0
        );
        assert_eq!(
            edge_auto_scroll_velocity(40.0, viewport, 80.0, 800.0),
            -200.0
        );
        assert_eq!(
            edge_auto_scroll_velocity(560.0, viewport, 80.0, 800.0),
            200.0
        );
        assert_eq!(
            edge_auto_scroll_velocity(600.0, viewport, 80.0, 800.0),
            800.0
        );
    }

    #[test]
    fn edge_auto_scroll_rejects_invalid_configuration() {
        let viewport = Rect::from_min_max(Pos2::ZERO, Pos2::new(800.0, 600.0));

        assert_eq!(edge_auto_scroll_velocity(10.0, viewport, 0.0, 800.0), 0.0);
        assert_eq!(edge_auto_scroll_velocity(10.0, viewport, 80.0, -1.0), 0.0);
        assert_eq!(
            edge_auto_scroll_velocity(f32::NAN, viewport, 80.0, 800.0),
            0.0
        );
    }

    #[test]
    fn tile_ranges_cover_image_without_overlap() {
        assert_eq!(tile_row_ranges(0, 2_048), Vec::<Range<u32>>::new());
        assert_eq!(tile_row_ranges(10, 0), Vec::<Range<u32>>::new());
        assert_eq!(
            tile_row_ranges(4_097, 2_048),
            vec![0..2_048, 2_048..4_096, 4_096..4_097]
        );
    }

    #[test]
    fn tiled_texture_rejects_empty_images() {
        let source = SourceImage::from_pixels(
            "empty.png".into(),
            SourcePixels::Rgb(image::RgbImage::new(0, 1)),
        );

        assert!(matches!(
            TiledTexture::new(source, "empty"),
            Err(TiledTextureError::EmptyImage)
        ));
    }

    #[test]
    fn visible_tiles_cover_inside_spanning_partial_and_outside_rows() {
        let rows = tile_row_ranges(4_097, 2_048);

        assert_eq!(visible_tile_indices(&rows, 100..200), Some(0..=0));
        assert_eq!(visible_tile_indices(&rows, 2_000..2_100), Some(0..=1));
        assert_eq!(visible_tile_indices(&rows, 4_095..4_097), Some(1..=2));
        assert_eq!(visible_tile_indices(&rows, 5_000..6_000), None);
    }

    #[test]
    fn tiled_texture_uploads_lazily_with_a_per_frame_limit() {
        let context = Context::default();
        let source = SourceImage::from_pixels(
            "tall.png".into(),
            SourcePixels::Rgb(image::RgbImage::from_pixel(
                8,
                5_000,
                image::Rgb([10, 20, 30]),
            )),
        );
        let mut texture = TiledTexture::new(source, "tall").expect("texture should initialize");
        let first_clip = Rect::from_min_max(Pos2::ZERO, Pos2::new(8.0, 100.0));
        let first_painter = eframe::egui::Painter::new(
            context.clone(),
            eframe::egui::LayerId::background(),
            first_clip,
        );
        let transform = ViewportTransform::new(Pos2::ZERO, 1.0).expect("transform should be valid");

        assert!(texture.full.tiles.iter().all(|tile| tile.texture.is_none()));
        texture.paint(&context, &first_painter, transform);

        assert_eq!(
            texture
                .full
                .tiles
                .iter()
                .map(|tile| tile.texture.is_some())
                .collect::<Vec<_>>(),
            vec![true, true, false]
        );

        let bottom_clip = Rect::from_min_max(Pos2::new(0.0, 4_900.0), Pos2::new(8.0, 5_000.0));
        let bottom_painter = eframe::egui::Painter::new(
            context.clone(),
            eframe::egui::LayerId::background(),
            bottom_clip,
        );
        texture.paint(&context, &bottom_painter, transform);

        assert!(texture.full.tiles[2].texture.is_some());
        assert!(texture.full.tiles.iter().enumerate().all(|(index, tile)| {
            index.abs_diff(2) <= RETAINED_TILE_MARGIN || tile.texture.is_none()
        }));
    }

    #[test]
    fn tiled_texture_switches_levels_with_the_display_scale() {
        let context = Context::default();
        let source = SourceImage::from_pixels(
            "wide.png".into(),
            SourcePixels::Rgb(image::RgbImage::from_pixel(
                8,
                5_000,
                image::Rgb([10, 20, 30]),
            )),
        );
        let mut texture = TiledTexture::new(source, "wide").expect("texture should initialize");
        let clip = Rect::from_min_max(Pos2::ZERO, Pos2::new(8.0, 50.0));
        let painter =
            eframe::egui::Painter::new(context.clone(), eframe::egui::LayerId::background(), clip);
        let zoomed_out = ViewportTransform::new(Pos2::ZERO, HALF_RESOLUTION_MAXIMUM_SCALE)
            .expect("transform should be valid");
        let full_scale =
            ViewportTransform::new(Pos2::ZERO, 1.0).expect("transform should be valid");
        let resident = |level: &TileLevel| {
            level
                .tiles
                .iter()
                .map(|tile| tile.texture.is_some())
                .collect::<Vec<_>>()
        };

        texture.paint(&context, &painter, zoomed_out);
        let half = texture
            .half
            .as_ref()
            .expect("an 8 x 5000 source has a half level");

        assert_eq!(resident(half), vec![true, true]);
        assert_eq!(resident(&texture.full), vec![false, false, false]);

        texture.paint(&context, &painter, full_scale);
        let half = texture
            .half
            .as_ref()
            .expect("an 8 x 5000 source has a half level");

        assert_eq!(resident(half), vec![false, false]);
        assert_eq!(resident(&texture.full), vec![true, true, false]);
    }

    #[test]
    fn half_level_tiles_span_the_whole_source_height() {
        let source = SourceImage::from_pixels(
            "odd.png".into(),
            SourcePixels::Rgb(image::RgbImage::new(4, 4_099)),
        );

        let texture = TiledTexture::new(source, "odd").expect("texture should initialize");
        let half = texture
            .half
            .as_ref()
            .expect("a 4 x 4099 source has a half level");
        let ranges = half
            .tiles
            .iter()
            .map(|tile| (tile.pixel_rows.clone(), tile.source_rows.clone()))
            .collect::<Vec<_>>();

        assert_eq!(
            ranges,
            vec![(0..2_048, 0..4_096), (2_048..2_049, 4_096..4_099)]
        );
        assert_eq!(texture.full.tiles.len(), 3);
    }

    #[test]
    fn tiled_texture_releases_tiles_beyond_the_retained_margin() {
        let context = Context::default();
        let source = SourceImage::from_pixels(
            "very-tall.png".into(),
            SourcePixels::Rgb(image::RgbImage::from_pixel(
                8,
                9_000,
                image::Rgb([10, 20, 30]),
            )),
        );
        let mut texture =
            TiledTexture::new(source, "very-tall").expect("texture should initialize");
        let transform = ViewportTransform::new(Pos2::ZERO, 1.0).expect("transform should be valid");
        let first_clip = Rect::from_min_max(Pos2::ZERO, Pos2::new(8.0, 100.0));
        let first_painter = eframe::egui::Painter::new(
            context.clone(),
            eframe::egui::LayerId::background(),
            first_clip,
        );
        texture.paint(&context, &first_painter, transform);
        let bottom_clip = Rect::from_min_max(Pos2::new(0.0, 8_900.0), Pos2::new(8.0, 9_000.0));
        let bottom_painter = eframe::egui::Painter::new(
            context.clone(),
            eframe::egui::LayerId::background(),
            bottom_clip,
        );

        texture.paint(&context, &bottom_painter, transform);

        assert!(texture.full.tiles[0].texture.is_none());
        assert!(texture.full.tiles[1].texture.is_none());
        assert!(texture.full.tiles[3].texture.is_some());
        assert!(texture.full.tiles[4].texture.is_some());
    }

    #[test]
    fn tile_color_image_converts_rgb_pixels_exactly() {
        let pixels = SourcePixels::Rgb(
            image::RgbImage::from_raw(
                2,
                3,
                vec![
                    1, 2, 3, 4, 5, 6, 10, 20, 30, 40, 50, 60, 7, 8, 9, 10, 11, 12,
                ],
            )
            .expect("fixture dimensions should match the buffer"),
        );

        let image = tile_color_image(&pixels, 1..2);

        assert_eq!(image.size, [2, 1]);
        assert_eq!(
            image.pixels,
            vec![Color32::from_rgb(10, 20, 30), Color32::from_rgb(40, 50, 60)]
        );
    }

    #[test]
    fn tile_color_image_premultiplies_rgba_pixels() {
        let pixels = SourcePixels::Rgba(
            image::RgbaImage::from_raw(2, 1, vec![100, 50, 25, 128, 10, 20, 30, 255])
                .expect("fixture dimensions should match the buffer"),
        );

        let image = tile_color_image(&pixels, 0..1);

        assert_eq!(image.size, [2, 1]);
        assert_eq!(
            image.pixels,
            vec![
                Color32::from_rgba_unmultiplied(100, 50, 25, 128),
                Color32::from_rgba_unmultiplied(10, 20, 30, 255)
            ]
        );
    }
}
