use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Args;
use image::RgbImage;
use thiserror::Error;

const ENVIRONMENT_EXIT: u8 = 3;
const NO_WINDOW_EXIT: u8 = 4;
const WHOLE_DESKTOP_EXIT: u8 = 5;
const TOO_SMALL_EXIT: u8 = 6;
const OBSCURED_EXIT: u8 = 7;

#[derive(Debug, Args)]
pub struct ScreenshotArgs {
    #[arg(
        long,
        default_value = "CropDeck",
        help = "Title of the window to capture"
    )]
    pub name: String,

    #[arg(
        long,
        default_value = "screenshots/cropdeck.png",
        help = "PNG path to write"
    )]
    pub output: PathBuf,

    #[arg(
        long,
        default_value_t = 200,
        help = "Reject a window narrower or shorter than this many pixels"
    )]
    pub min_side: u16,

    #[arg(
        long,
        help = "Raise and focus the window first, which takes focus from the user"
    )]
    pub activate: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl WindowRect {
    #[must_use]
    pub const fn overlaps(self, other: Self) -> bool {
        self.x < other.x + other.width as i32
            && other.x < self.x + self.width as i32
            && self.y < other.y + other.height as i32
            && other.y < self.y + self.height as i32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Error)]
pub enum CaptureRefusal {
    #[error("no window named {0} is mapped")]
    NoWindow(String),

    #[error(
        "window {name} is covered by {covered_by}; X11 captures whatever is on screen, so raise or close that window first, or pass --activate"
    )]
    Obscured { name: String, covered_by: String },

    #[error("window {name} is {width}x{height}, the whole desktop, not one window")]
    WholeDesktop {
        name: String,
        width: u32,
        height: u32,
    },

    #[error("window {name} is only {width}x{height}, smaller than --min-side {minimum}")]
    TooSmall {
        name: String,
        width: u32,
        height: u32,
        minimum: u16,
    },

    #[error("window pixels are {depth} bits deep, which this task cannot convert")]
    UnsupportedDepth { depth: u8 },
}

impl CaptureRefusal {
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::NoWindow(_) => NO_WINDOW_EXIT,
            Self::Obscured { .. } => OBSCURED_EXIT,
            Self::WholeDesktop { .. } => WHOLE_DESKTOP_EXIT,
            Self::TooSmall { .. } => TOO_SMALL_EXIT,
            Self::UnsupportedDepth { .. } => ENVIRONMENT_EXIT,
        }
    }
}

#[derive(Debug, Error)]
pub enum ScreenshotError {
    #[error(transparent)]
    Refused(#[from] CaptureRefusal),

    #[error(transparent)]
    Environment(#[from] anyhow::Error),
}

pub fn verify(
    name: &str,
    rect: WindowRect,
    screen: ScreenSize,
    minimum_side: u16,
) -> Result<(), CaptureRefusal> {
    if rect.width >= screen.width && rect.height >= screen.height {
        return Err(CaptureRefusal::WholeDesktop {
            name: name.to_owned(),
            width: rect.width,
            height: rect.height,
        });
    }
    if rect.width < u32::from(minimum_side) || rect.height < u32::from(minimum_side) {
        return Err(CaptureRefusal::TooSmall {
            name: name.to_owned(),
            width: rect.width,
            height: rect.height,
            minimum: minimum_side,
        });
    }
    Ok(())
}

pub fn rgb_image(
    depth: u8,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<RgbImage, CaptureRefusal> {
    if !matches!(depth, 24 | 32) {
        return Err(CaptureRefusal::UnsupportedDepth { depth });
    }
    let expected = width as usize * height as usize * 4;
    if pixels.len() < expected {
        return Err(CaptureRefusal::UnsupportedDepth { depth });
    }
    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    for pixel in pixels[..expected].chunks_exact(4) {
        rgb.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
    }
    RgbImage::from_raw(width, height, rgb).ok_or(CaptureRefusal::UnsupportedDepth { depth })
}

pub fn summary(output: &Path, rect: WindowRect, name: &str, screen: ScreenSize) -> String {
    format!(
        "{} {}x{} window '{name}' on {}x{} desktop",
        output.display(),
        rect.width,
        rect.height,
        screen.width,
        screen.height
    )
}

pub fn run(arguments: &ScreenshotArgs) -> ExitCode {
    match capture(arguments) {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(ScreenshotError::Refused(refusal)) => {
            eprintln!("screenshot: {refusal}");
            ExitCode::from(refusal.exit_code())
        }
        Err(ScreenshotError::Environment(error)) => {
            eprintln!("screenshot: {error:#}");
            ExitCode::from(ENVIRONMENT_EXIT)
        }
    }
}

#[cfg(unix)]
fn capture(arguments: &ScreenshotArgs) -> Result<String, ScreenshotError> {
    use crate::x11_capture::Desktop;

    let desktop = Desktop::connect()?;
    let window = desktop
        .find_window(&arguments.name)?
        .ok_or_else(|| CaptureRefusal::NoWindow(arguments.name.clone()))?;
    if arguments.activate {
        desktop.activate(window)?;
    }
    let rect = desktop.rect(window)?;
    if let Some(covered_by) = desktop.occluder(window, rect)? {
        return Err(CaptureRefusal::Obscured {
            name: arguments.name.clone(),
            covered_by,
        }
        .into());
    }
    let screen = desktop.screen_size();
    verify(&arguments.name, rect, screen, arguments.min_side)?;

    let pixels = desktop.pixels(window, rect)?;
    let image = rgb_image(pixels.depth, rect.width, rect.height, &pixels.bytes)?;
    write_png(&image, &arguments.output)?;

    Ok(summary(&arguments.output, rect, &arguments.name, screen))
}

#[cfg(not(unix))]
fn capture(_arguments: &ScreenshotArgs) -> Result<String, ScreenshotError> {
    Err(anyhow::anyhow!("window capture is implemented for X11 sessions only").into())
}

fn write_png(image: &RgbImage, output: &Path) -> Result<(), anyhow::Error> {
    use anyhow::Context;

    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    image
        .save(output)
        .with_context(|| format!("could not write {}", output.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: ScreenSize = ScreenSize {
        width: 5_120,
        height: 1_440,
    };

    fn rect(x: i32, y: i32, width: u32, height: u32) -> WindowRect {
        WindowRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn touching_windows_do_not_count_as_overlapping() {
        let window = rect(100, 100, 800, 600);

        assert!(window.overlaps(rect(899, 699, 10, 10)));
        assert!(!window.overlaps(rect(900, 100, 10, 10)));
        assert!(!window.overlaps(rect(100, 700, 10, 10)));
        assert!(window.overlaps(rect(-50, -50, 400, 400)));
    }

    #[test]
    fn a_capture_the_size_of_the_desktop_is_refused() {
        let error = verify("CropDeck", rect(0, 0, 5_120, 1_440), SCREEN, 200)
            .expect_err("a desktop sized capture must be refused");

        assert!(matches!(error, CaptureRefusal::WholeDesktop { .. }));
        assert_eq!(error.exit_code(), WHOLE_DESKTOP_EXIT);
    }

    #[test]
    fn a_window_smaller_than_the_minimum_side_is_refused() {
        let error = verify("CropDeck", rect(0, 0, 828, 120), SCREEN, 200)
            .expect_err("a short window must be refused");

        assert!(matches!(
            error,
            CaptureRefusal::TooSmall {
                height: 120,
                minimum: 200,
                ..
            }
        ));
        assert_eq!(error.exit_code(), TOO_SMALL_EXIT);
    }

    #[test]
    fn an_ordinary_window_passes_verification() {
        assert!(verify("CropDeck", rect(104, 70, 828, 666), SCREEN, 200).is_ok());
    }

    #[test]
    fn pixels_convert_from_the_server_layout_without_reordering_rows() {
        let pixels = [
            10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255,
        ];

        let image = rgb_image(24, 2, 2, &pixels).expect("24 bit pixels should convert");

        assert_eq!(image.get_pixel(0, 0).0, [30, 20, 10]);
        assert_eq!(image.get_pixel(1, 1).0, [120, 110, 100]);
    }

    #[test]
    fn an_unsupported_depth_and_a_short_buffer_are_reported() {
        assert!(matches!(
            rgb_image(16, 1, 1, &[0; 4]),
            Err(CaptureRefusal::UnsupportedDepth { depth: 16 })
        ));
        assert!(matches!(
            rgb_image(24, 4, 4, &[0; 8]),
            Err(CaptureRefusal::UnsupportedDepth { depth: 24 })
        ));
    }

    #[test]
    fn the_summary_names_the_file_the_window_and_the_desktop() {
        assert_eq!(
            summary(
                Path::new("screenshots/cropdeck.png"),
                rect(104, 70, 828, 666),
                "CropDeck",
                SCREEN
            ),
            "screenshots/cropdeck.png 828x666 window 'CropDeck' on 5120x1440 desktop"
        );
    }

    #[test]
    fn refusals_keep_their_documented_exit_codes() {
        assert_eq!(
            CaptureRefusal::NoWindow(String::from("CropDeck")).exit_code(),
            NO_WINDOW_EXIT
        );
        assert_eq!(
            CaptureRefusal::Obscured {
                name: String::from("CropDeck"),
                covered_by: String::from("Files"),
            }
            .exit_code(),
            OBSCURED_EXIT
        );
        assert_eq!(
            CaptureRefusal::UnsupportedDepth { depth: 16 }.exit_code(),
            ENVIRONMENT_EXIT
        );
    }
}
