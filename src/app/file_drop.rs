use std::path::PathBuf;

use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Stroke, StrokeKind};
use thiserror::Error;

use crate::image_io::is_supported_image;

use super::{CropDeckApp, display_file_name};

const OVERLAY_INSET: f32 = 12.0;
const OVERLAY_TEXT_SIZE: f32 = 21.0;
const OVERLAY_FILL: Color32 = Color32::from_black_alpha(176);
const OVERLAY_ACCENT: Color32 = Color32::from_rgb(255, 194, 72);
const OVERLAY_REJECTION: Color32 = Color32::from_rgb(255, 120, 110);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DroppedSource {
    Scan(PathBuf),
    Images(Vec<PathBuf>),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(super) enum DropRejection {
    #[error("Drop one image, several images, or one folder")]
    NothingUsable,
    #[error("{0} is not a JPEG, PNG, or WebP image")]
    UnsupportedFile(String),
}

pub(super) fn classify_drop(paths: Vec<PathBuf>) -> Result<DroppedSource, DropRejection> {
    if paths.is_empty() {
        return Err(DropRejection::NothingUsable);
    }
    if let [only] = paths.as_slice() {
        return Ok(DroppedSource::Scan(only.clone()));
    }
    if let Some(rejected) = paths.iter().find(|path| !is_supported_image(path)) {
        return Err(DropRejection::UnsupportedFile(display_file_name(rejected)));
    }
    Ok(DroppedSource::Images(paths))
}

fn invitation(outcome: &Result<DroppedSource, DropRejection>) -> (String, Color32) {
    match outcome {
        Ok(DroppedSource::Scan(path)) => {
            (format!("Open {}", display_file_name(path)), OVERLAY_ACCENT)
        }
        Ok(DroppedSource::Images(paths)) => {
            (format!("Open {} images", paths.len()), OVERLAY_ACCENT)
        }
        Err(rejection) => (rejection.to_string(), OVERLAY_REJECTION),
    }
}

fn paint_invitation(context: &egui::Context, outcome: &Result<DroppedSource, DropRejection>) {
    let (text, color) = invitation(outcome);
    let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("cropdeck_file_drop"));
    let rect = context.content_rect();
    let painter = context.layer_painter(layer);
    painter.rect_filled(rect, 0.0, OVERLAY_FILL);
    painter.rect_stroke(
        rect.shrink(OVERLAY_INSET),
        CornerRadius::same(6),
        Stroke::new(2.0, color),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(OVERLAY_TEXT_SIZE),
        color,
    );
}

impl CropDeckApp {
    pub(super) fn file_drop(&mut self, context: &egui::Context) {
        let (hovered, dropped) = context.input(|input| {
            (
                input.raw.hovered_files.len(),
                input
                    .raw
                    .dropped_files
                    .iter()
                    .map(|file| file.path().to_path_buf())
                    .collect::<Vec<PathBuf>>(),
            )
        });
        if !dropped.is_empty() {
            self.apply_drop(classify_drop(dropped));
            return;
        }
        if hovered == 0 {
            return;
        }
        let paths = context.input(|input| {
            input
                .raw
                .hovered_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });
        paint_invitation(context, &classify_drop(paths));
    }

    fn apply_drop(&mut self, outcome: Result<DroppedSource, DropRejection>) {
        match outcome {
            Ok(DroppedSource::Scan(path)) => self.open_source(path),
            Ok(DroppedSource::Images(paths)) => self.open_image_set(paths),
            Err(rejection) => self.report_error(rejection.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths_of(names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|name| PathBuf::from("/deck").join(name))
            .collect()
    }

    #[test]
    fn a_single_item_is_scanned_whether_it_is_a_folder_or_an_image() {
        for name in ["chapter", "page1.png"] {
            assert_eq!(
                classify_drop(paths_of(&[name])),
                Ok(DroppedSource::Scan(PathBuf::from("/deck").join(name)))
            );
        }
    }

    #[test]
    fn several_images_become_one_queue_batch() {
        let images = paths_of(&["page10.webp", "page2.jpg", "cover.png"]);

        assert_eq!(
            classify_drop(images.clone()),
            Ok(DroppedSource::Images(images))
        );
    }

    #[test]
    fn a_mixed_drop_names_the_first_unsupported_item() {
        assert_eq!(
            classify_drop(paths_of(&["page1.png", "notes.txt", "raw.tiff"])),
            Err(DropRejection::UnsupportedFile(String::from("notes.txt")))
        );
        assert_eq!(
            classify_drop(paths_of(&["page1.png", "chapter"])),
            Err(DropRejection::UnsupportedFile(String::from("chapter")))
        );
    }

    #[test]
    fn a_drop_without_paths_invites_a_supported_source() {
        assert_eq!(classify_drop(Vec::new()), Err(DropRejection::NothingUsable));
    }

    #[test]
    fn the_overlay_paints_a_shaded_frame_and_its_label() {
        let context = egui::Context::default();

        context.begin_pass(egui::RawInput::default());
        paint_invitation(&context, &classify_drop(paths_of(&["page1.png"])));
        let mut output = context.end_pass();
        output.textures_delta.clear();

        assert!(output.shapes.len() >= 3);
    }

    #[test]
    fn the_overlay_names_the_source_and_marks_rejections() {
        let accepted = invitation(&classify_drop(paths_of(&["page1.png"])));
        let batch = invitation(&classify_drop(paths_of(&["a.png", "b.png"])));
        let rejected = invitation(&classify_drop(paths_of(&["a.png", "notes.txt"])));

        assert_eq!(accepted, (String::from("Open page1.png"), OVERLAY_ACCENT));
        assert_eq!(batch.0, String::from("Open 2 images"));
        assert_ne!(rejected.1, OVERLAY_ACCENT);
    }
}
