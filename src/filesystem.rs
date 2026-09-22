use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender};
use thiserror::Error;

use crate::file_manager::{self, FileManagerError};
use crate::image_io::{ImageIoError, ScanControl, ScanDepth, is_supported_image, scan_images};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PathFacts {
    pub exists: bool,
    pub is_directory: bool,
}

impl PathFacts {
    #[must_use]
    pub fn inspect(path: &Path) -> Self {
        match fs::metadata(path) {
            Ok(metadata) => Self {
                exists: true,
                is_directory: metadata.is_dir(),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(_other) => Self {
                exists: true,
                is_directory: false,
            },
        }
    }
}

#[must_use]
pub fn is_missing_source(error: &ImageIoError) -> bool {
    matches!(
        error,
        ImageIoError::ReadDirectory { source, .. } if source.kind() == std::io::ErrorKind::NotFound
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    SourceImage,
    SourceFolder,
    ExportDestination,
}

#[derive(Debug)]
pub enum FilesystemEvent {
    ScanBatch {
        generation: u64,
        is_folder: bool,
        paths: Vec<PathBuf>,
    },
    ScanFinished {
        generation: u64,
        total: usize,
    },
    ScanFailed {
        generation: u64,
        error: ImageIoError,
    },
    Probed {
        path: PathBuf,
        facts: PathFacts,
    },
    DialogClosed {
        kind: DialogKind,
        path: Option<PathBuf>,
    },
    Revealed {
        path: PathBuf,
        outcome: Result<(), FileManagerError>,
    },
}

#[derive(Debug, Error)]
pub enum FilesystemError {
    #[error("failed to start filesystem worker: {0}")]
    SpawnWorker(#[source] std::io::Error),

    #[error("filesystem workers have stopped")]
    WorkersStopped,

    #[error("filesystem scan generations are exhausted")]
    GenerationExhausted,

    #[error("a filesystem worker panicked")]
    WorkerPanicked,
}

#[derive(Debug)]
struct ScanRequest {
    root: PathBuf,
    depth: ScanDepth,
    generation: u64,
}

#[derive(Debug)]
struct ProbeRequest {
    path: PathBuf,
}

pub struct FilesystemService {
    scan_requests: Option<Sender<ScanRequest>>,
    probe_requests: Option<Sender<ProbeRequest>>,
    events: Receiver<FilesystemEvent>,
    event_sender: Option<Sender<FilesystemEvent>>,
    scan_generation: Arc<AtomicU64>,
    probes_in_flight: HashSet<PathBuf>,
    repaint: eframe::egui::Context,
    workers: Vec<JoinHandle<()>>,
}

impl FilesystemService {
    pub fn new(repaint: eframe::egui::Context) -> Result<Self, FilesystemError> {
        let (scan_sender, scan_receiver) = crossbeam_channel::unbounded::<ScanRequest>();
        let (probe_sender, probe_receiver) = crossbeam_channel::unbounded::<ProbeRequest>();
        let (event_sender, events) = crossbeam_channel::unbounded::<FilesystemEvent>();
        let scan_generation = Arc::new(AtomicU64::new(0));

        let scan_events = event_sender.clone();
        let scan_repaint = repaint.clone();
        let worker_generation = Arc::clone(&scan_generation);
        let scan_worker = thread::Builder::new()
            .name(String::from("cropdeck-fs-scan"))
            .spawn(move || {
                scan_worker(scan_receiver, scan_events, worker_generation, scan_repaint);
            })
            .map_err(FilesystemError::SpawnWorker)?;

        let probe_events = event_sender.clone();
        let probe_repaint = repaint.clone();
        let probe_worker = match thread::Builder::new()
            .name(String::from("cropdeck-fs-probe"))
            .spawn(move || {
                probe_worker(probe_receiver, probe_events, probe_repaint);
            }) {
            Ok(worker) => worker,
            Err(source) => {
                drop(scan_sender);
                let _join_result = scan_worker.join();
                return Err(FilesystemError::SpawnWorker(source));
            }
        };

        Ok(Self {
            scan_requests: Some(scan_sender),
            probe_requests: Some(probe_sender),
            events,
            event_sender: Some(event_sender),
            scan_generation,
            probes_in_flight: HashSet::new(),
            repaint,
            workers: vec![scan_worker, probe_worker],
        })
    }

    pub fn scan(&self, root: PathBuf, depth: ScanDepth) -> Result<u64, FilesystemError> {
        let generation = self
            .scan_generation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |generation| {
                generation.checked_add(1)
            })
            .map(|generation| generation + 1)
            .map_err(|_overflow| FilesystemError::GenerationExhausted)?;
        let requests = self
            .scan_requests
            .as_ref()
            .ok_or(FilesystemError::WorkersStopped)?;
        requests
            .send(ScanRequest {
                root,
                depth,
                generation,
            })
            .map(|()| generation)
            .map_err(|_send_error| FilesystemError::WorkersStopped)
    }

    pub fn cancel_scan(&self) {
        self.scan_generation.fetch_add(1, Ordering::AcqRel);
    }

    pub fn probe(&mut self, path: &Path) -> Result<(), FilesystemError> {
        if self.probes_in_flight.contains(path) {
            return Ok(());
        }
        let requests = self
            .probe_requests
            .as_ref()
            .ok_or(FilesystemError::WorkersStopped)?;
        requests
            .send(ProbeRequest {
                path: path.to_path_buf(),
            })
            .map_err(|_send_error| FilesystemError::WorkersStopped)?;
        self.probes_in_flight.insert(path.to_path_buf());
        Ok(())
    }

    pub fn open_dialog(&self, kind: DialogKind) -> Result<(), FilesystemError> {
        let events = self
            .event_sender
            .as_ref()
            .ok_or(FilesystemError::WorkersStopped)?
            .clone();
        let repaint = self.repaint.clone();
        thread::Builder::new()
            .name(String::from("cropdeck-fs-dialog"))
            .spawn(move || {
                let path = match kind {
                    DialogKind::SourceImage => rfd::FileDialog::new()
                        .add_filter("Images", &["jpg", "jpeg", "png", "webp"])
                        .pick_file(),
                    DialogKind::SourceFolder | DialogKind::ExportDestination => {
                        rfd::FileDialog::new().pick_folder()
                    }
                };
                if events
                    .send(FilesystemEvent::DialogClosed { kind, path })
                    .is_ok()
                {
                    repaint.request_repaint();
                }
            })
            .map(|_handle| ())
            .map_err(FilesystemError::SpawnWorker)
    }

    pub fn reveal(&self, path: PathBuf) -> Result<(), FilesystemError> {
        let events = self
            .event_sender
            .as_ref()
            .ok_or(FilesystemError::WorkersStopped)?
            .clone();
        let repaint = self.repaint.clone();
        thread::Builder::new()
            .name(String::from("cropdeck-fs-reveal"))
            .spawn(move || {
                let outcome = file_manager::reveal(&path);
                if events
                    .send(FilesystemEvent::Revealed { path, outcome })
                    .is_ok()
                {
                    repaint.request_repaint();
                }
            })
            .map(|_handle| ())
            .map_err(FilesystemError::SpawnWorker)
    }

    pub fn poll(&mut self) -> Vec<FilesystemEvent> {
        self.events
            .try_iter()
            .inspect(|event| {
                if let FilesystemEvent::Probed { path, .. } = event {
                    self.probes_in_flight.remove(path);
                }
            })
            .collect()
    }

    pub fn shutdown(mut self) -> Result<(), FilesystemError> {
        self.stop_and_join()
    }

    fn stop_and_join(&mut self) -> Result<(), FilesystemError> {
        self.scan_requests.take();
        self.probe_requests.take();
        self.event_sender.take();
        self.scan_generation.fetch_add(1, Ordering::AcqRel);
        let mut worker_panicked = false;
        for worker in self.workers.drain(..) {
            worker_panicked |= worker.join().is_err();
        }
        if worker_panicked {
            return Err(FilesystemError::WorkerPanicked);
        }
        Ok(())
    }
}

impl Drop for FilesystemService {
    fn drop(&mut self) {
        let _shutdown_result = self.stop_and_join();
    }
}

fn scan_worker(
    requests: Receiver<ScanRequest>,
    events: Sender<FilesystemEvent>,
    latest_generation: Arc<AtomicU64>,
    repaint: eframe::egui::Context,
) {
    while let Ok(request) = requests.recv() {
        if request.generation < latest_generation.load(Ordering::Acquire) {
            continue;
        }
        let mut send = |event| {
            let sent = events.send(event).is_ok();
            if sent {
                repaint.request_repaint();
            }
            sent
        };
        run_scan(&request, &latest_generation, &mut send);
    }
}

fn run_scan<F>(request: &ScanRequest, latest_generation: &AtomicU64, send: &mut F)
where
    F: FnMut(FilesystemEvent) -> bool,
{
    let generation = request.generation;
    let facts = PathFacts::inspect(&request.root);
    if !facts.exists {
        send(FilesystemEvent::ScanFailed {
            generation,
            error: ImageIoError::ReadDirectory {
                path: request.root.clone(),
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            },
        });
        return;
    }

    if !facts.is_directory {
        if !is_supported_image(&request.root) {
            send(FilesystemEvent::ScanFailed {
                generation,
                error: ImageIoError::EmptyQueue,
            });
            return;
        }
        if send(FilesystemEvent::ScanBatch {
            generation,
            is_folder: false,
            paths: vec![request.root.clone()],
        }) {
            send(FilesystemEvent::ScanFinished {
                generation,
                total: 1,
            });
        }
        return;
    }

    let mut total = 0;
    let mut cancelled = false;
    let outcome = scan_images(&request.root, request.depth, &mut |paths| {
        if latest_generation.load(Ordering::Acquire) != generation {
            cancelled = true;
            return ScanControl::Cancel;
        }
        total += paths.len();
        let delivered = send(FilesystemEvent::ScanBatch {
            generation,
            is_folder: true,
            paths,
        });
        if delivered {
            ScanControl::Continue
        } else {
            cancelled = true;
            ScanControl::Cancel
        }
    });

    if cancelled {
        return;
    }
    match outcome {
        Ok(()) if total == 0 => {
            send(FilesystemEvent::ScanFailed {
                generation,
                error: ImageIoError::NoSupportedImages(request.root.clone()),
            });
        }
        Ok(()) => {
            send(FilesystemEvent::ScanFinished { generation, total });
        }
        Err(error) => {
            send(FilesystemEvent::ScanFailed { generation, error });
        }
    }
}

fn probe_worker(
    requests: Receiver<ProbeRequest>,
    events: Sender<FilesystemEvent>,
    repaint: eframe::egui::Context,
) {
    while let Ok(request) = requests.recv() {
        let facts = PathFacts::inspect(&request.path);
        if events
            .send(FilesystemEvent::Probed {
                path: request.path,
                facts,
            })
            .is_err()
        {
            break;
        }
        repaint.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use std::thread::sleep;
    use std::time::Duration;

    use tempfile::tempdir;

    use super::*;

    fn service() -> FilesystemService {
        FilesystemService::new(eframe::egui::Context::default())
            .expect("filesystem service should start")
    }

    fn collect_until<F>(service: &mut FilesystemService, mut satisfied: F) -> Vec<FilesystemEvent>
    where
        F: FnMut(&[FilesystemEvent]) -> bool,
    {
        let mut collected = Vec::new();
        for _attempt in 0..200 {
            collected.extend(service.poll());
            if satisfied(&collected) {
                break;
            }
            sleep(Duration::from_millis(5));
        }
        collected
    }

    fn probed_facts(events: &[FilesystemEvent], wanted: &Path) -> Option<PathFacts> {
        events.iter().find_map(|event| match event {
            FilesystemEvent::Probed { path, facts } if path == wanted => Some(*facts),
            _other => None,
        })
    }

    fn drain_until<F, T>(service: &mut FilesystemService, mut pick: F) -> Option<T>
    where
        F: FnMut(&FilesystemEvent) -> Option<T>,
    {
        let mut found = None;
        collect_until(service, |events| {
            found = events.iter().find_map(&mut pick);
            found.is_some()
        });
        found
    }

    fn write_images(directory: &Path, names: &[&str]) {
        for name in names {
            fs::write(directory.join(name), []).expect("fixture should be written");
        }
    }

    #[test]
    fn inspect_separates_directories_files_and_missing_paths() {
        let directory = tempdir().expect("temporary directory should be created");
        let file = directory.path().join("page1.png");
        fs::write(&file, []).expect("fixture should be written");

        let folder_facts = PathFacts::inspect(directory.path());
        let file_facts = PathFacts::inspect(&file);
        let missing_facts = PathFacts::inspect(&directory.path().join("absent.png"));

        assert!(folder_facts.exists && folder_facts.is_directory);
        assert!(file_facts.exists && !file_facts.is_directory);
        assert!(!missing_facts.exists && !missing_facts.is_directory);
    }

    #[test]
    fn probe_reports_presence_and_absence() {
        let directory = tempdir().expect("temporary directory should be created");
        let present = directory.path().join("page1.png");
        fs::write(&present, []).expect("fixture should be written");
        let missing = directory.path().join("absent.png");
        let mut service = service();

        service.probe(&present).expect("probe should be accepted");
        service.probe(&missing).expect("probe should be accepted");

        let events = collect_until(&mut service, |events| {
            probed_facts(events, &present).is_some() && probed_facts(events, &missing).is_some()
        });

        let present_facts = probed_facts(&events, &present).expect("present probe should return");
        let missing_facts = probed_facts(&events, &missing).expect("missing probe should return");
        assert!(present_facts.exists);
        assert!(!missing_facts.exists);
    }

    #[test]
    fn duplicate_probes_are_ignored_while_one_is_in_flight() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("page1.png");
        fs::write(&path, []).expect("fixture should be written");
        let mut service = service();

        service.probe(&path).expect("probe should be accepted");
        service.probe(&path).expect("probe should be accepted");

        let mut events = collect_until(&mut service, |events| !events.is_empty());
        sleep(Duration::from_millis(50));
        events.extend(service.poll());

        let probes = events
            .iter()
            .filter(|event| matches!(event, FilesystemEvent::Probed { .. }))
            .count();
        assert_eq!(probes, 1);
    }

    #[test]
    fn scanning_a_folder_streams_every_supported_image() {
        let directory = tempdir().expect("temporary directory should be created");
        write_images(directory.path(), &["page1.png", "page2.webp", "notes.txt"]);
        let mut service = service();

        service
            .scan(directory.path().to_path_buf(), ScanDepth::FolderOnly)
            .expect("scan should be accepted");

        let total = drain_until(&mut service, |event| match event {
            FilesystemEvent::ScanFinished { total, .. } => Some(*total),
            _other => None,
        })
        .expect("scan should finish");
        assert_eq!(total, 2);
    }

    #[test]
    fn scanning_a_single_image_reports_a_file_source() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("page1.png");
        fs::write(&path, []).expect("fixture should be written");
        let mut service = service();

        service
            .scan(path.clone(), ScanDepth::FolderOnly)
            .expect("scan should be accepted");

        let batch = drain_until(&mut service, |event| match event {
            FilesystemEvent::ScanBatch {
                is_folder, paths, ..
            } => Some((*is_folder, paths.clone())),
            _other => None,
        })
        .expect("scan should emit a batch");
        assert_eq!(batch, (false, vec![path]));
    }

    #[test]
    fn scanning_an_empty_folder_fails_with_no_supported_images() {
        let directory = tempdir().expect("temporary directory should be created");
        let mut service = service();

        service
            .scan(directory.path().to_path_buf(), ScanDepth::FolderOnly)
            .expect("scan should be accepted");

        let failed = drain_until(&mut service, |event| match event {
            FilesystemEvent::ScanFailed { error, .. } => {
                Some(matches!(error, ImageIoError::NoSupportedImages(_)))
            }
            _other => None,
        })
        .expect("scan should fail");
        assert!(failed);
    }

    #[test]
    fn scanning_an_unsupported_file_fails_with_an_empty_queue() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("notes.txt");
        fs::write(&path, []).expect("fixture should be written");
        let mut service = service();

        service
            .scan(path, ScanDepth::FolderOnly)
            .expect("scan should be accepted");

        let failed = drain_until(&mut service, |event| match event {
            FilesystemEvent::ScanFailed { error, .. } => {
                Some(matches!(error, ImageIoError::EmptyQueue))
            }
            _other => None,
        })
        .expect("scan should fail");
        assert!(failed);
    }

    #[test]
    fn superseded_scans_never_deliver_their_generation() {
        let directory = tempdir().expect("temporary directory should be created");
        write_images(directory.path(), &["page1.png"]);
        let mut service = service();

        let first = service
            .scan(directory.path().to_path_buf(), ScanDepth::FolderOnly)
            .expect("scan should be accepted");
        let second = service
            .scan(directory.path().to_path_buf(), ScanDepth::FolderOnly)
            .expect("scan should be accepted");

        assert!(second > first);
        let finished = drain_until(&mut service, |event| match event {
            FilesystemEvent::ScanFinished { generation, .. } => Some(*generation),
            _other => None,
        })
        .expect("the newest scan should finish");
        assert_eq!(finished, second);
    }

    #[test]
    fn scanning_a_missing_path_reports_it_as_not_found() {
        let directory = tempdir().expect("temporary directory should be created");
        let mut service = service();

        service
            .scan(directory.path().join("absent"), ScanDepth::FolderOnly)
            .expect("scan should be accepted");

        let missing = drain_until(&mut service, |event| match event {
            FilesystemEvent::ScanFailed { error, .. } => Some(is_missing_source(error)),
            _other => None,
        })
        .expect("scan should fail");
        assert!(missing);
    }

    #[test]
    fn an_empty_folder_is_not_reported_as_missing() {
        let directory = tempdir().expect("temporary directory should be created");
        let mut service = service();

        service
            .scan(directory.path().to_path_buf(), ScanDepth::FolderOnly)
            .expect("scan should be accepted");

        let missing = drain_until(&mut service, |event| match event {
            FilesystemEvent::ScanFailed { error, .. } => Some(is_missing_source(error)),
            _other => None,
        })
        .expect("scan should fail");
        assert!(!missing);
    }

    #[test]
    fn streaming_a_nested_tree_assembles_a_naturally_ordered_queue() {
        use crate::image_io::{ImageQueue, sort_paths_naturally};

        let directory = tempdir().expect("temporary directory should be created");
        let mut expected = Vec::new();
        for chapter in 1..=3 {
            let nested = directory.path().join(format!("chapter_{chapter:02}"));
            fs::create_dir(&nested).expect("nested fixture directory should be created");
            for page in [1, 2, 10, 20, 100] {
                let path = nested.join(format!("page{page}.png"));
                fs::write(&path, []).expect("fixture should be written");
                expected.push(path);
            }
        }
        fs::write(directory.path().join("notes.txt"), []).expect("fixture should be written");
        sort_paths_naturally(&mut expected);
        let mut service = service();

        service
            .scan(directory.path().to_path_buf(), ScanDepth::Recursive)
            .expect("scan should be accepted");
        let events = collect_until(&mut service, |events| {
            events
                .iter()
                .any(|event| matches!(event, FilesystemEvent::ScanFinished { .. }))
        });

        let mut queue: Option<ImageQueue> = None;
        let mut current = None;
        let mut total = None;
        for event in &events {
            match event {
                FilesystemEvent::ScanBatch { paths, .. } => match queue.as_mut() {
                    None => {
                        let built = ImageQueue::from_batch(paths.clone())
                            .expect("the first batch should form a queue");
                        current = Some(built.current().to_path_buf());
                        queue = Some(built);
                    }
                    Some(queue) => {
                        let mut batch = paths.clone();
                        sort_paths_naturally(&mut batch);
                        queue.merge_sorted(&batch);
                        assert_eq!(Some(queue.current().to_path_buf()), current);
                    }
                },
                FilesystemEvent::ScanFinished { total: count, .. } => total = Some(*count),
                _other => {}
            }
        }

        let queue = queue.expect("the scan should have produced a queue");
        assert_eq!(total, Some(expected.len()));
        assert_eq!(queue.paths(), expected.as_slice());
        assert_eq!(queue.current(), expected[0]);
    }

    #[test]
    fn service_stops_cleanly() {
        assert!(service().shutdown().is_ok());
    }
}
