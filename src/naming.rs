use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDate};
use thiserror::Error;

use crate::crop::AspectRatio;

const MAX_INDEX_WIDTH: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputDimensions {
    width: u32,
    height: u32,
}

impl OutputDimensions {
    pub fn new(width: u32, height: u32) -> Result<Self, FilenameContextError> {
        if width == 0 || height == 0 {
            return Err(FilenameContextError::InvalidDimensions { width, height });
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilenameContext {
    source: String,
    folder: String,
    index: u64,
    ratio: AspectRatio,
    dimensions: OutputDimensions,
    date: NaiveDate,
}

impl FilenameContext {
    pub fn new(
        source: impl Into<String>,
        folder: impl Into<String>,
        index: u64,
        ratio: AspectRatio,
        dimensions: OutputDimensions,
    ) -> Self {
        Self {
            source: source.into(),
            folder: folder.into(),
            index,
            ratio,
            dimensions,
            date: Local::now().date_naive(),
        }
    }

    pub fn from_source_path(
        source_path: &Path,
        index: u64,
        ratio: AspectRatio,
        dimensions: OutputDimensions,
    ) -> Result<Self, FilenameContextError> {
        let source = source_path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| FilenameContextError::MissingSourceName(source_path.to_owned()))?;
        let folder = source_path
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .unwrap_or("folder");
        Ok(Self::new(source, folder, index, ratio, dimensions))
    }

    #[must_use]
    pub fn with_date(mut self, date: NaiveDate) -> Self {
        self.date = date;
        self
    }

    #[must_use]
    pub const fn index(&self) -> u64 {
        self.index
    }

    fn set_index(&mut self, index: u64) {
        self.index = index;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilenameTemplate {
    source: String,
    segments: Vec<Segment>,
}

impl FilenameTemplate {
    pub fn parse(template: &str) -> Result<Self, TemplateError> {
        if template.is_empty() {
            return Err(TemplateError::Empty);
        }

        let mut segments = Vec::new();
        let mut literal_start = 0;
        let mut cursor = 0;
        let mut has_index = false;
        while cursor < template.len() {
            match template.as_bytes()[cursor] {
                b'{' => {
                    push_literal(&mut segments, template, literal_start, cursor)?;
                    let relative_end = template[cursor + 1..]
                        .find('}')
                        .ok_or(TemplateError::UnterminatedToken { position: cursor })?;
                    let end = cursor + 1 + relative_end;
                    let segment = parse_token(&template[cursor + 1..end], cursor)?;
                    has_index |= matches!(segment, Segment::Index { .. });
                    segments.push(segment);
                    cursor = end + 1;
                    literal_start = cursor;
                }
                b'}' => {
                    return Err(TemplateError::UnexpectedClosingBrace { position: cursor });
                }
                _ => cursor += 1,
            }
        }
        push_literal(&mut segments, template, literal_start, template.len())?;
        if !has_index {
            return Err(TemplateError::MissingIndex);
        }
        Ok(Self {
            source: template.to_owned(),
            segments,
        })
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.source
    }

    #[must_use]
    pub fn render(&self, context: &FilenameContext) -> String {
        let mut output = String::with_capacity(self.source.len() + 32);
        self.segments.iter().for_each(|segment| match segment {
            Segment::Literal(value) => output.push_str(value),
            Segment::Source => push_safe_component(&mut output, &context.source, "source"),
            Segment::Index { width } => {
                if let Some(width) = width {
                    write!(&mut output, "{:0width$}", context.index, width = width)
                        .expect("writing to a String cannot fail");
                } else {
                    write!(&mut output, "{}", context.index)
                        .expect("writing to a String cannot fail");
                }
            }
            Segment::Ratio => {
                write!(
                    &mut output,
                    "{}x{}",
                    context.ratio.width(),
                    context.ratio.height()
                )
                .expect("writing to a String cannot fail");
            }
            Segment::Width => {
                write!(&mut output, "{}", context.dimensions.width())
                    .expect("writing to a String cannot fail");
            }
            Segment::Height => {
                write!(&mut output, "{}", context.dimensions.height())
                    .expect("writing to a String cannot fail");
            }
            Segment::Date => {
                write!(&mut output, "{}", context.date.format("%Y-%m-%d"))
                    .expect("writing to a String cannot fail");
            }
            Segment::Folder => push_safe_component(&mut output, &context.folder, "folder"),
        });
        output
    }

    pub fn resolve_available(
        &self,
        directory: &Path,
        extension: &str,
        context: &FilenameContext,
    ) -> Result<ResolvedFilename, NamingError> {
        let extension = validate_extension(extension)?;
        let mut candidate_context = context.clone();
        loop {
            let stem = self.render(&candidate_context);
            let path = directory.join(format!("{stem}.{extension}"));
            if !path
                .try_exists()
                .map_err(|source| NamingError::CheckCollision {
                    path: path.clone(),
                    source,
                })?
            {
                return Ok(ResolvedFilename {
                    path,
                    index: candidate_context.index,
                });
            }
            let index = candidate_context
                .index
                .checked_add(1)
                .ok_or(NamingError::IndexExhausted)?;
            candidate_context.set_index(index);
        }
    }
}

impl std::str::FromStr for FilenameTemplate {
    type Err = TemplateError;

    fn from_str(template: &str) -> Result<Self, Self::Err> {
        Self::parse(template)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedFilename {
    path: PathBuf,
    index: u64,
}

impl ResolvedFilename {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn into_path(self) -> PathBuf {
        self.path
    }

    #[must_use]
    pub const fn index(&self) -> u64 {
        self.index
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Literal(String),
    Source,
    Index { width: Option<usize> },
    Ratio,
    Width,
    Height,
    Date,
    Folder,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TemplateError {
    #[error("filename template cannot be empty")]
    Empty,

    #[error("unterminated filename token at byte {position}")]
    UnterminatedToken { position: usize },

    #[error("unexpected closing brace at byte {position}")]
    UnexpectedClosingBrace { position: usize },

    #[error("unknown filename token `{token}` at byte {position}")]
    UnknownToken { token: String, position: usize },

    #[error("invalid index format `{format}` at byte {position}; expected digits such as `03`")]
    InvalidIndexFormat { format: String, position: usize },

    #[error("index width {width} exceeds the maximum of {MAX_INDEX_WIDTH}")]
    IndexWidthTooLarge { width: usize },

    #[error("filename template must contain an `{{index}}` token")]
    MissingIndex,

    #[error(
        "filename template contains invalid literal character `{character}` at byte {position}"
    )]
    InvalidLiteralCharacter { character: char, position: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FilenameContextError {
    #[error("filename dimensions must be non-zero, received {width}x{height}")]
    InvalidDimensions { width: u32, height: u32 },

    #[error("source path has no usable file name: {0}")]
    MissingSourceName(PathBuf),
}

#[derive(Debug, Error)]
pub enum NamingError {
    #[error("invalid output extension `{0}`")]
    InvalidExtension(String),

    #[error("failed to check output path {path}: {source}")]
    CheckCollision {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("filename index space is exhausted")]
    IndexExhausted,
}

fn push_literal(
    segments: &mut Vec<Segment>,
    template: &str,
    start: usize,
    end: usize,
) -> Result<(), TemplateError> {
    if start == end {
        return Ok(());
    }
    let literal = &template[start..end];
    if let Some((relative_position, character)) = literal
        .char_indices()
        .find(|(_, character)| invalid_literal_character(*character))
    {
        return Err(TemplateError::InvalidLiteralCharacter {
            character,
            position: start + relative_position,
        });
    }
    segments.push(Segment::Literal(literal.to_owned()));
    Ok(())
}

fn parse_token(token: &str, position: usize) -> Result<Segment, TemplateError> {
    let (name, format) = token
        .split_once(':')
        .map_or((token, None), |(name, format)| (name, Some(format)));
    match (name, format) {
        ("source", None) => Ok(Segment::Source),
        ("index", None) => Ok(Segment::Index { width: None }),
        ("index", Some(format)) => parse_index_width(format, position),
        ("ratio", None) => Ok(Segment::Ratio),
        ("width", None) => Ok(Segment::Width),
        ("height", None) => Ok(Segment::Height),
        ("date", None) => Ok(Segment::Date),
        ("folder", None) => Ok(Segment::Folder),
        ("source" | "ratio" | "width" | "height" | "date" | "folder", Some(format)) => {
            Err(TemplateError::InvalidIndexFormat {
                format: format.to_owned(),
                position,
            })
        }
        _ => Err(TemplateError::UnknownToken {
            token: token.to_owned(),
            position,
        }),
    }
}

fn parse_index_width(format: &str, position: usize) -> Result<Segment, TemplateError> {
    if format.is_empty() || !format.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(TemplateError::InvalidIndexFormat {
            format: format.to_owned(),
            position,
        });
    }
    let width = format
        .parse::<usize>()
        .map_err(|_| TemplateError::InvalidIndexFormat {
            format: format.to_owned(),
            position,
        })?;
    if width == 0 {
        return Err(TemplateError::InvalidIndexFormat {
            format: format.to_owned(),
            position,
        });
    }
    if width > MAX_INDEX_WIDTH {
        return Err(TemplateError::IndexWidthTooLarge { width });
    }
    Ok(Segment::Index { width: Some(width) })
}

fn invalid_literal_character(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*'
        )
}

fn push_safe_component(output: &mut String, value: &str, fallback: &str) {
    let start = output.len();
    value.chars().for_each(|character| {
        if invalid_literal_character(character) {
            output.push('_');
        } else {
            output.push(character);
        }
    });
    if output.len() == start {
        output.push_str(fallback);
    }
}

fn validate_extension(extension: &str) -> Result<&str, NamingError> {
    let extension = extension.strip_prefix('.').unwrap_or(extension);
    if extension.is_empty() || !extension.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(NamingError::InvalidExtension(extension.to_owned()));
    }
    Ok(extension)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn context(index: u64) -> FilenameContext {
        let dimensions = OutputDimensions::new(720, 1280).expect("dimensions should be valid");
        let date = NaiveDate::from_ymd_opt(2026, 9, 2).expect("date should be valid");
        FilenameContext::new(
            "chapter_041",
            "volume_2",
            index,
            AspectRatio::PORTRAIT_9_16,
            dimensions,
        )
        .with_date(date)
    }

    #[test]
    fn every_supported_token_is_rendered() {
        let template =
            FilenameTemplate::parse("{folder}_{source}_{ratio}_{width}x{height}_{date}_{index:04}")
                .expect("template should compile");

        let rendered = template.render(&context(7));

        assert_eq!(
            rendered,
            "volume_2_chapter_041_9x16_720x1280_2026-09-02_0007"
        );
    }

    #[test]
    fn malformed_and_unknown_tokens_are_rejected() {
        assert!(matches!(
            FilenameTemplate::parse("{source}_{index"),
            Err(TemplateError::UnterminatedToken { .. })
        ));
        assert!(matches!(
            FilenameTemplate::parse("{unknown}_{index}"),
            Err(TemplateError::UnknownToken { .. })
        ));
        assert!(matches!(
            FilenameTemplate::parse("{source}"),
            Err(TemplateError::MissingIndex)
        ));
    }

    #[test]
    fn path_separators_are_rejected_in_literals_and_sanitized_in_values() {
        assert!(matches!(
            FilenameTemplate::parse("bad/{index}"),
            Err(TemplateError::InvalidLiteralCharacter { .. })
        ));
        let template =
            FilenameTemplate::parse("{source}_{index}").expect("template should compile");
        let dimensions = OutputDimensions::new(10, 10).expect("dimensions should be valid");
        let context =
            FilenameContext::new("unsafe/name", "folder", 1, AspectRatio::SQUARE, dimensions);

        assert_eq!(template.render(&context), "unsafe_name_1");
    }

    #[test]
    fn collision_resolution_increments_until_free() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        fs::write(directory.path().join("chapter_041_001.webp"), [])
            .expect("collision should be created");
        fs::write(directory.path().join("chapter_041_002.webp"), [])
            .expect("collision should be created");
        let template =
            FilenameTemplate::parse("{source}_{index:03}").expect("template should compile");

        let resolved = template
            .resolve_available(directory.path(), "webp", &context(1))
            .expect("a free path should be found");

        assert_eq!(resolved.index(), 3);
        assert_eq!(
            resolved.path(),
            directory.path().join("chapter_041_003.webp")
        );
    }

    #[test]
    fn extension_cannot_escape_the_destination() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let template =
            FilenameTemplate::parse("{source}_{index}").expect("template should compile");

        let error = template
            .resolve_available(directory.path(), "../png", &context(1))
            .expect_err("extension should be rejected");

        assert!(matches!(error, NamingError::InvalidExtension(_)));
    }

    #[test]
    fn source_path_context_uses_file_stem_and_parent_folder() {
        let dimensions = OutputDimensions::new(100, 200).expect("dimensions should be valid");

        let context = FilenameContext::from_source_path(
            Path::new("book/chapter_001.webp"),
            4,
            AspectRatio::PORTRAIT_2_3,
            dimensions,
        )
        .expect("context should be derived");
        let template =
            FilenameTemplate::parse("{folder}_{source}_{index}").expect("template should compile");

        assert_eq!(template.render(&context), "book_chapter_001_4");
    }
}
