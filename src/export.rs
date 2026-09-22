use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::{FilterType, resize};
use image::{ExtendedColorType, ImageEncoder, RgbaImage};
use thiserror::Error;

use crate::config::{ExportFormat, ExportSettings};
use crate::crop::CropRect;
use crate::image_io::SourceImage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportQuality(u8);

impl ExportQuality {
    pub fn new(value: u8) -> Result<Self, ExportError> {
        if !(1..=100).contains(&value) {
            return Err(ExportError::InvalidQuality(value));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn value(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputSize {
    width: u32,
    height: u32,
}

impl OutputSize {
    pub fn new(width: u32, height: u32) -> Result<Self, ExportError> {
        if width == 0 || height == 0 {
            return Err(ExportError::InvalidOutputSize { width, height });
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
pub struct ExportOptions {
    format: ExportFormat,
    quality: ExportQuality,
    output_size: Option<OutputSize>,
}

impl ExportOptions {
    #[must_use]
    pub const fn new(
        format: ExportFormat,
        quality: ExportQuality,
        output_size: Option<OutputSize>,
    ) -> Self {
        Self {
            format,
            quality,
            output_size,
        }
    }

    pub fn from_settings(settings: &ExportSettings) -> Result<Self, ExportError> {
        let quality = ExportQuality::new(settings.quality())?;
        let output_size = settings
            .output_size()
            .map(|(width, height)| OutputSize::new(width, height))
            .transpose()?;

        Ok(Self::new(settings.format(), quality, output_size))
    }

    #[must_use]
    pub const fn format(self) -> ExportFormat {
        self.format
    }

    #[must_use]
    pub const fn quality(self) -> ExportQuality {
        self.quality
    }

    #[must_use]
    pub const fn output_size(self) -> Option<OutputSize> {
        self.output_size
    }
}

#[derive(Debug, Clone)]
pub struct ExportRequest {
    source: SourceImage,
    crop: CropRect,
    destination: PathBuf,
    options: ExportOptions,
}

impl ExportRequest {
    #[must_use]
    pub fn new(
        source: SourceImage,
        crop: CropRect,
        destination: PathBuf,
        options: ExportOptions,
    ) -> Self {
        Self {
            source,
            crop,
            destination,
            options,
        }
    }

    #[must_use]
    pub fn destination(&self) -> &Path {
        &self.destination
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExportJobId(u64);

impl ExportJobId {
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReceipt {
    destination: PathBuf,
    width: u32,
    height: u32,
}

impl ExportReceipt {
    #[must_use]
    pub fn destination(&self) -> &Path {
        &self.destination
    }

    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }
}

#[derive(Debug)]
pub struct ExportResult {
    id: ExportJobId,
    destination: PathBuf,
    outcome: Result<ExportReceipt, ExportError>,
}

impl ExportResult {
    #[must_use]
    pub const fn id(&self) -> ExportJobId {
        self.id
    }

    #[must_use]
    pub fn destination(&self) -> &Path {
        &self.destination
    }

    pub const fn outcome(&self) -> &Result<ExportReceipt, ExportError> {
        &self.outcome
    }

    pub fn into_outcome(self) -> Result<ExportReceipt, ExportError> {
        self.outcome
    }
}

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("export quality must be between 1 and 100, got {0}")]
    InvalidQuality(u8),

    #[error("output dimensions must be non-zero, got {width}x{height}")]
    InvalidOutputSize { width: u32, height: u32 },

    #[error(
        "crop ({x}, {y}, {width}x{height}) is outside source dimensions {source_width}x{source_height}"
    )]
    InvalidCrop {
        x: u32,

        y: u32,

        width: u32,

        height: u32,

        source_width: u32,

        source_height: u32,
    },

    #[error("refusing to overwrite existing output {0}")]
    DestinationExists(PathBuf),

    #[error("failed to create export destination {path}: {source}")]
    CreateDestination {
        path: PathBuf,

        #[source]
        source: std::io::Error,
    },

    #[error("failed to write export destination {path}: {source}")]
    WriteDestination {
        path: PathBuf,

        #[source]
        source: std::io::Error,
    },

    #[error("failed to encode {format}: {source}")]
    ImageEncoding {
        format: &'static str,

        #[source]
        source: image::ImageError,
    },

    #[error("failed to encode WebP: {0:?}")]
    WebPEncoding(webp::WebPEncodingError),
}

#[derive(Debug, Error)]
pub enum ExportQueueError {
    #[error("export worker has stopped")]
    WorkerStopped,

    #[error("failed to start export worker: {0}")]
    SpawnWorker(#[source] std::io::Error),

    #[error("export job identifiers are exhausted")]
    JobIdExhausted,

    #[error("export worker panicked")]
    WorkerPanicked,
}

#[derive(Debug)]
pub struct ExportQueue {
    request_sender: Option<Sender<ExportJob>>,
    result_receiver: Receiver<ExportResult>,
    worker: Option<JoinHandle<()>>,
    next_id: AtomicU64,
}

#[derive(Debug)]
struct ExportJob {
    id: ExportJobId,
    request: ExportRequest,
}

impl ExportQueue {
    pub fn new(repaint: eframe::egui::Context) -> Result<Self, ExportQueueError> {
        let (request_sender, request_receiver) = crossbeam_channel::unbounded::<ExportJob>();
        let (result_sender, result_receiver) = crossbeam_channel::unbounded();
        let worker = thread::Builder::new()
            .name(String::from("cropdeck-export"))
            .spawn(move || {
                while let Ok(job) = request_receiver.recv() {
                    let destination = job.request.destination.clone();
                    let outcome = export_image(&job.request);
                    let result = ExportResult {
                        id: job.id,
                        destination,
                        outcome,
                    };
                    if result_sender.send(result).is_err() {
                        break;
                    }
                    repaint.request_repaint();
                }
            })
            .map_err(ExportQueueError::SpawnWorker)?;

        Ok(Self {
            request_sender: Some(request_sender),
            result_receiver,
            worker: Some(worker),
            next_id: AtomicU64::new(1),
        })
    }

    pub fn submit(&self, request: ExportRequest) -> Result<ExportJobId, ExportQueueError> {
        let id = self
            .next_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map(ExportJobId)
            .map_err(|_| ExportQueueError::JobIdExhausted)?;
        let sender = self
            .request_sender
            .as_ref()
            .ok_or(ExportQueueError::WorkerStopped)?;

        sender
            .send(ExportJob { id, request })
            .map(|()| id)
            .map_err(|_send_error| ExportQueueError::WorkerStopped)
    }

    pub fn poll_result(&self) -> Result<Option<ExportResult>, ExportQueueError> {
        match self.result_receiver.try_recv() {
            Ok(result) => Ok(Some(result)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(ExportQueueError::WorkerStopped),
        }
    }

    pub fn shutdown(mut self) -> Result<(), ExportQueueError> {
        self.stop_and_join()
    }

    fn stop_and_join(&mut self) -> Result<(), ExportQueueError> {
        self.request_sender.take();
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| ExportQueueError::WorkerPanicked)?;
        }
        Ok(())
    }
}

impl Drop for ExportQueue {
    fn drop(&mut self) {
        let _ = self.stop_and_join();
    }
}

pub fn export_image(request: &ExportRequest) -> Result<ExportReceipt, ExportError> {
    validate_crop(&request.source, &request.crop)?;
    let prepared = crop_pixels(&request.source, &request.crop, request.options.output_size);
    let bytes = encode_pixels(&prepared, request.options)?;
    write_new_file(&request.destination, &bytes)?;

    Ok(ExportReceipt {
        destination: request.destination.clone(),
        width: prepared.width(),
        height: prepared.height(),
    })
}

fn validate_crop(source: &SourceImage, crop: &CropRect) -> Result<(), ExportError> {
    let dimensions = source.dimensions();
    let crop_right = crop.x().checked_add(crop.width());
    let crop_bottom = crop.y().checked_add(crop.height());
    let valid = crop.width() > 0
        && crop.height() > 0
        && crop_right.is_some_and(|right| right <= dimensions.width())
        && crop_bottom.is_some_and(|bottom| bottom <= dimensions.height());

    if valid {
        return Ok(());
    }

    Err(ExportError::InvalidCrop {
        x: crop.x(),
        y: crop.y(),
        width: crop.width(),
        height: crop.height(),
        source_width: dimensions.width(),
        source_height: dimensions.height(),
    })
}

pub fn crop_pixels(
    source: &SourceImage,
    crop: &CropRect,
    output_size: Option<OutputSize>,
) -> RgbaImage {
    let pixels = source
        .pixels()
        .crop_to_rgba(crop.x(), crop.y(), crop.width(), crop.height());

    match output_size {
        Some(size) if size.width != crop.width() || size.height != crop.height() => {
            resize(&pixels, size.width, size.height, FilterType::Lanczos3)
        }
        Some(_) | None => pixels,
    }
}

fn encode_pixels(image: &RgbaImage, options: ExportOptions) -> Result<Vec<u8>, ExportError> {
    let estimated_size = image.as_raw().len() / 2;
    let mut encoded = Vec::with_capacity(estimated_size);

    match options.format {
        ExportFormat::Png => PngEncoder::new(&mut encoded)
            .write_image(
                image.as_raw(),
                image.width(),
                image.height(),
                ExtendedColorType::Rgba8,
            )
            .map_err(|source| ExportError::ImageEncoding {
                format: "PNG",
                source,
            })?,
        ExportFormat::Jpeg => {
            JpegEncoder::new_with_quality(&mut encoded, options.quality.value())
                .encode_image(image)
                .map_err(|source| ExportError::ImageEncoding {
                    format: "JPEG",
                    source,
                })?;
        }
        ExportFormat::WebP => {
            let encoded_webp =
                webp::Encoder::from_rgba(image.as_raw(), image.width(), image.height())
                    .encode_simple(false, f32::from(options.quality.value()))
                    .map_err(ExportError::WebPEncoding)?;
            encoded.extend_from_slice(&encoded_webp);
        }
    }

    Ok(encoded)
}

fn write_new_file(destination: &Path, bytes: &[u8]) -> Result<(), ExportError> {
    let mut output = open_new_file(destination)?;
    output
        .write_all(bytes)
        .map_err(|source| ExportError::WriteDestination {
            path: destination.to_path_buf(),
            source,
        })
}

fn open_new_file(destination: &Path) -> Result<File, ExportError> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::AlreadyExists {
                ExportError::DestinationExists(destination.to_path_buf())
            } else {
                ExportError::CreateDestination {
                    path: destination.to_path_buf(),
                    source,
                }
            }
        })
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use image::{Rgba, RgbaImage};
    use tempfile::tempdir;

    use crate::crop::SourceSize;
    use crate::image_io::decode_image;

    use super::*;

    fn source_fixture() -> (tempfile::TempDir, SourceImage) {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("source.png");
        let image = RgbaImage::from_fn(4, 4, |x, y| {
            Rgba([
                u8::try_from(x * 40).expect("fixture channel should fit"),
                u8::try_from(y * 40).expect("fixture channel should fit"),
                100,
                255,
            ])
        });
        image.save(&path).expect("source fixture should be encoded");
        let source = decode_image(&path).expect("source fixture should decode");
        (directory, source)
    }

    fn crop_fixture() -> CropRect {
        let source = SourceSize::new(4, 4).expect("fixture source should be valid");
        CropRect::new(1, 1, 2, 2, source).expect("fixture crop should be valid")
    }

    fn options(format: ExportFormat, output_size: Option<OutputSize>) -> ExportOptions {
        ExportOptions::new(
            format,
            ExportQuality::new(85).expect("fixture quality should be valid"),
            output_size,
        )
    }

    #[test]
    fn quality_and_output_size_validation_reject_invalid_values() {
        assert!(matches!(
            ExportQuality::new(0),
            Err(ExportError::InvalidQuality(0))
        ));
        assert!(matches!(
            OutputSize::new(0, 10),
            Err(ExportError::InvalidOutputSize {
                width: 0,
                height: 10
            })
        ));
    }

    #[test]
    fn png_export_uses_exact_source_crop_coordinates() {
        let (directory, source) = source_fixture();
        let destination = directory.path().join("crop.png");
        let request = ExportRequest::new(
            source,
            crop_fixture(),
            destination.clone(),
            options(ExportFormat::Png, None),
        );

        let receipt = export_image(&request).expect("PNG export should succeed");
        let exported = image::open(destination)
            .expect("PNG export should decode")
            .into_rgba8();

        assert_eq!((receipt.width(), receipt.height()), (2, 2));
        assert_eq!(exported.get_pixel(0, 0), &Rgba([40, 40, 100, 255]));
        assert_eq!(exported.get_pixel(1, 1), &Rgba([80, 80, 100, 255]));
    }

    #[test]
    fn optional_resize_uses_requested_dimensions() {
        let (directory, source) = source_fixture();
        let destination = directory.path().join("resized.png");
        let size = OutputSize::new(6, 8).expect("fixture dimensions should be valid");
        let request = ExportRequest::new(
            source,
            crop_fixture(),
            destination.clone(),
            options(ExportFormat::Png, Some(size)),
        );

        export_image(&request).expect("resized export should succeed");
        let exported = image::open(destination).expect("resized export should decode");

        assert_eq!((exported.width(), exported.height()), (6, 8));
    }

    #[test]
    fn jpeg_and_lossy_webp_exports_are_decodable() {
        for (format, extension) in [(ExportFormat::Jpeg, "jpg"), (ExportFormat::WebP, "webp")] {
            let (directory, source) = source_fixture();
            let destination = directory.path().join(format!("crop.{extension}"));
            let request = ExportRequest::new(
                source,
                crop_fixture(),
                destination.clone(),
                options(format, None),
            );

            export_image(&request).expect("lossy export should succeed");
            let exported = decode_image(&destination).expect("lossy export should decode");
            let dimensions = exported.dimensions();

            assert_eq!((dimensions.width(), dimensions.height()), (2, 2));
        }
    }

    #[test]
    fn export_refuses_to_overwrite_existing_destination() {
        let (directory, source) = source_fixture();
        let destination = directory.path().join("existing.png");
        std::fs::write(&destination, b"keep me").expect("existing fixture should be written");
        let request = ExportRequest::new(
            source,
            crop_fixture(),
            destination.clone(),
            options(ExportFormat::Png, None),
        );

        let error = export_image(&request).expect_err("overwrite must be rejected");

        assert!(matches!(error, ExportError::DestinationExists(path) if path == destination));
        assert_eq!(
            std::fs::read(request.destination()).expect("existing fixture should remain readable"),
            b"keep me"
        );
    }

    #[test]
    fn out_of_bounds_crop_is_rejected_before_encoding() {
        let (directory, source) = source_fixture();
        let destination = directory.path().join("invalid.png");
        let larger_source = SourceSize::new(10, 10).expect("fixture source should be valid");
        let crop =
            CropRect::new(3, 3, 2, 2, larger_source).expect("crop should fit its original source");
        let request = ExportRequest::new(
            source,
            crop,
            destination.clone(),
            options(ExportFormat::Png, None),
        );

        let error = export_image(&request).expect_err("out-of-bounds crop must be rejected");

        assert!(matches!(error, ExportError::InvalidCrop { .. }));
        assert!(!destination.exists());
    }

    #[test]
    fn queue_exports_on_worker_and_reports_completion() {
        let (directory, source) = source_fixture();
        let destination = directory.path().join("queued.png");
        let request = ExportRequest::new(
            source,
            crop_fixture(),
            destination.clone(),
            options(ExportFormat::Png, None),
        );
        let queue =
            ExportQueue::new(eframe::egui::Context::default()).expect("worker should start");

        let id = queue.submit(request).expect("job should be accepted");
        let deadline = Instant::now() + Duration::from_secs(5);
        let result = loop {
            if let Some(result) = queue.poll_result().expect("worker should remain available") {
                break result;
            }
            assert!(Instant::now() < deadline, "worker result timed out");
            std::thread::yield_now();
        };

        assert_eq!(result.id(), id);
        assert_eq!(result.destination(), destination);
        assert!(result.outcome().is_ok());
        queue.shutdown().expect("worker should stop cleanly");
    }

    #[test]
    fn queue_accepts_more_jobs_than_the_old_bounded_capacity() {
        let (directory, source) = source_fixture();
        let queue =
            ExportQueue::new(eframe::egui::Context::default()).expect("worker should start");
        let job_count = 16;

        for index in 0..job_count {
            let destination = directory.path().join(format!("queued_{index:02}.png"));
            let request = ExportRequest::new(
                source.clone(),
                crop_fixture(),
                destination,
                options(ExportFormat::Png, None),
            );
            queue
                .submit(request)
                .expect("an unbounded queue should accept every job");
        }

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut receipts = 0;
        while receipts < job_count {
            if queue
                .poll_result()
                .expect("worker should remain available")
                .is_some()
            {
                receipts += 1;
                continue;
            }
            assert!(Instant::now() < deadline, "worker results timed out");
            std::thread::yield_now();
        }

        assert_eq!(receipts, job_count);
        queue.shutdown().expect("worker should stop cleanly");
    }
}
