use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender};
use thiserror::Error;

use crate::image_io::{ImageIoError, SourceImage, decode_image};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadPriority {
    Current,
    Prefetch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadRequest {
    path: PathBuf,
    generation: u64,
    priority: LoadPriority,
}

#[derive(Debug)]
pub struct LoadResult {
    path: PathBuf,
    generation: u64,
    outcome: Result<SourceImage, ImageIoError>,
}

impl LoadResult {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn into_outcome(self) -> Result<SourceImage, ImageIoError> {
        self.outcome
    }
}

#[derive(Debug, Error)]
pub enum LoaderError {
    #[error("image loader worker count must be greater than zero")]
    InvalidWorkerCount,

    #[error("failed to start image loader worker: {0}")]
    SpawnWorker(#[source] std::io::Error),

    #[error("image loader workers have stopped")]
    WorkersStopped,

    #[error("image load generations are exhausted")]
    GenerationExhausted,

    #[error("an image loader worker panicked")]
    WorkerPanicked,
}

#[derive(Debug)]
enum WorkerResult {
    Loaded(LoadResult),
    Skipped(LoadRequest),
}

pub struct ImageLoader {
    requests: Option<Sender<LoadRequest>>,
    results: Receiver<WorkerResult>,
    latest_generation: Arc<AtomicU64>,
    in_flight: HashSet<PathBuf>,
    cache: SourceCache,
    workers: Vec<JoinHandle<()>>,
}

impl ImageLoader {
    pub fn new(
        worker_count: usize,
        repaint: eframe::egui::Context,
        cache_budget_bytes: usize,
    ) -> Result<Self, LoaderError> {
        if worker_count == 0 {
            return Err(LoaderError::InvalidWorkerCount);
        }

        let (request_sender, request_receiver) = crossbeam_channel::unbounded::<LoadRequest>();
        let (result_sender, result_receiver) = crossbeam_channel::unbounded::<WorkerResult>();
        let latest_generation = Arc::new(AtomicU64::new(0));
        let mut workers = Vec::with_capacity(worker_count);
        for index in 0..worker_count {
            let requests = request_receiver.clone();
            let results = result_sender.clone();
            let worker_generation = Arc::clone(&latest_generation);
            let worker_repaint = repaint.clone();
            let worker = match thread::Builder::new()
                .name(format!("cropdeck-loader-{index}"))
                .spawn(move || {
                    load_worker(requests, results, worker_generation, worker_repaint);
                }) {
                Ok(worker) => worker,
                Err(source) => {
                    drop(request_sender);
                    workers.into_iter().for_each(|worker: JoinHandle<()>| {
                        let _join_result = worker.join();
                    });
                    return Err(LoaderError::SpawnWorker(source));
                }
            };
            workers.push(worker);
        }
        drop(result_sender);

        Ok(Self {
            requests: Some(request_sender),
            results: result_receiver,
            latest_generation,
            in_flight: HashSet::new(),
            cache: SourceCache::new(cache_budget_bytes),
            workers,
        })
    }

    pub fn request(
        &mut self,
        path: PathBuf,
        priority: LoadPriority,
    ) -> Result<Option<SourceImage>, LoaderError> {
        if let Some(source) = self.cache.get(&path) {
            return Ok(Some(source));
        }
        if self.in_flight.contains(&path) {
            return Ok(None);
        }

        let generation = match priority {
            LoadPriority::Current => self
                .latest_generation
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |generation| {
                    generation.checked_add(1)
                })
                .map(|generation| generation + 1)
                .map_err(|_| LoaderError::GenerationExhausted)?,
            LoadPriority::Prefetch => self.latest_generation.load(Ordering::Acquire),
        };
        let request = LoadRequest {
            path: path.clone(),
            generation,
            priority,
        };
        self.in_flight.insert(path.clone());
        let Some(requests) = self.requests.as_ref() else {
            self.in_flight.remove(&path);
            return Err(LoaderError::WorkersStopped);
        };
        if requests.send(request).is_err() {
            self.in_flight.remove(&path);
            return Err(LoaderError::WorkersStopped);
        }
        Ok(None)
    }

    pub fn poll(&mut self) -> Vec<LoadResult> {
        self.results
            .try_iter()
            .filter_map(|result| match result {
                WorkerResult::Loaded(result) => {
                    self.in_flight.remove(&result.path);
                    if let Ok(source) = &result.outcome {
                        self.cache.insert(source.clone());
                    }
                    Some(result)
                }
                WorkerResult::Skipped(request) => {
                    self.in_flight.remove(&request.path);
                    None
                }
            })
            .collect()
    }

    #[must_use]
    pub fn is_loading(&self, path: &Path) -> bool {
        self.in_flight.contains(path)
    }

    #[must_use]
    pub fn cached(&self, path: &Path) -> Option<SourceImage> {
        self.cache.peek(path)
    }

    pub fn shutdown(mut self) -> Result<(), LoaderError> {
        self.stop_and_join()
    }

    fn stop_and_join(&mut self) -> Result<(), LoaderError> {
        self.requests.take();
        let mut worker_panicked = false;
        for worker in self.workers.drain(..) {
            worker_panicked |= worker.join().is_err();
        }
        if worker_panicked {
            return Err(LoaderError::WorkerPanicked);
        }
        Ok(())
    }
}

impl Drop for ImageLoader {
    fn drop(&mut self) {
        let _shutdown_result = self.stop_and_join();
    }
}

fn load_worker(
    requests: Receiver<LoadRequest>,
    results: Sender<WorkerResult>,
    latest_generation: Arc<AtomicU64>,
    repaint: eframe::egui::Context,
) {
    while let Ok(request) = requests.recv() {
        if is_stale(&request, latest_generation.load(Ordering::Acquire)) {
            if results.send(WorkerResult::Skipped(request)).is_err() {
                break;
            }
            continue;
        }
        let outcome = decode_image(&request.path);
        let result = LoadResult {
            path: request.path,
            generation: request.generation,
            outcome,
        };
        if results.send(WorkerResult::Loaded(result)).is_err() {
            break;
        }
        repaint.request_repaint();
    }
}

#[must_use]
pub fn is_stale(request: &LoadRequest, latest_generation: u64) -> bool {
    match request.priority {
        LoadPriority::Current => request.generation < latest_generation,
        LoadPriority::Prefetch => request.generation.saturating_add(1) < latest_generation,
    }
}

struct SourceCache {
    entries: VecDeque<SourceImage>,
    budget_bytes: usize,
    used_bytes: usize,
}

impl SourceCache {
    fn new(budget_bytes: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            budget_bytes,
            used_bytes: 0,
        }
    }

    fn insert(&mut self, image: SourceImage) {
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.path() == image.path())
            && let Some(replaced) = self.entries.remove(index)
        {
            self.used_bytes -= replaced.byte_len();
        }
        self.used_bytes = self.used_bytes.saturating_add(image.byte_len());
        self.entries.push_back(image);
        while self.used_bytes > self.budget_bytes && self.entries.len() > 1 {
            if let Some(evicted) = self.entries.pop_front() {
                self.used_bytes -= evicted.byte_len();
            }
        }
    }

    fn get(&mut self, path: &Path) -> Option<SourceImage> {
        let index = self.entries.iter().position(|entry| entry.path() == path)?;
        let image = self.entries.remove(index)?;
        let result = image.clone();
        self.entries.push_back(image);
        Some(result)
    }

    fn peek(&self, path: &Path) -> Option<SourceImage> {
        self.entries
            .iter()
            .find(|entry| entry.path() == path)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use image::{Rgb, RgbImage};
    use tempfile::tempdir;

    use crate::image_io::SourcePixels;

    use super::*;

    fn source(path: &str, byte_len: usize) -> SourceImage {
        let width = u32::try_from(byte_len / 3).expect("fixture width should fit");
        SourceImage::from_pixels(
            PathBuf::from(path),
            SourcePixels::Rgb(RgbImage::from_pixel(width, 1, Rgb([1, 2, 3]))),
        )
    }

    fn request(priority: LoadPriority, generation: u64) -> LoadRequest {
        LoadRequest {
            path: PathBuf::from("fixture.png"),
            generation,
            priority,
        }
    }

    #[test]
    fn stale_requests_respect_priority_and_generation_distance() {
        assert!(!is_stale(&request(LoadPriority::Current, 3), 3));
        assert!(is_stale(&request(LoadPriority::Current, 2), 3));
        assert!(!is_stale(&request(LoadPriority::Prefetch, 3), 3));
        assert!(!is_stale(&request(LoadPriority::Prefetch, 2), 3));
        assert!(is_stale(&request(LoadPriority::Prefetch, 1), 3));
    }

    #[test]
    fn loader_rejects_zero_workers() {
        let result = ImageLoader::new(0, eframe::egui::Context::default(), 1_024);

        assert!(matches!(result, Err(LoaderError::InvalidWorkerCount)));
    }

    #[test]
    fn cache_evicts_the_oldest_entry_and_refreshes_recency() {
        let mut cache = SourceCache::new(12);
        cache.insert(source("first.png", 6));
        cache.insert(source("second.png", 6));
        assert!(cache.get(Path::new("first.png")).is_some());

        cache.insert(source("third.png", 6));

        assert!(cache.get(Path::new("first.png")).is_some());
        assert!(cache.get(Path::new("second.png")).is_none());
        assert!(cache.get(Path::new("third.png")).is_some());
    }

    #[test]
    fn cache_never_evicts_its_newest_oversized_entry() {
        let mut cache = SourceCache::new(3);

        cache.insert(source("oversized.png", 6));

        assert_eq!(cache.entries.len(), 1);
        assert!(cache.get(Path::new("oversized.png")).is_some());
    }

    #[test]
    fn cache_replaces_matching_paths_without_double_counting() {
        let mut cache = SourceCache::new(24);
        cache.insert(source("same.png", 6));

        cache.insert(source("same.png", 12));

        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.used_bytes, 12);
        assert_eq!(
            cache
                .get(Path::new("same.png"))
                .map(|image| image.pixels().byte_len()),
            Some(12)
        );
    }

    #[test]
    fn loader_round_trip_decodes_and_caches_an_image() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("source.png");
        RgbImage::from_pixel(4, 4, Rgb([10, 20, 30]))
            .save(&path)
            .expect("fixture should be written");
        let mut loader = ImageLoader::new(2, eframe::egui::Context::default(), 1_024)
            .expect("loader should start");

        let immediate = loader
            .request(path.clone(), LoadPriority::Current)
            .expect("request should be accepted");
        assert!(immediate.is_none());
        let result = (0..100).find_map(|_| {
            let result = loader.poll().into_iter().next();
            if result.is_none() {
                thread::sleep(Duration::from_millis(5));
            }
            result
        });

        let result = result.expect("loader should finish within the bounded wait");
        assert_eq!(result.path(), path);
        assert!(result.into_outcome().is_ok());
        assert!(loader.cached(&path).is_some());
        loader.shutdown().expect("loader should stop");
    }

    #[test]
    fn duplicate_in_flight_request_does_not_advance_generation() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("missing.png");
        let mut loader = ImageLoader::new(1, eframe::egui::Context::default(), 1_024)
            .expect("loader should start");
        loader
            .request(path.clone(), LoadPriority::Current)
            .expect("first request should be accepted");
        let generation = loader.latest_generation.load(Ordering::Acquire);

        let duplicate = loader
            .request(path.clone(), LoadPriority::Current)
            .expect("duplicate request should be ignored");

        assert!(loader.is_loading(&path));
        assert!(duplicate.is_none());
        assert_eq!(loader.latest_generation.load(Ordering::Acquire), generation);
        loader.shutdown().expect("loader should stop");
    }
}
