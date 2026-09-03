use eframe::egui::{self, CursorIcon, Event, Key, MouseWheelUnit, PointerButton, Vec2};

use crate::crop::{AspectRatio, CropRect, CropResizeDirection, SourceSize, common_crop_sizes};
use crate::viewport::{SourcePoint, ViewportTransform, edge_auto_scroll_velocity};

use super::CropSizePreference;
use super::crop_commands::{FINE_RESIZE_PIXELS, crop_effective_ratio, matching_tier};
use super::workspace::{HistoryEntry, ResizeWheelState, crop_to_source_rect};

const AUTO_SCROLL_ZONE: f32 = 72.0;
const AUTO_SCROLL_MAXIMUM: f32 = 1_100.0;
const WHEEL_RESIZE_THRESHOLD: f32 = 24.0;
const WHEEL_SNAP_RELEASE_MULTIPLIER: f32 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CropDrag {
    initial_crop: CropRect,
    initial_pointer: SourcePoint,
}

pub(super) struct WorkspaceInteraction<'a> {
    pub(super) source_size: SourceSize,
    pub(super) nominal_ratio: AspectRatio,
    pub(super) effective_ratio: &'a mut AspectRatio,
    pub(super) crop: &'a mut Option<CropRect>,
    pub(super) history: &'a [HistoryEntry],
    pub(super) drag: &'a mut Option<CropDrag>,
    pub(super) resize_wheel: &'a mut ResizeWheelState,
    pub(super) size_preference: &'a mut CropSizePreference,
    pub(super) ensure_crop_visible: &'a mut bool,
}

pub(super) fn interact_with_workspace(
    ui: &egui::Ui,
    response: &egui::Response,
    transform: ViewportTransform,
    state: &mut WorkspaceInteraction<'_>,
) {
    let Some(mut current_crop) = *state.crop else {
        return;
    };
    let pointer = response
        .interact_pointer_pos()
        .or_else(|| ui.input(|input| input.pointer.hover_pos()));
    let mut crop_rect = transform.source_rect_to_display(crop_to_source_rect(current_crop));
    let space_down = ui.input(|input| input.key_down(Key::Space));
    let crop_hovered =
        response.hovered() && pointer.is_some_and(|position| crop_rect.contains(position));

    if response.hovered() {
        let (wheel_delta, resize_modifier) = ui.input_mut(|input| {
            let resize_modifier = input.modifiers.shift && !input.modifiers.command;
            if !resize_modifier {
                return (0.0, false);
            }
            let wheel_delta = input
                .events
                .iter()
                .filter_map(|event| {
                    if let Event::MouseWheel {
                        unit,
                        delta,
                        modifiers,
                        ..
                    } = event
                        && modifiers.shift
                        && !modifiers.command
                    {
                        return Some(normalized_resize_wheel_delta(*unit, *delta));
                    }
                    None
                })
                .sum();
            input.smooth_scroll_delta = Vec2::ZERO;
            (wheel_delta, true)
        });
        if !resize_modifier {
            state.resize_wheel.accumulator = 0.0;
        } else if let Some(direction) =
            wheel_resize_direction(state.resize_wheel, current_crop.width(), wheel_delta)
        {
            let (resized, snapped_width) = magnetically_resized_crop(
                current_crop,
                direction,
                state.nominal_ratio,
                *state.effective_ratio,
                state.source_size,
            );
            current_crop = resized;
            *state.effective_ratio = crop_effective_ratio(resized);
            *state.size_preference = matching_tier(state.nominal_ratio, resized)
                .map_or(CropSizePreference::Automatic, CropSizePreference::Tier);
            state.resize_wheel.latched_width = snapped_width;
            *state.crop = Some(current_crop);
            *state.ensure_crop_visible = true;
            crop_rect = transform.source_rect_to_display(crop_to_source_rect(current_crop));
        }
    } else {
        state.resize_wheel.accumulator = 0.0;
    }

    if crop_hovered {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
        response.clone().on_hover_text_at_pointer(
            "Shift+scroll resizes with magnetic generation-size snapping.",
        );
    }
    if response.drag_started_by(PointerButton::Primary)
        && !space_down
        && let Some(pointer) = pointer
        && crop_rect.contains(pointer)
    {
        *state.drag = Some(CropDrag {
            initial_crop: current_crop,
            initial_pointer: transform.display_to_source(pointer),
        });
    }
    if response.dragged_by(PointerButton::Primary)
        && !space_down
        && let (Some(active_drag), Some(pointer)) = (*state.drag, pointer)
    {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        let source_pointer = transform.display_to_source(pointer);
        let delta_x = (source_pointer.x - active_drag.initial_pointer.x).round() as i64;
        let delta_y = (source_pointer.y - active_drag.initial_pointer.y).round() as i64;
        *state.crop = Some(
            active_drag
                .initial_crop
                .moved_by(delta_x, delta_y, state.source_size),
        );

        let velocity = edge_auto_scroll_velocity(
            pointer.y,
            ui.clip_rect(),
            AUTO_SCROLL_ZONE,
            AUTO_SCROLL_MAXIMUM,
        );
        if velocity != 0.0 {
            let delta_time = ui.input(|input| input.stable_dt.min(0.05));
            ui.scroll_with_delta(Vec2::new(0.0, -velocity * delta_time));
            ui.ctx().request_repaint();
        }
    }
    if response.drag_stopped_by(PointerButton::Primary) {
        *state.drag = None;
    }
    if let Some(active_drag) = *state.drag
        && ui.input(|input| input.key_pressed(Key::Escape))
    {
        *state.crop = Some(active_drag.initial_crop);
        *state.drag = None;
    }

    let panning = response.dragged_by(PointerButton::Middle)
        || (space_down && response.dragged_by(PointerButton::Primary));
    if panning {
        let pointer_delta = ui.input(|input| input.pointer.delta());
        ui.scroll_with_delta(pointer_delta);
        *state.drag = None;
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }

    if response.clicked_by(PointerButton::Primary)
        && let Some(pointer) = pointer
        && let Some(entry) = state.history.iter().rev().find(|entry| {
            transform
                .source_rect_to_display(crop_to_source_rect(entry.crop))
                .contains(pointer)
        })
    {
        *state.crop = Some(entry.crop);
        *state.effective_ratio = crop_effective_ratio(entry.crop);
        *state.size_preference = CropSizePreference::Automatic;
    }
}

fn normalized_resize_wheel_delta(unit: MouseWheelUnit, delta: Vec2) -> f32 {
    let dominant_delta = if delta.x.abs() > delta.y.abs() {
        delta.x
    } else {
        delta.y
    };
    if dominant_delta == 0.0 {
        return 0.0;
    }

    match unit {
        MouseWheelUnit::Point if delta.length() < 8.0 => dominant_delta,
        MouseWheelUnit::Point | MouseWheelUnit::Line | MouseWheelUnit::Page => {
            dominant_delta.signum() * WHEEL_RESIZE_THRESHOLD
        }
    }
}

fn wheel_resize_direction(
    state: &mut ResizeWheelState,
    current_width: u32,
    wheel_delta: f32,
) -> Option<CropResizeDirection> {
    if !wheel_delta.is_finite() || wheel_delta == 0.0 {
        return None;
    }
    if state.latched_width != Some(current_width) {
        state.latched_width = None;
    }
    if state.accumulator != 0.0 && state.accumulator.signum() != wheel_delta.signum() {
        state.accumulator = 0.0;
    }
    state.accumulator += wheel_delta;
    let threshold = if state.latched_width.is_some() {
        WHEEL_RESIZE_THRESHOLD * WHEEL_SNAP_RELEASE_MULTIPLIER
    } else {
        WHEEL_RESIZE_THRESHOLD
    };
    if state.accumulator.abs() < threshold {
        return None;
    }

    let direction = if state.accumulator.is_sign_positive() {
        CropResizeDirection::Larger
    } else {
        CropResizeDirection::Smaller
    };
    state.accumulator = 0.0;
    state.latched_width = None;
    Some(direction)
}

fn magnetically_resized_crop(
    crop: CropRect,
    direction: CropResizeDirection,
    nominal_ratio: AspectRatio,
    effective_ratio: AspectRatio,
    source: SourceSize,
) -> (CropRect, Option<u32>) {
    let step = (crop.width() / 16)
        .max(FINE_RESIZE_PIXELS)
        .next_multiple_of(8);
    let signed_step = match direction {
        CropResizeDirection::Smaller => -i64::from(step),
        CropResizeDirection::Larger => i64::from(step),
    };
    let candidate = crop.resized_by(signed_step, effective_ratio, source);
    let snap_size = common_crop_sizes(nominal_ratio, source)
        .into_iter()
        .filter(|size| match direction {
            CropResizeDirection::Smaller => size.width() < crop.width(),
            CropResizeDirection::Larger => size.width() > crop.width(),
        })
        .min_by_key(|size| size.width().abs_diff(candidate.width()))
        .filter(|size| size.width().abs_diff(candidate.width()) <= step);

    snap_size.map_or((candidate, None), |size| {
        (
            crop.resized_to_dimensions(size.width(), size.height(), source),
            Some(size.width()),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_resize_requires_a_deliberate_scroll_threshold() {
        let mut state = ResizeWheelState::default();

        assert_eq!(wheel_resize_direction(&mut state, 700, 10.0), None);
        assert_eq!(
            wheel_resize_direction(&mut state, 700, 14.0),
            Some(CropResizeDirection::Larger)
        );
        assert_eq!(state.accumulator, 0.0);
    }

    #[test]
    fn wheel_resize_reverses_cleanly_without_retaining_opposite_motion() {
        let mut state = ResizeWheelState {
            accumulator: 12.0,
            latched_width: None,
        };

        assert_eq!(wheel_resize_direction(&mut state, 700, -12.0), None);
        assert_eq!(state.accumulator, -12.0);
        assert_eq!(
            wheel_resize_direction(&mut state, 700, -12.0),
            Some(CropResizeDirection::Smaller)
        );
    }

    #[test]
    fn snapped_wheel_size_requires_extra_motion_to_release() {
        let mut state = ResizeWheelState {
            accumulator: 0.0,
            latched_width: Some(1_024),
        };

        assert_eq!(wheel_resize_direction(&mut state, 1_024, 24.0), None);
        assert_eq!(wheel_resize_direction(&mut state, 1_024, 24.0), None);
        assert_eq!(
            wheel_resize_direction(&mut state, 1_024, 24.0),
            Some(CropResizeDirection::Larger)
        );
        assert_eq!(state.latched_width, None);
    }

    #[test]
    fn mouse_wheel_events_normalize_to_one_resize_action() {
        assert_eq!(
            normalized_resize_wheel_delta(MouseWheelUnit::Line, Vec2::new(1.0, 0.0)),
            WHEEL_RESIZE_THRESHOLD
        );
        assert_eq!(
            normalized_resize_wheel_delta(MouseWheelUnit::Point, Vec2::new(0.0, -3.0)),
            -3.0
        );
    }

    #[test]
    fn magnetic_resize_pulls_nearby_crop_to_standard_size() {
        let source = SourceSize::new(2_048, 3_000).expect("source should be valid");
        let crop = CropRect::new(552, 1_028, 944, 944, source).expect("crop should fit");

        let (resized, snapped_width) = magnetically_resized_crop(
            crop,
            CropResizeDirection::Larger,
            AspectRatio::SQUARE,
            AspectRatio::SQUARE,
            source,
        );

        assert_eq!((resized.width(), resized.height()), (1_024, 1_024));
        assert_eq!(snapped_width, Some(1_024));
    }

    #[test]
    fn magnetic_resize_remains_continuous_between_distant_presets() {
        let source = SourceSize::new(2_048, 3_000).expect("source should be valid");
        let crop = CropRect::new(768, 1_244, 512, 512, source).expect("crop should fit");

        let (resized, snapped_width) = magnetically_resized_crop(
            crop,
            CropResizeDirection::Larger,
            AspectRatio::SQUARE,
            AspectRatio::SQUARE,
            source,
        );

        assert_eq!((resized.width(), resized.height()), (544, 544));
        assert_eq!(snapped_width, None);
    }

    #[test]
    fn magnetic_resize_snaps_to_effective_catalog_ratio() {
        let source = SourceSize::new(1_024, 1_024).expect("source should be valid");
        let crop = CropRect::new(352, 274, 320, 476, source).expect("crop should fit");
        let effective = AspectRatio::new(43, 64).expect("ratio should be valid");

        let (resized, snapped_width) = magnetically_resized_crop(
            crop,
            CropResizeDirection::Larger,
            AspectRatio::PORTRAIT_2_3,
            effective,
            source,
        );

        assert_eq!((resized.width(), resized.height()), (344, 512));
        assert_eq!(snapped_width, Some(344));
    }
}
