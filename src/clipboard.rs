use std::borrow::Cow;
use std::thread::{self, JoinHandle};

use arboard::{Clipboard, ImageData};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use image::RgbaImage;
use thiserror::Error;

use crate::crop::CropRect;
use crate::export::{OutputSize, crop_pixels};
use crate::image_io::SourceImage;

#[derive(Debug, Error)]
pub enum ClipboardError {
    #[error("the system clipboard is unavailable: {0}")]
    Unavailable(#[source] arboard::Error),

    #[error("the system clipboard rejected the crop: {0}")]
    Write(#[source] arboard::Error),
}

#[derive(Debug, Error)]
pub enum ClipboardServiceError {
    #[error("failed to start clipboard worker: {0}")]
    SpawnWorker(#[source] std::io::Error),

    #[error("clipboard worker has stopped")]
    WorkerStopped,

    #[error("clipboard worker panicked")]
    WorkerPanicked,
}

#[derive(Debug, Clone)]
pub struct ClipboardRequest {
    source: SourceImage,
    crop: CropRect,
    output_size: Option<OutputSize>,
}

impl ClipboardRequest {
    #[must_use]
    pub const fn new(source: SourceImage, crop: CropRect, output_size: Option<OutputSize>) -> Self {
        Self {
            source,
            crop,
            output_size,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardReceipt {
    width: u32,
    height: u32,
}

impl ClipboardReceipt {
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
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

#[derive(Debug)]
pub struct ClipboardService {
    requests: Option<Sender<ClipboardRequest>>,
    results: Receiver<Result<ClipboardReceipt, ClipboardError>>,
    worker: Option<JoinHandle<()>>,
}

impl ClipboardService {
    pub fn new(repaint: eframe::egui::Context) -> Result<Self, ClipboardServiceError> {
        let (request_sender, request_receiver) = crossbeam_channel::unbounded::<ClipboardRequest>();
        let (result_sender, results) = crossbeam_channel::unbounded();
        let worker = thread::Builder::new()
            .name(String::from("cropdeck-clipboard"))
            .spawn(move || {
                let mut clipboard = None;
                while let Ok(request) = request_receiver.recv() {
                    let pixels = crop_pixels(&request.source, &request.crop, request.output_size);
                    let result = write_image(&mut clipboard, &pixels);
                    if result_sender.send(result).is_err() {
                        break;
                    }
                    repaint.request_repaint();
                }
            })
            .map_err(ClipboardServiceError::SpawnWorker)?;

        Ok(Self {
            requests: Some(request_sender),
            results,
            worker: Some(worker),
        })
    }

    pub fn copy(&self, request: ClipboardRequest) -> Result<(), ClipboardServiceError> {
        self.requests
            .as_ref()
            .ok_or(ClipboardServiceError::WorkerStopped)?
            .send(request)
            .map_err(|_send_error| ClipboardServiceError::WorkerStopped)
    }

    pub fn poll_result(
        &self,
    ) -> Result<Option<Result<ClipboardReceipt, ClipboardError>>, ClipboardServiceError> {
        match self.results.try_recv() {
            Ok(result) => Ok(Some(result)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(ClipboardServiceError::WorkerStopped),
        }
    }

    pub fn shutdown(mut self) -> Result<(), ClipboardServiceError> {
        self.stop_and_join()
    }

    fn stop_and_join(&mut self) -> Result<(), ClipboardServiceError> {
        self.requests.take();
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_panic| ClipboardServiceError::WorkerPanicked)?;
        }
        Ok(())
    }
}

impl Drop for ClipboardService {
    fn drop(&mut self) {
        let _shutdown_result = self.stop_and_join();
    }
}

fn write_image(
    clipboard: &mut Option<Clipboard>,
    image: &RgbaImage,
) -> Result<ClipboardReceipt, ClipboardError> {
    if clipboard.is_none() {
        *clipboard = Some(Clipboard::new().map_err(ClipboardError::Unavailable)?);
    }
    let written = clipboard
        .as_mut()
        .expect("clipboard handle is present after connecting")
        .set_image(ImageData {
            width: image.width() as usize,
            height: image.height() as usize,
            bytes: Cow::Borrowed(image.as_raw()),
        });
    if let Err(error) = written {
        clipboard.take();
        return Err(ClipboardError::Write(error));
    }

    Ok(ClipboardReceipt::new(image.width(), image.height()))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use image::{Rgba, RgbaImage};
    use tempfile::TempDir;

    use crate::crop::SourceSize;
    use crate::image_io::decode_image;

    use super::*;

    fn backend_available() -> bool {
        if cfg!(target_os = "linux") {
            std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
        } else {
            true
        }
    }

    fn source_fixture() -> (TempDir, SourceImage) {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
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

    fn read_back() -> ImageData<'static> {
        let mut clipboard = Clipboard::new().expect("clipboard should be available");
        clipboard
            .get_image()
            .expect("clipboard should hold an image")
    }

    #[test]
    fn a_copied_crop_can_be_pasted_back_with_its_source_pixels() {
        if !backend_available() {
            return;
        }
        let (_directory, source) = source_fixture();
        let source_size = SourceSize::new(4, 4).expect("fixture source should be valid");
        let crop = CropRect::new(1, 1, 2, 2, source_size).expect("fixture crop should be valid");
        let service = ClipboardService::new(eframe::egui::Context::default())
            .expect("clipboard worker should start");

        service
            .copy(ClipboardRequest::new(source, crop, None))
            .expect("copy should be accepted");
        let deadline = Instant::now() + Duration::from_secs(10);
        let receipt = loop {
            if let Some(result) = service
                .poll_result()
                .expect("clipboard worker should remain available")
            {
                break result.expect("clipboard write should succeed");
            }
            assert!(Instant::now() < deadline, "clipboard result timed out");
            thread::sleep(Duration::from_millis(5));
        };
        let pasted = read_back();

        assert_eq!((receipt.width(), receipt.height()), (2, 2));
        assert_eq!((pasted.width, pasted.height), (2, 2));
        assert_eq!(&pasted.bytes[0..4], &[40, 40, 100, 255]);
        assert_eq!(&pasted.bytes[12..16], &[80, 80, 100, 255]);
        service.shutdown().expect("worker should stop cleanly");
    }

    #[test]
    fn a_copied_crop_honors_the_configured_output_size() {
        let (_directory, source) = source_fixture();
        let source_size = SourceSize::new(4, 4).expect("fixture source should be valid");
        let crop = CropRect::new(0, 0, 2, 2, source_size).expect("fixture crop should be valid");
        let size = OutputSize::new(6, 8).expect("fixture dimensions should be valid");

        let pixels = crop_pixels(&source, &crop, Some(size));

        assert_eq!((pixels.width(), pixels.height()), (6, 8));
    }

    #[test]
    fn a_stopped_worker_reports_instead_of_panicking() {
        let mut service = ClipboardService::new(eframe::egui::Context::default())
            .expect("clipboard worker should start");
        let (_directory, source) = source_fixture();
        let source_size = SourceSize::new(4, 4).expect("fixture source should be valid");
        let crop = CropRect::new(0, 0, 2, 2, source_size).expect("fixture crop should be valid");
        service.stop_and_join().expect("worker should stop cleanly");

        let error = service
            .copy(ClipboardRequest::new(source, crop, None))
            .expect_err("a stopped worker must reject copies");

        assert!(matches!(error, ClipboardServiceError::WorkerStopped));
    }
}
