use std::num::IntErrorKind;

use eframe::egui::{
    self, Key, Modifiers, Sense,
    text::{CCursor, CCursorRange},
};
use thiserror::Error;

use crate::crop::{CropRect, SourceSize};

use super::CropDeckApp;
use super::icons::{LINK, LINK_BREAK};

const COORDINATE_FIELD_PADDING: i8 = 2;
const COORDINATE_FIELD_STROKE: i8 = 1;
const LOCK_GLYPH_SIZE: f32 = 15.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    Horizontal,
    Vertical,
}

impl Axis {
    const ALL: [Self; 2] = [Self::Horizontal, Self::Vertical];

    const fn label(self) -> &'static str {
        match self {
            Self::Horizontal => "x",
            Self::Vertical => "y",
        }
    }

    const fn coordinate(self, crop: CropRect) -> u32 {
        match self {
            Self::Horizontal => crop.x(),
            Self::Vertical => crop.y(),
        }
    }

    fn positioned(self, crop: CropRect, requested: i64, source: SourceSize) -> CropRect {
        match self {
            Self::Horizontal => crop.positioned_at(requested, i64::from(crop.y()), source),
            Self::Vertical => crop.positioned_at(i64::from(crop.x()), requested, source),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
enum CoordinateError {
    #[error("{axis} must be a whole number of pixels, received \"{input}\"")]
    NotAWholeNumber { axis: &'static str, input: String },
}

fn parse_coordinate(input: &str, axis: Axis) -> Result<i64, CoordinateError> {
    let trimmed = input.trim();
    trimmed.parse::<i64>().or_else(|error| match error.kind() {
        IntErrorKind::PosOverflow => Ok(i64::MAX),
        IntErrorKind::NegOverflow => Ok(i64::MIN),
        _ => Err(CoordinateError::NotAWholeNumber {
            axis: axis.label(),
            input: trimmed.to_owned(),
        }),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CoordinateDraft {
    axis: Axis,
    text: String,
    focus_pending: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct CoordinateEditor {
    draft: Option<CoordinateDraft>,
}

impl CoordinateEditor {
    fn begin(&mut self, axis: Axis, value: u32) {
        self.draft = Some(CoordinateDraft {
            axis,
            text: value.to_string(),
            focus_pending: true,
        });
    }

    fn draft_mut(&mut self, axis: Axis) -> Option<&mut CoordinateDraft> {
        self.draft.as_mut().filter(|draft| draft.axis == axis)
    }
}

impl CropDeckApp {
    pub(super) fn crop_position_controls(&mut self, ui: &mut egui::Ui, crop: CropRect) {
        for axis in Axis::ALL {
            self.coordinate_field(ui, axis, crop);
        }
        let locked = self.workspace.location_locked;
        let tooltip = if locked {
            "Crop position is kept when switching images. Click to unlock."
        } else {
            "Click to keep the crop position when switching images"
        };
        let glyph = if locked { LINK } else { LINK_BREAK };
        if ui
            .add(
                egui::Button::new(egui::RichText::new(glyph).size(LOCK_GLYPH_SIZE))
                    .selected(locked),
            )
            .on_hover_text(tooltip)
            .clicked()
        {
            self.workspace.location_locked = !locked;
        }
    }

    fn coordinate_field(&mut self, ui: &mut egui::Ui, axis: Axis, crop: CropRect) {
        ui.monospace(axis.label());
        let Some(draft) = self.coordinate_editor.draft_mut(axis) else {
            let value = egui::Label::new(
                egui::RichText::new(axis.coordinate(crop).to_string()).monospace(),
            )
            .sense(Sense::click());
            if ui
                .add(value)
                .on_hover_text(format!(
                    "Double-click to type an exact {} position",
                    axis.label()
                ))
                .double_clicked()
            {
                self.coordinate_editor.begin(axis, axis.coordinate(crop));
            }
            return;
        };
        let mut output = egui::TextEdit::singleline(&mut draft.text)
            .desired_width(0.0)
            .clip_text(false)
            .frame(coordinate_frame(ui.visuals()))
            .font(egui::TextStyle::Monospace)
            .show(ui);
        let response = output.response.response;
        if std::mem::take(&mut draft.focus_pending) {
            response.request_focus();
            output.state.cursor.set_char_range(Some(CCursorRange::two(
                CCursor::new(0),
                CCursor::new(draft.text.chars().count()),
            )));
            output.state.store(ui.ctx(), response.id);
        }
        let response = response.on_hover_text("Enter applies the position. Esc cancels the edit.");
        if !response.lost_focus() {
            return;
        }
        let cancelled = ui.input_mut(|input| {
            input.consume_key(Modifiers::NONE, Key::Enter);
            input.consume_key(Modifiers::NONE, Key::Escape)
        });
        let Some(draft) = self.coordinate_editor.draft.take() else {
            return;
        };
        if !cancelled {
            self.apply_coordinate(draft.axis, &draft.text);
        }
    }

    fn apply_coordinate(&mut self, axis: Axis, input: &str) {
        let (Some(crop), Some(source)) = (self.workspace.crop, self.source_size()) else {
            return;
        };
        let requested = match parse_coordinate(input, axis) {
            Ok(requested) => requested,
            Err(error) => {
                self.report_error(error.to_string());
                return;
            }
        };
        let moved = axis.positioned(crop, requested, source);
        self.workspace.crop = Some(moved);
        self.workspace.ensure_crop_visible = true;
        let applied = axis.coordinate(moved);
        if i64::from(applied) != requested {
            self.notify(format!(
                "{} limited to {applied} to keep the crop inside the image",
                axis.label()
            ));
        }
    }
}

fn coordinate_frame(visuals: &egui::Visuals) -> egui::Frame {
    let overhang = COORDINATE_FIELD_PADDING + COORDINATE_FIELD_STROKE;
    egui::Frame::NONE
        .fill(visuals.text_edit_bg_color())
        .stroke(egui::Stroke::new(
            f32::from(COORDINATE_FIELD_STROKE),
            visuals.selection.stroke.color,
        ))
        .corner_radius(2)
        .inner_margin(egui::Margin::symmetric(COORDINATE_FIELD_PADDING, 0))
        .outer_margin(egui::Margin::symmetric(-overhang, -COORDINATE_FIELD_STROKE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinates_accept_whole_numbers_with_surrounding_space() {
        assert_eq!(parse_coordinate(" 120 ", Axis::Horizontal), Ok(120));
        assert_eq!(parse_coordinate("+7", Axis::Vertical), Ok(7));
        assert_eq!(parse_coordinate("-15", Axis::Vertical), Ok(-15));
    }

    #[test]
    fn coordinates_reject_text_fractions_and_empty_input() {
        for input in ["", "abc", "12.5", "1e3", "12px"] {
            assert_eq!(
                parse_coordinate(input, Axis::Horizontal),
                Err(CoordinateError::NotAWholeNumber {
                    axis: "x",
                    input: input.trim().to_owned(),
                })
            );
        }
    }

    #[test]
    fn overflowing_coordinates_saturate_instead_of_failing() {
        assert_eq!(
            parse_coordinate("99999999999999999999999", Axis::Vertical),
            Ok(i64::MAX)
        );
        assert_eq!(
            parse_coordinate("-99999999999999999999999", Axis::Vertical),
            Ok(i64::MIN)
        );
    }

    #[test]
    fn each_axis_moves_only_its_own_coordinate() {
        let source = SourceSize::new(1_000, 3_000).expect("source should be valid");
        let crop = CropRect::new(100, 200, 400, 600, source).expect("crop should fit");

        let horizontal = Axis::Horizontal.positioned(crop, 900, source);
        let vertical = Axis::Vertical.positioned(crop, 1_500, source);

        assert_eq!((horizontal.x(), horizontal.y()), (600, 200));
        assert_eq!((vertical.x(), vertical.y()), (100, 1_500));
    }

    #[test]
    fn the_coordinate_frame_occupies_exactly_the_space_of_the_label() {
        let frame = coordinate_frame(&egui::Visuals::dark());

        assert_eq!(frame.total_margin().sum(), egui::Vec2::ZERO);
    }

    #[test]
    fn the_editor_owns_one_axis_draft_at_a_time() {
        let mut editor = CoordinateEditor::default();

        editor.begin(Axis::Horizontal, 42);

        assert!(editor.draft_mut(Axis::Vertical).is_none());
        let draft = editor
            .draft_mut(Axis::Horizontal)
            .expect("the horizontal draft should be active");
        assert_eq!(draft.text, "42");
        assert!(draft.focus_pending);
    }
}
