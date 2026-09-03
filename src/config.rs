use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::crop::AspectRatio;
use crate::naming::FilenameTemplate;

const CONFIG_FILE_NAME: &str = "config.json";
const DEFAULT_FILENAME_TEMPLATE: &str = "{source}_{index:03}";
const MINIMUM_CACHE_BUDGET_MEGABYTES: u32 = 128;
const MAXIMUM_CACHE_BUDGET_MEGABYTES: u32 = 16_384;
const MAXIMUM_RECENT_SOURCES: usize = 10;
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Png,

    Jpeg,

    #[default]
    WebP,
}

impl ExportFormat {
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::WebP => "webp",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportSettings {
    format: ExportFormat,
    quality: u8,
    output_width: Option<u32>,
    output_height: Option<u32>,
    filename_template: String,
    destination: Option<PathBuf>,
}

impl ExportSettings {
    #[must_use]
    pub const fn format(&self) -> ExportFormat {
        self.format
    }

    pub fn set_format(&mut self, format: ExportFormat) {
        self.format = format;
    }

    #[must_use]
    pub const fn quality(&self) -> u8 {
        self.quality
    }

    pub fn set_quality(&mut self, quality: u8) -> Result<(), ConfigValidationError> {
        validate_quality(quality)?;
        self.quality = quality;
        Ok(())
    }

    #[must_use]
    pub const fn output_size(&self) -> Option<(u32, u32)> {
        match (self.output_width, self.output_height) {
            (Some(width), Some(height)) => Some((width, height)),
            (None, None) | (Some(_), None) | (None, Some(_)) => None,
        }
    }

    pub fn set_output_size(
        &mut self,
        output_size: Option<(u32, u32)>,
    ) -> Result<(), ConfigValidationError> {
        if let Some((width, height)) = output_size
            && (width == 0 || height == 0)
        {
            return Err(ConfigValidationError::InvalidOutputSize { width, height });
        }
        (self.output_width, self.output_height) = match output_size {
            Some((width, height)) => (Some(width), Some(height)),
            None => (None, None),
        };
        Ok(())
    }

    #[must_use]
    pub fn filename_template(&self) -> &str {
        &self.filename_template
    }

    pub fn set_filename_template(
        &mut self,
        template: impl Into<String>,
    ) -> Result<(), ConfigValidationError> {
        let template = template.into();
        FilenameTemplate::parse(&template)?;
        self.filename_template = template;
        Ok(())
    }

    #[must_use]
    pub fn destination(&self) -> Option<&Path> {
        self.destination.as_deref()
    }

    pub fn set_destination(&mut self, destination: Option<PathBuf>) {
        self.destination = destination;
    }

    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        validate_quality(self.quality)?;
        match (self.output_width, self.output_height) {
            (Some(width), Some(height)) if width > 0 && height > 0 => {}
            (None, None) => {}
            (Some(width), Some(height)) => {
                return Err(ConfigValidationError::InvalidOutputSize { width, height });
            }
            (Some(_), None) | (None, Some(_)) => {
                return Err(ConfigValidationError::IncompleteOutputSize);
            }
        }
        FilenameTemplate::parse(&self.filename_template)?;
        Ok(())
    }
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            format: ExportFormat::WebP,
            quality: 90,
            output_width: None,
            output_height: None,
            filename_template: DEFAULT_FILENAME_TEMPLATE.to_owned(),
            destination: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Image,
    Folder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentSource {
    path: PathBuf,
    kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    image_count: Option<usize>,
}

impl RecentSource {
    #[must_use]
    pub const fn image(path: PathBuf) -> Self {
        Self {
            path,
            kind: SourceKind::Image,
            image_count: None,
        }
    }

    #[must_use]
    pub const fn folder(path: PathBuf, image_count: usize) -> Self {
        Self {
            path,
            kind: SourceKind::Folder,
            image_count: Some(image_count),
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub const fn kind(&self) -> SourceKind {
        self.kind
    }

    #[must_use]
    pub const fn image_count(&self) -> Option<usize> {
        self.image_count
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    export: ExportSettings,
    aspect_ratio: AspectRatio,
    recursive_scan: bool,
    capture_advance_percent: u8,
    cache_budget_megabytes: u32,
    recent_sources: Vec<RecentSource>,

    #[serde(skip_serializing)]
    last_source: Option<PathBuf>,
}

impl AppConfig {
    #[must_use]
    pub const fn export(&self) -> &ExportSettings {
        &self.export
    }

    pub fn export_mut(&mut self) -> &mut ExportSettings {
        &mut self.export
    }

    #[must_use]
    pub const fn aspect_ratio(&self) -> AspectRatio {
        self.aspect_ratio
    }

    pub fn set_aspect_ratio(&mut self, aspect_ratio: AspectRatio) {
        self.aspect_ratio = aspect_ratio;
    }

    #[must_use]
    pub const fn recursive_scan(&self) -> bool {
        self.recursive_scan
    }

    pub fn set_recursive_scan(&mut self, recursive_scan: bool) {
        self.recursive_scan = recursive_scan;
    }

    #[must_use]
    pub const fn capture_advance_percent(&self) -> u8 {
        self.capture_advance_percent
    }

    pub fn set_capture_advance_percent(
        &mut self,
        percentage: u8,
    ) -> Result<(), ConfigValidationError> {
        validate_advance(percentage)?;
        self.capture_advance_percent = percentage;
        Ok(())
    }

    #[must_use]
    pub const fn cache_budget_megabytes(&self) -> u32 {
        self.cache_budget_megabytes
    }

    pub fn set_cache_budget_megabytes(
        &mut self,
        megabytes: u32,
    ) -> Result<(), ConfigValidationError> {
        validate_cache_budget(megabytes)?;
        self.cache_budget_megabytes = megabytes;
        Ok(())
    }

    #[must_use]
    pub fn recent_sources(&self) -> &[RecentSource] {
        &self.recent_sources
    }

    pub fn push_recent_source(&mut self, source: RecentSource) {
        self.remove_recent_source(&source.path);
        self.recent_sources.insert(0, source);
        self.recent_sources.truncate(MAXIMUM_RECENT_SOURCES);
    }

    pub fn remove_recent_source(&mut self, path: &Path) {
        self.recent_sources.retain(|entry| entry.path != path);
    }

    pub fn clear_recent_sources(&mut self) {
        self.recent_sources.clear();
    }

    fn migrate_last_source(&mut self) {
        if let Some(path) = self.last_source.take()
            && self.recent_sources.is_empty()
        {
            let source = if path.is_dir() {
                RecentSource::folder(path, 0)
            } else {
                RecentSource::image(path)
            };
            self.recent_sources.push(source);
        }
    }

    pub fn path() -> Result<PathBuf, ConfigError> {
        let project = ProjectDirs::from("org", "cropdeck", "CropDeck")
            .ok_or(ConfigError::ConfigDirectoryUnavailable)?;
        Ok(project.config_dir().join(CONFIG_FILE_NAME))
    }

    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from(&Self::path()?)
    }

    pub fn load_or_default() -> Result<Self, ConfigError> {
        let path = Self::path()?;
        if !path.try_exists().map_err(|source| ConfigError::Read {
            path: path.clone(),
            source,
        })? {
            return Ok(Self::default());
        }
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let file = File::open(path).map_err(|source| ConfigError::Read {
            path: path.to_owned(),
            source,
        })?;
        let mut config: Self = serde_json::from_reader(BufReader::new(file)).map_err(|source| {
            ConfigError::Deserialize {
                path: path.to_owned(),
                source,
            }
        })?;
        config.validate()?;
        config.migrate_last_source();
        Ok(config)
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        self.save_to(&Self::path()?)
    }

    pub fn save_to(&self, path: &Path) -> Result<(), ConfigError> {
        self.validate()?;
        let directory = path.parent().ok_or_else(|| ConfigError::MissingParent {
            path: path.to_owned(),
        })?;
        fs::create_dir_all(directory).map_err(|source| ConfigError::CreateDirectory {
            path: directory.to_owned(),
            source,
        })?;
        let (temporary_path, temporary_file) = create_temporary_file(path)?;
        if let Err(error) = write_config(temporary_file, &temporary_path, self)
            .and_then(|()| replace_file(&temporary_path, path))
        {
            let _cleanup_result = fs::remove_file(&temporary_path);
            return Err(error);
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        self.export.validate()?;
        validate_advance(self.capture_advance_percent)?;
        validate_cache_budget(self.cache_budget_megabytes)
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            export: ExportSettings::default(),
            aspect_ratio: AspectRatio::default(),
            recursive_scan: false,
            capture_advance_percent: 85,
            cache_budget_megabytes: 1_024,
            recent_sources: Vec::new(),
            last_source: None,
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigValidationError {
    #[error("export quality must be between 1 and 100, received {0}")]
    InvalidQuality(u8),

    #[error("output width and height must either both be set or both be omitted")]
    IncompleteOutputSize,

    #[error("output dimensions must be non-zero, received {width}x{height}")]
    InvalidOutputSize { width: u32, height: u32 },

    #[error("capture advancement must be between 1 and 100 percent, received {0}")]
    InvalidCaptureAdvance(u8),

    #[error("decoded image cache must be between 128 and 16384 MiB, received {0}")]
    InvalidCacheBudgetMegabytes(u32),

    #[error(transparent)]
    FilenameTemplate(#[from] crate::naming::TemplateError),
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("the platform configuration directory is unavailable")]
    ConfigDirectoryUnavailable,

    #[error("configuration path has no parent directory: {path}")]
    MissingParent { path: PathBuf },

    #[error("failed to create configuration directory {path}: {source}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to read configuration from {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse configuration from {path}: {source}")]
    Deserialize {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("failed to create temporary configuration file {path}: {source}")]
    CreateTemporary {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write temporary configuration file {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to serialize configuration to {path}: {source}")]
    Serialize {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("failed to replace configuration file {path}: {source}")]
    Replace {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(transparent)]
    Validation(#[from] ConfigValidationError),
}

fn validate_quality(quality: u8) -> Result<(), ConfigValidationError> {
    if !(1..=100).contains(&quality) {
        return Err(ConfigValidationError::InvalidQuality(quality));
    }
    Ok(())
}

fn validate_advance(percentage: u8) -> Result<(), ConfigValidationError> {
    if !(1..=100).contains(&percentage) {
        return Err(ConfigValidationError::InvalidCaptureAdvance(percentage));
    }
    Ok(())
}

fn validate_cache_budget(megabytes: u32) -> Result<(), ConfigValidationError> {
    if !(MINIMUM_CACHE_BUDGET_MEGABYTES..=MAXIMUM_CACHE_BUDGET_MEGABYTES).contains(&megabytes) {
        return Err(ConfigValidationError::InvalidCacheBudgetMegabytes(
            megabytes,
        ));
    }
    Ok(())
}

fn create_temporary_file(path: &Path) -> Result<(PathBuf, File), ConfigError> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(CONFIG_FILE_NAME);
    for _attempt in 0..16 {
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary_path = path.with_file_name(format!(
            ".{file_name}.tmp-{}-{sequence}",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
        {
            Ok(file) => return Ok((temporary_path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(ConfigError::CreateTemporary {
                    path: temporary_path,
                    source,
                });
            }
        }
    }
    let temporary_path = path.with_file_name(format!(".{file_name}.tmp"));
    Err(ConfigError::CreateTemporary {
        path: temporary_path,
        source: std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "temporary filename attempts were exhausted",
        ),
    })
}

fn write_config(file: File, path: &Path, config: &AppConfig) -> Result<(), ConfigError> {
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, config).map_err(|source| ConfigError::Serialize {
        path: path.to_owned(),
        source,
    })?;
    writer
        .write_all(b"\n")
        .map_err(|source| ConfigError::Write {
            path: path.to_owned(),
            source,
        })?;
    writer.flush().map_err(|source| ConfigError::Write {
        path: path.to_owned(),
        source,
    })?;
    writer
        .get_ref()
        .sync_all()
        .map_err(|source| ConfigError::Write {
            path: path.to_owned(),
            source,
        })
}

#[cfg(not(windows))]
fn replace_file(temporary_path: &Path, path: &Path) -> Result<(), ConfigError> {
    fs::rename(temporary_path, path).map_err(|source| ConfigError::Replace {
        path: path.to_owned(),
        source,
    })
}

#[cfg(windows)]
fn replace_file(temporary_path: &Path, path: &Path) -> Result<(), ConfigError> {
    if !path.try_exists().map_err(|source| ConfigError::Replace {
        path: path.to_owned(),
        source,
    })? {
        return fs::rename(temporary_path, path).map_err(|source| ConfigError::Replace {
            path: path.to_owned(),
            source,
        });
    }

    let backup_path = path.with_extension("json.replacing");
    let _stale_backup_result = fs::remove_file(&backup_path);
    fs::rename(path, &backup_path).map_err(|source| ConfigError::Replace {
        path: path.to_owned(),
        source,
    })?;
    if let Err(source) = fs::rename(temporary_path, path) {
        let _restore_result = fs::rename(&backup_path, path);
        return Err(ConfigError::Replace {
            path: path.to_owned(),
            source,
        });
    }
    let _cleanup_result = fs::remove_file(backup_path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_generation_friendly() {
        let config = AppConfig::default();

        config.validate().expect("defaults should be valid");

        assert_eq!(config.aspect_ratio(), AspectRatio::PORTRAIT_9_16);
        assert_eq!(config.export().format(), ExportFormat::WebP);
        assert_eq!(config.export().quality(), 90);
        assert_eq!(config.capture_advance_percent(), 85);
        assert_eq!(config.cache_budget_megabytes(), 1_024);
    }

    #[test]
    fn config_round_trips_through_atomic_save() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let path = directory.path().join("nested").join("settings.json");
        let mut expected = AppConfig::default();
        expected.set_recursive_scan(true);
        expected.push_recent_source(RecentSource::image(PathBuf::from("chapter_12.webp")));
        expected
            .export_mut()
            .set_output_size(Some((720, 1280)))
            .expect("dimensions should be valid");

        expected.save_to(&path).expect("config should save");
        let actual = AppConfig::load_from(&path).expect("config should load");

        assert_eq!(actual, expected);
        assert_eq!(actual.export().output_size(), Some((720, 1280)));
    }

    #[test]
    fn recent_sources_dedupe_move_to_front_and_stay_bounded() {
        let mut config = AppConfig::default();
        for index in 0..12 {
            config.push_recent_source(RecentSource::image(PathBuf::from(format!("{index}.png"))));
        }
        config.push_recent_source(RecentSource::folder(PathBuf::from("5.png"), 3));

        let paths: Vec<_> = config
            .recent_sources()
            .iter()
            .map(|entry| entry.path().to_path_buf())
            .collect();
        assert_eq!(paths.len(), MAXIMUM_RECENT_SOURCES);
        assert_eq!(paths[0], PathBuf::from("5.png"));
        assert_eq!(config.recent_sources()[0].kind(), SourceKind::Folder);
        assert_eq!(config.recent_sources()[0].image_count(), Some(3));
        assert_eq!(
            paths
                .iter()
                .filter(|path| path.as_path() == Path::new("5.png"))
                .count(),
            1
        );
        assert!(!paths.contains(&PathBuf::from("1.png")));

        config.remove_recent_source(Path::new("5.png"));
        assert_eq!(config.recent_sources().len(), MAXIMUM_RECENT_SOURCES - 1);
        config.clear_recent_sources();
        assert!(config.recent_sources().is_empty());
    }

    #[test]
    fn legacy_last_source_seeds_the_recent_list_once() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let path = directory.path().join("settings.json");
        fs::write(&path, r#"{"last_source":"chapter_12.webp"}"#)
            .expect("test config should be written");

        let config = AppConfig::load_from(&path).expect("config should load");

        assert_eq!(
            config.recent_sources(),
            &[RecentSource::image(PathBuf::from("chapter_12.webp"))]
        );
        config.save_to(&path).expect("config should save");
        let saved = fs::read_to_string(&path).expect("config should be readable");
        assert!(!saved.contains("last_source"));
    }

    #[test]
    fn missing_config_loads_defaults() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let path = directory.path().join("missing.json");

        let missing = AppConfig::load_from(&path);

        assert!(matches!(missing, Err(ConfigError::Read { .. })));
    }

    #[test]
    fn invalid_deserialized_values_are_rejected() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let path = directory.path().join("settings.json");
        fs::write(
            &path,
            r#"{"export":{"quality":0,"filename_template":"{source}_{index}"}}"#,
        )
        .expect("test config should be written");

        let error = AppConfig::load_from(&path).expect_err("quality should be rejected");

        assert!(matches!(
            error,
            ConfigError::Validation(ConfigValidationError::InvalidQuality(0))
        ));
    }

    #[test]
    fn output_size_requires_two_nonzero_dimensions() {
        let mut settings = ExportSettings::default();

        let error = settings
            .set_output_size(Some((0, 100)))
            .expect_err("zero width should be rejected");

        assert!(matches!(
            error,
            ConfigValidationError::InvalidOutputSize {
                width: 0,
                height: 100
            }
        ));
    }

    #[test]
    fn cache_budget_rejects_values_outside_its_supported_range() {
        let mut config = AppConfig::default();

        let below = config
            .set_cache_budget_megabytes(127)
            .expect_err("small cache should be rejected");
        let above = config
            .set_cache_budget_megabytes(16_385)
            .expect_err("large cache should be rejected");

        assert!(matches!(
            below,
            ConfigValidationError::InvalidCacheBudgetMegabytes(127)
        ));
        assert!(matches!(
            above,
            ConfigValidationError::InvalidCacheBudgetMegabytes(16_385)
        ));
        assert_eq!(config.cache_budget_megabytes(), 1_024);
    }

    #[test]
    fn deserialized_cache_budget_is_validated() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let path = directory.path().join("settings.json");
        fs::write(&path, r#"{"cache_budget_megabytes":127}"#)
            .expect("test config should be written");

        let error = AppConfig::load_from(&path).expect_err("small cache should be rejected");

        assert!(matches!(
            error,
            ConfigError::Validation(ConfigValidationError::InvalidCacheBudgetMegabytes(127))
        ));
    }
}
