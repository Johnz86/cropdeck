use std::cmp::Ordering;
use std::fs;
use std::io::Cursor;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::{DynamicImage, ImageReader, RgbImage, RgbaImage};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScanDepth {
    #[default]
    FolderOnly,

    Recursive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageDimensions {
    width: u32,
    height: u32,
}

impl ImageDimensions {
    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }
}

#[derive(Debug)]
pub enum SourcePixels {
    Rgb(RgbImage),
    Rgba(RgbaImage),
}

impl SourcePixels {
    #[must_use]
    pub fn width(&self) -> u32 {
        match self {
            Self::Rgb(pixels) => pixels.width(),
            Self::Rgba(pixels) => pixels.width(),
        }
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        match self {
            Self::Rgb(pixels) => pixels.height(),
            Self::Rgba(pixels) => pixels.height(),
        }
    }

    #[must_use]
    pub const fn bytes_per_pixel(&self) -> usize {
        match self {
            Self::Rgb(_) => 3,
            Self::Rgba(_) => 4,
        }
    }

    #[must_use]
    pub fn byte_len(&self) -> usize {
        match self {
            Self::Rgb(pixels) => pixels.as_raw().len(),
            Self::Rgba(pixels) => pixels.as_raw().len(),
        }
    }

    #[must_use]
    pub fn rows(&self, rows: Range<u32>) -> &[u8] {
        debug_assert!(rows.start <= rows.end && rows.end <= self.height());
        let row_bytes = self.width() as usize * self.bytes_per_pixel();
        let start = rows.start as usize * row_bytes;
        let end = rows.end as usize * row_bytes;
        match self {
            Self::Rgb(pixels) => &pixels.as_raw()[start..end],
            Self::Rgba(pixels) => &pixels.as_raw()[start..end],
        }
    }

    #[must_use]
    pub fn crop_to_rgba(&self, x: u32, y: u32, width: u32, height: u32) -> RgbaImage {
        match self {
            Self::Rgb(pixels) => {
                let cropped = image::imageops::crop_imm(pixels, x, y, width, height).to_image();
                DynamicImage::ImageRgb8(cropped).into_rgba8()
            }
            Self::Rgba(pixels) => image::imageops::crop_imm(pixels, x, y, width, height).to_image(),
        }
    }

    #[must_use]
    pub fn half_resolution(&self) -> Option<Self> {
        let (width, height) = (self.width() / 2, self.height() / 2);
        if width == 0 || height == 0 {
            return None;
        }
        let source = self.rows(0..height * 2);
        let row_bytes = self.width() as usize * self.bytes_per_pixel();
        match self {
            Self::Rgb(_) => {
                RgbImage::from_raw(width, height, box_downsample::<3>(source, row_bytes))
                    .map(Self::Rgb)
            }
            Self::Rgba(_) => {
                RgbaImage::from_raw(width, height, box_downsample::<4>(source, row_bytes))
                    .map(Self::Rgba)
            }
        }
    }
}

fn box_downsample<const CHANNELS: usize>(source: &[u8], row_bytes: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(source.len() / 4);
    for row_pair in source.chunks_exact(row_bytes * 2) {
        let (upper, lower) = row_pair.split_at(row_bytes);
        let (upper_pairs, _odd_upper) = upper.as_chunks::<CHANNELS>().0.as_chunks::<2>();
        let (lower_pairs, _odd_lower) = lower.as_chunks::<CHANNELS>().0.as_chunks::<2>();
        for ([top_left, top_right], [bottom_left, bottom_right]) in
            upper_pairs.iter().zip(lower_pairs)
        {
            output.extend((0..CHANNELS).map(|channel| {
                let sum = u16::from(top_left[channel])
                    + u16::from(top_right[channel])
                    + u16::from(bottom_left[channel])
                    + u16::from(bottom_right[channel]);
                ((sum + 2) / 4) as u8
            }));
        }
    }
    output
}

impl From<DynamicImage> for SourcePixels {
    fn from(dynamic: DynamicImage) -> Self {
        match dynamic {
            DynamicImage::ImageRgb8(pixels) => Self::Rgb(pixels),
            DynamicImage::ImageRgba8(pixels) => Self::Rgba(pixels),
            dynamic if dynamic.color().has_alpha() => Self::Rgba(dynamic.into_rgba8()),
            dynamic => Self::Rgb(dynamic.into_rgb8()),
        }
    }
}

#[derive(Debug)]
struct SourceImageData {
    path: PathBuf,
    pixels: SourcePixels,
    half_pixels: Option<SourcePixels>,
    dimensions: ImageDimensions,
}

#[derive(Debug, Clone)]
pub struct SourceImage {
    data: Arc<SourceImageData>,
}

impl SourceImage {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.data.path
    }

    #[must_use]
    pub fn dimensions(&self) -> ImageDimensions {
        self.data.dimensions
    }

    #[must_use]
    pub fn pixels(&self) -> &SourcePixels {
        &self.data.pixels
    }

    #[must_use]
    pub fn half_pixels(&self) -> Option<&SourcePixels> {
        self.data.half_pixels.as_ref()
    }

    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.data.pixels.byte_len()
            + self
                .data
                .half_pixels
                .as_ref()
                .map_or(0, SourcePixels::byte_len)
    }

    fn new(path: PathBuf, pixels: SourcePixels) -> Self {
        let dimensions = ImageDimensions {
            width: pixels.width(),
            height: pixels.height(),
        };
        let half_pixels = pixels.half_resolution();
        Self {
            data: Arc::new(SourceImageData {
                path,
                pixels,
                half_pixels,
                dimensions,
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_pixels(path: PathBuf, pixels: SourcePixels) -> Self {
        Self::new(path, pixels)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageQueue {
    paths: Vec<PathBuf>,
    current_index: usize,
}

impl ImageQueue {
    pub fn from_folder(folder: &Path, depth: ScanDepth) -> Result<Self, ImageIoError> {
        let paths = discover_images(folder, depth)?;
        if paths.is_empty() {
            return Err(ImageIoError::NoSupportedImages(folder.to_path_buf()));
        }

        Ok(Self {
            paths,
            current_index: 0,
        })
    }

    pub fn from_paths<I>(paths: I) -> Result<Self, ImageIoError>
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let mut paths: Vec<_> = paths
            .into_iter()
            .filter(|path| is_supported_image(path))
            .collect();
        sort_paths_naturally(&mut paths);

        if paths.is_empty() {
            return Err(ImageIoError::EmptyQueue);
        }

        Ok(Self {
            paths,
            current_index: 0,
        })
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.paths.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    #[must_use]
    pub const fn current_index(&self) -> usize {
        self.current_index
    }

    #[must_use]
    pub fn current(&self) -> &Path {
        &self.paths[self.current_index]
    }

    pub fn move_next(&mut self) -> Option<&Path> {
        let next_index = self.current_index.checked_add(1)?;
        if next_index >= self.paths.len() {
            return None;
        }

        self.current_index = next_index;
        Some(self.current())
    }

    pub fn move_previous(&mut self) -> Option<&Path> {
        self.current_index = self.current_index.checked_sub(1)?;
        Some(self.current())
    }

    #[must_use]
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }
}

#[derive(Debug, Error)]
pub enum ImageIoError {
    #[error("failed to read image folder {path}: {source}")]
    ReadDirectory {
        path: PathBuf,

        #[source]
        source: std::io::Error,
    },

    #[error("failed to inspect directory entry {path}: {source}")]
    InspectEntry {
        path: PathBuf,

        #[source]
        source: std::io::Error,
    },

    #[error("no supported JPEG, PNG, or WebP images found in {0}")]
    NoSupportedImages(PathBuf),

    #[error("no supported JPEG, PNG, or WebP paths were supplied")]
    EmptyQueue,

    #[error("failed to open image {path}: {source}")]
    OpenImage {
        path: PathBuf,

        #[source]
        source: std::io::Error,
    },

    #[error("failed to identify image format for {path}: {source}")]
    IdentifyFormat {
        path: PathBuf,

        #[source]
        source: image::ImageError,
    },

    #[error("failed to decode image {path}: {source}")]
    DecodeImage {
        path: PathBuf,

        #[source]
        source: image::ImageError,
    },

    #[error("unsupported or corrupt WebP image {0}")]
    UnsupportedWebP(PathBuf),

    #[error("image dimensions must be greater than zero for {0}")]
    EmptyImage(PathBuf),
}

#[must_use]
pub fn is_supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["jpg", "jpeg", "png", "webp"]
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
}

pub fn discover_images(folder: &Path, depth: ScanDepth) -> Result<Vec<PathBuf>, ImageIoError> {
    let mut pending_directories = vec![folder.to_path_buf()];
    let mut paths = Vec::new();

    while let Some(directory) = pending_directories.pop() {
        let entries = fs::read_dir(&directory).map_err(|source| ImageIoError::ReadDirectory {
            path: directory.clone(),
            source,
        })?;

        for entry in entries {
            let entry = entry.map_err(|source| ImageIoError::ReadDirectory {
                path: directory.clone(),
                source,
            })?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|source| ImageIoError::InspectEntry {
                    path: path.clone(),
                    source,
                })?;

            if file_type.is_file() && is_supported_image(&path) {
                paths.push(path);
            } else if depth == ScanDepth::Recursive && file_type.is_dir() {
                pending_directories.push(path);
            }
        }
    }

    sort_paths_naturally(&mut paths);
    Ok(paths)
}

pub fn decode_image(path: &Path) -> Result<SourceImage, ImageIoError> {
    let bytes = fs::read(path).map_err(|source| ImageIoError::OpenImage {
        path: path.to_path_buf(),
        source,
    })?;
    let format = image::guess_format(&bytes).map_err(|source| ImageIoError::IdentifyFormat {
        path: path.to_path_buf(),
        source,
    })?;
    let pixels = if format == image::ImageFormat::WebP {
        decode_webp(path, &bytes)?
    } else {
        let dynamic = ImageReader::with_format(Cursor::new(&bytes), format)
            .decode()
            .map_err(|source| ImageIoError::DecodeImage {
                path: path.to_path_buf(),
                source,
            })?;
        SourcePixels::from(dynamic)
    };
    if pixels.width() == 0 || pixels.height() == 0 {
        return Err(ImageIoError::EmptyImage(path.to_path_buf()));
    }
    Ok(SourceImage::new(path.to_path_buf(), pixels))
}

fn decode_webp(path: &Path, bytes: &[u8]) -> Result<SourcePixels, ImageIoError> {
    let decoded = webp::Decoder::new(bytes)
        .decode()
        .ok_or_else(|| ImageIoError::UnsupportedWebP(path.to_path_buf()))?;
    let (width, height) = (decoded.width(), decoded.height());
    match decoded.layout() {
        webp::PixelLayout::Rgb => RgbImage::from_raw(width, height, decoded.to_vec())
            .map(SourcePixels::Rgb)
            .ok_or_else(|| ImageIoError::UnsupportedWebP(path.to_path_buf())),
        webp::PixelLayout::Rgba => RgbaImage::from_raw(width, height, decoded.to_vec())
            .map(SourcePixels::Rgba)
            .ok_or_else(|| ImageIoError::UnsupportedWebP(path.to_path_buf())),
    }
}

fn sort_paths_naturally(paths: &mut [PathBuf]) {
    paths.sort_by(|left, right| {
        let ordering = natord::compare_ignore_case(
            left.to_string_lossy().as_ref(),
            right.to_string_lossy().as_ref(),
        );
        if ordering == Ordering::Equal {
            left.cmp(right)
        } else {
            ordering
        }
    });
}

#[cfg(test)]
mod tests {
    use std::fs;

    use image::{GrayAlphaImage, GrayImage, Luma, LumaA, Rgb, RgbImage, Rgba, RgbaImage};
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn supported_extensions_are_case_insensitive() {
        assert!(is_supported_image(Path::new("source.JPEG")));
        assert!(is_supported_image(Path::new("source.png")));
        assert!(is_supported_image(Path::new("source.WeBp")));
        assert!(!is_supported_image(Path::new("source.gif")));
        assert!(!is_supported_image(Path::new("source")));
    }

    #[test]
    fn discovery_filters_and_naturally_sorts_paths() {
        let directory = tempdir().expect("temporary directory should be created");
        for name in ["page10.png", "page2.webp", "page1.jpg", "notes.txt"] {
            fs::write(directory.path().join(name), []).expect("fixture should be written");
        }

        let paths = discover_images(directory.path(), ScanDepth::FolderOnly)
            .expect("image discovery should succeed");
        let names: Vec<_> = paths
            .iter()
            .filter_map(|path| path.file_name().and_then(|name| name.to_str()))
            .collect();

        assert_eq!(names, ["page1.jpg", "page2.webp", "page10.png"]);
    }

    #[test]
    fn recursive_discovery_includes_nested_images() {
        let directory = tempdir().expect("temporary directory should be created");
        let nested = directory.path().join("chapter2");
        fs::create_dir(&nested).expect("nested fixture directory should be created");
        fs::write(directory.path().join("page1.png"), []).expect("fixture should be written");
        fs::write(nested.join("page2.png"), []).expect("fixture should be written");

        let shallow = discover_images(directory.path(), ScanDepth::FolderOnly)
            .expect("shallow discovery should succeed");
        let recursive = discover_images(directory.path(), ScanDepth::Recursive)
            .expect("recursive discovery should succeed");

        assert_eq!(shallow.len(), 1);
        assert_eq!(recursive.len(), 2);
    }

    #[test]
    fn queue_navigation_stops_at_each_boundary() {
        let mut queue =
            ImageQueue::from_paths([PathBuf::from("page10.png"), PathBuf::from("page2.png")])
                .expect("supported paths should form a queue");

        assert_eq!(queue.current(), Path::new("page2.png"));
        assert_eq!(queue.move_previous(), None);
        assert_eq!(queue.move_next(), Some(Path::new("page10.png")));
        assert_eq!(queue.move_next(), None);
        assert_eq!(queue.current_index(), 1);
    }

    #[test]
    fn rgb_rows_return_each_requested_contiguous_range() {
        let pixels = SourcePixels::Rgb(
            RgbImage::from_raw(2, 3, (0..18).collect())
                .expect("fixture dimensions should match the buffer"),
        );

        assert_eq!(pixels.rows(0..1), &[0, 1, 2, 3, 4, 5]);
        assert_eq!(pixels.rows(1..2), &[6, 7, 8, 9, 10, 11]);
        assert_eq!(pixels.rows(2..3), &[12, 13, 14, 15, 16, 17]);
        assert!(pixels.rows(1..1).is_empty());
    }

    #[test]
    fn rgba_rows_return_each_requested_contiguous_range() {
        let pixels = SourcePixels::Rgba(
            RgbaImage::from_raw(2, 3, (0..24).collect())
                .expect("fixture dimensions should match the buffer"),
        );

        assert_eq!(pixels.rows(0..1), &[0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(pixels.rows(1..2), &[8, 9, 10, 11, 12, 13, 14, 15]);
        assert_eq!(pixels.rows(2..3), &[16, 17, 18, 19, 20, 21, 22, 23]);
        assert!(pixels.rows(2..2).is_empty());
    }

    #[test]
    fn rgb_crop_converts_to_exact_opaque_rgba_pixels() {
        let source = RgbImage::from_fn(3, 2, |x, y| {
            Rgb([
                u8::try_from(x * 10).expect("fixture channel should fit"),
                u8::try_from(y * 20).expect("fixture channel should fit"),
                30,
            ])
        });

        let crop = SourcePixels::Rgb(source).crop_to_rgba(1, 0, 2, 2);

        assert_eq!(crop.dimensions(), (2, 2));
        assert_eq!(crop.get_pixel(0, 0), &Rgba([10, 0, 30, 255]));
        assert_eq!(crop.get_pixel(1, 1), &Rgba([20, 20, 30, 255]));
    }

    #[test]
    fn half_resolution_averages_rgb_blocks_and_drops_odd_edges() {
        let source = RgbImage::from_fn(5, 3, |x, y| {
            let base = u8::try_from(y * 40 + x * 8).expect("fixture channel should fit");
            Rgb([base, base + 1, base + 2])
        });

        let half = SourcePixels::Rgb(source)
            .half_resolution()
            .expect("a 5 x 3 source has a half copy");

        assert_eq!((half.width(), half.height()), (2, 1));
        assert_eq!(half.rows(0..1), &[24, 25, 26, 40, 41, 42]);
    }

    #[test]
    fn half_resolution_averages_every_rgba_channel_with_rounding() {
        let source = RgbaImage::from_raw(
            2,
            2,
            vec![0, 10, 255, 1, 1, 10, 255, 2, 2, 10, 253, 2, 3, 10, 251, 3],
        )
        .expect("fixture dimensions should match the buffer");

        let half = SourcePixels::Rgba(source)
            .half_resolution()
            .expect("a 2 x 2 source has a half copy");

        assert!(matches!(half, SourcePixels::Rgba(_)));
        assert_eq!(half.rows(0..1), &[2, 10, 254, 2]);
    }

    #[test]
    fn half_resolution_is_absent_below_two_pixels_in_either_dimension() {
        assert!(
            SourcePixels::Rgb(RgbImage::new(1, 4))
                .half_resolution()
                .is_none()
        );
        assert!(
            SourcePixels::Rgba(RgbaImage::new(4, 1))
                .half_resolution()
                .is_none()
        );
    }

    #[test]
    fn source_byte_len_includes_the_half_resolution_copy() {
        let source = SourceImage::from_pixels(
            PathBuf::from("square.png"),
            SourcePixels::Rgb(RgbImage::new(4, 4)),
        );

        assert_eq!(source.pixels().byte_len(), 48);
        assert_eq!(source.half_pixels().map(SourcePixels::byte_len), Some(12));
        assert_eq!(source.byte_len(), 60);
    }

    #[test]
    fn dynamic_images_preserve_or_select_the_expected_native_layout() {
        let rgb = DynamicImage::ImageRgb8(RgbImage::new(1, 1));
        let rgba = DynamicImage::ImageRgba8(RgbaImage::new(1, 1));
        let luma = DynamicImage::ImageLuma8(GrayImage::from_pixel(1, 1, Luma([10])));
        let luma_alpha =
            DynamicImage::ImageLumaA8(GrayAlphaImage::from_pixel(1, 1, LumaA([10, 20])));

        assert!(matches!(SourcePixels::from(rgb), SourcePixels::Rgb(_)));
        assert!(matches!(SourcePixels::from(rgba), SourcePixels::Rgba(_)));
        assert!(matches!(SourcePixels::from(luma), SourcePixels::Rgb(_)));
        assert!(matches!(
            SourcePixels::from(luma_alpha),
            SourcePixels::Rgba(_)
        ));
    }

    #[test]
    fn decode_preserves_png_layouts_and_uses_libwebp() {
        let directory = tempdir().expect("temporary directory should be created");
        let rgb_path = directory.path().join("rgb.png");
        let rgba_path = directory.path().join("rgba.png");
        let webp_path = directory.path().join("rgb.webp");
        let rgb = RgbImage::from_fn(2, 2, |x, y| {
            Rgb([
                u8::try_from(x * 30).expect("fixture channel should fit"),
                u8::try_from(y * 40).expect("fixture channel should fit"),
                50,
            ])
        });
        let rgba = RgbaImage::from_pixel(2, 2, Rgba([10, 20, 30, 40]));
        rgb.save(&rgb_path).expect("RGB PNG should be encoded");
        rgba.save(&rgba_path).expect("RGBA PNG should be encoded");
        let encoded =
            webp::Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height()).encode_lossless();
        fs::write(&webp_path, &*encoded).expect("WebP fixture should be written");

        let decoded_rgb = decode_image(&rgb_path).expect("RGB PNG should decode");
        let decoded_rgba = decode_image(&rgba_path).expect("RGBA PNG should decode");
        let decoded_webp = decode_image(&webp_path).expect("WebP should decode");
        let shared = decoded_webp.clone();

        assert!(matches!(decoded_rgb.pixels(), SourcePixels::Rgb(_)));
        assert!(matches!(decoded_rgba.pixels(), SourcePixels::Rgba(_)));
        let SourcePixels::Rgb(webp_pixels) = decoded_webp.pixels() else {
            panic!("opaque WebP should retain RGB pixels");
        };
        assert_eq!(webp_pixels, &rgb);
        assert_eq!(decoded_webp.path(), webp_path);
        assert_eq!(decoded_webp.dimensions().width(), 2);
        assert_eq!(decoded_webp.dimensions().height(), 2);
        assert!(Arc::ptr_eq(&decoded_webp.data, &shared.data));
    }
}
