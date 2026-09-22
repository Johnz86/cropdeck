use eframe::egui::{self, Key};

use crate::crop::{AspectRatio, CropResizeDirection};
use crate::shortcuts::{ChordModifiers, ShortcutAction, ShortcutBindings, chord_from_event};

use super::CropDeckApp;
use super::crop_commands::FINE_RESIZE_PIXELS;
use super::workspace::{ZOOM_FACTOR, ZoomMode};

const COARSE_MOVE_PIXELS: i64 = 50;
const NORMAL_MOVE_PIXELS: i64 = 10;
const FINE_MOVE_PIXELS: i64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InputFocus {
    Canvas,
    TextField,
    Menu,
    Modal,
}

impl InputFocus {
    pub(super) fn current(context: &egui::Context, modal_open: bool) -> Self {
        if modal_open {
            Self::Modal
        } else if context.egui_wants_keyboard_input() {
            Self::TextField
        } else if context.any_popup_open() {
            Self::Menu
        } else {
            Self::Canvas
        }
    }

    pub(super) const fn suppresses_workspace_keys(self) -> bool {
        matches!(self, Self::TextField | Self::Modal)
    }

    pub(super) const fn suppresses_wheel_zoom(self) -> bool {
        matches!(self, Self::Modal)
    }
}

const fn is_menu_key(key: Key) -> bool {
    matches!(
        key,
        Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight | Key::Enter | Key::Escape
    )
}

const fn move_distance(modifiers: ChordModifiers) -> i64 {
    if modifiers.contains(ChordModifiers::COMMAND) {
        FINE_MOVE_PIXELS
    } else if modifiers.contains(ChordModifiers::SHIFT) {
        COARSE_MOVE_PIXELS
    } else {
        NORMAL_MOVE_PIXELS
    }
}

fn resize_distance(modifiers: ChordModifiers) -> i64 {
    if modifiers.contains(ChordModifiers::COMMAND) {
        1
    } else {
        i64::from(FINE_RESIZE_PIXELS)
    }
}

pub(super) fn take_actions(
    context: &egui::Context,
    bindings: &ShortcutBindings,
) -> Vec<(ShortcutAction, ChordModifiers)> {
    context.input_mut(|input| {
        let modifiers = input.modifiers;
        let mut actions = Vec::new();
        input.events.retain(|event| {
            let Some((chord, repeat)) = chord_from_event(event, modifiers) else {
                return true;
            };
            let Some(action) = bindings.action_for(chord, repeat) else {
                return true;
            };
            actions.push((action, chord.modifiers()));
            false
        });
        actions
    })
}

impl CropDeckApp {
    pub(super) fn handle_shortcuts(&mut self, context: &egui::Context, available_width: f32) {
        let focus = self.input_focus(context);
        if !focus.suppresses_wheel_zoom() {
            self.handle_wheel_zoom(context, available_width);
        }
        if focus.suppresses_workspace_keys() {
            return;
        }
        if focus == InputFocus::Menu && context.input(menu_owns_input) {
            return;
        }
        let actions = take_actions(context, self.config.shortcuts());
        if actions.is_empty() {
            return;
        }
        if focus == InputFocus::Menu {
            egui::Popup::close_all(context);
        }
        let pointer_down = context.input(|input| input.pointer.any_down());
        for (action, modifiers) in actions {
            self.run_shortcut(action, modifiers, available_width, pointer_down);
        }
    }

    fn handle_wheel_zoom(&mut self, context: &egui::Context, available_width: f32) {
        let Some(source) = self.source_size() else {
            return;
        };
        let wheel_zoom = context.input(|input| {
            if input.modifiers.ctrl {
                input.zoom_delta()
            } else {
                1.0
            }
        });
        if (wheel_zoom - 1.0).abs() > f32::EPSILON {
            self.workspace.zoom =
                self.workspace
                    .zoom
                    .zoomed(source.width(), available_width, wheel_zoom);
        }
    }

    fn run_shortcut(
        &mut self,
        action: ShortcutAction,
        modifiers: ChordModifiers,
        available_width: f32,
        pointer_down: bool,
    ) {
        let movement = move_distance(modifiers);
        match action {
            ShortcutAction::OpenImage => self.open_image_dialog(),
            ShortcutAction::OpenFolder => self.open_folder_dialog(),
            ShortcutAction::OpenSettings => self.settings_open = true,
            ShortcutAction::OpenShortcuts => self.shortcut_editor.open(),
            ShortcutAction::CopyCrop => self.copy_crop(),
            ShortcutAction::RevealLastExport => self.reveal_last_export(),
            ShortcutAction::CaptureAndAdvance => {
                if !pointer_down {
                    self.capture_and_advance();
                }
            }
            ShortcutAction::PreviousSource => self.navigate(false),
            ShortcutAction::NextSource => self.navigate(true),
            ShortcutAction::MoveCropLeft => self.move_crop(-movement, 0),
            ShortcutAction::MoveCropRight => self.move_crop(movement, 0),
            ShortcutAction::MoveCropUp => self.move_crop(0, -movement),
            ShortcutAction::MoveCropDown => self.move_crop(0, movement),
            ShortcutAction::MoveCropPageUp => self.move_crop_by_page(false),
            ShortcutAction::MoveCropPageDown => self.move_crop_by_page(true),
            ShortcutAction::MoveCropToTop => {
                self.move_crop(0, i64::MIN);
                self.workspace.requested_scroll_y = Some(0.0);
            }
            ShortcutAction::MoveCropToBottom => {
                self.move_crop(0, i64::MAX);
                self.workspace.requested_scroll_y = Some(f32::MAX);
            }
            ShortcutAction::ShrinkCrop => self.resize_crop(-resize_distance(modifiers)),
            ShortcutAction::GrowCrop => self.resize_crop(resize_distance(modifiers)),
            ShortcutAction::SnapCropSmaller => self.snap_crop(CropResizeDirection::Smaller),
            ShortcutAction::SnapCropLarger => self.snap_crop(CropResizeDirection::Larger),
            ShortcutAction::CycleRatioForward => self.cycle_ratio(false),
            ShortcutAction::CycleRatioBackward => self.cycle_ratio(true),
            ShortcutAction::AspectRatioSlot1 => self.apply_ratio_slot(0),
            ShortcutAction::AspectRatioSlot2 => self.apply_ratio_slot(1),
            ShortcutAction::AspectRatioSlot3 => self.apply_ratio_slot(2),
            ShortcutAction::AspectRatioSlot4 => self.apply_ratio_slot(3),
            ShortcutAction::AspectRatioSlot5 => self.apply_ratio_slot(4),
            ShortcutAction::AspectRatioSlot6 => self.apply_ratio_slot(5),
            ShortcutAction::AspectRatioSlot7 => self.apply_ratio_slot(6),
            ShortcutAction::AspectRatioSlot8 => self.apply_ratio_slot(7),
            ShortcutAction::AspectRatioSlot9 => self.apply_ratio_slot(8),
            ShortcutAction::FitImageWidth => self.workspace.zoom = ZoomMode::FitWidth,
            ShortcutAction::ZoomIn => self.zoom_by(ZOOM_FACTOR, available_width),
            ShortcutAction::ZoomOut => self.zoom_by(1.0 / ZOOM_FACTOR, available_width),
        }
    }

    fn apply_ratio_slot(&mut self, slot: usize) {
        if let Some(ratio) = AspectRatio::PRESETS.get(slot) {
            self.apply_ratio(*ratio);
        }
    }

    fn move_crop_by_page(&mut self, downward: bool) {
        if let Some(crop) = self.workspace.crop {
            let height = i64::from(crop.height());
            self.move_crop(0, if downward { height } else { -height });
        }
    }

    fn zoom_by(&mut self, factor: f32, available_width: f32) {
        if let Some(source) = self.source_size() {
            self.workspace.zoom =
                self.workspace
                    .zoom
                    .zoomed(source.width(), available_width, factor);
        }
    }
}

fn menu_owns_input(input: &egui::InputState) -> bool {
    input.events.iter().all(|event| {
        matches!(event, egui::Event::Key { key, pressed: true, .. } if is_menu_key(*key))
            || !matches!(event, egui::Event::Key { pressed: true, .. })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcuts::Chord;
    use eframe::egui::{Event, Modifiers};

    fn focus_with(events: Vec<Event>, modal_open: bool) -> InputFocus {
        let context = egui::Context::default();
        context.begin_pass(egui::RawInput {
            events,
            ..Default::default()
        });
        InputFocus::current(&context, modal_open)
    }

    fn key_event(key: Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn actions_for(events: Vec<Event>) -> Vec<(ShortcutAction, ChordModifiers)> {
        let context = egui::Context::default();
        context.begin_pass(egui::RawInput {
            events,
            ..Default::default()
        });
        take_actions(&context, &ShortcutBindings::default())
    }

    #[test]
    fn modal_and_text_focus_suppress_workspace_keys_but_menus_do_not() {
        assert!(InputFocus::Modal.suppresses_workspace_keys());
        assert!(InputFocus::TextField.suppresses_workspace_keys());
        assert!(!InputFocus::Menu.suppresses_workspace_keys());
        assert!(!InputFocus::Canvas.suppresses_workspace_keys());
        assert!(InputFocus::Modal.suppresses_wheel_zoom());
        assert!(!InputFocus::TextField.suppresses_wheel_zoom());
    }

    #[test]
    fn modal_state_wins_over_everything_else() {
        assert_eq!(focus_with(Vec::new(), true), InputFocus::Modal);
        assert_eq!(focus_with(Vec::new(), false), InputFocus::Canvas);
    }

    #[test]
    fn bound_events_are_taken_once_and_unbound_events_stay() {
        let context = egui::Context::default();
        context.begin_pass(egui::RawInput {
            events: vec![
                key_event(Key::Space, Modifiers::NONE),
                Event::Text(String::from("c")),
            ],
            ..Default::default()
        });
        let bindings = ShortcutBindings::default();

        let taken = take_actions(&context, &bindings);

        assert_eq!(
            taken,
            vec![(ShortcutAction::CaptureAndAdvance, ChordModifiers::NONE)]
        );
        assert!(take_actions(&context, &bindings).is_empty());
        context.input(|input| {
            assert_eq!(input.events, vec![Event::Text(String::from("c"))]);
        });
    }

    #[test]
    fn the_copy_event_reaches_the_copy_action() {
        assert_eq!(
            actions_for(vec![Event::Copy]),
            vec![(ShortcutAction::CopyCrop, ChordModifiers::COMMAND)]
        );
    }

    #[test]
    fn menus_keep_navigation_keys_and_yield_the_rest() {
        assert!(is_menu_key(Key::ArrowDown));
        assert!(is_menu_key(Key::Escape));
        assert!(!is_menu_key(Key::Space));
        assert!(!is_menu_key(Key::Q));

        let context = egui::Context::default();
        let begin = |events| {
            context.begin_pass(egui::RawInput {
                events,
                ..Default::default()
            });
        };
        begin(vec![
            key_event(Key::ArrowDown, Modifiers::NONE),
            key_event(Key::Enter, Modifiers::NONE),
        ]);
        assert!(context.input(menu_owns_input));

        begin(vec![
            key_event(Key::ArrowDown, Modifiers::NONE),
            key_event(Key::Space, Modifiers::NONE),
        ]);
        assert!(!context.input(menu_owns_input));
    }

    #[test]
    fn modifiers_scale_movement_and_resizing() {
        assert_eq!(move_distance(ChordModifiers::NONE), NORMAL_MOVE_PIXELS);
        assert_eq!(move_distance(ChordModifiers::SHIFT), COARSE_MOVE_PIXELS);
        assert_eq!(move_distance(ChordModifiers::COMMAND), FINE_MOVE_PIXELS);
        assert_eq!(resize_distance(ChordModifiers::COMMAND), 1);
        assert_eq!(
            resize_distance(ChordModifiers::NONE),
            i64::from(FINE_RESIZE_PIXELS)
        );
    }

    #[test]
    fn a_rebound_chord_reaches_its_new_action_only() {
        let mut bindings = ShortcutBindings::default();
        let chord = Chord::new(Key::F8, ChordModifiers::NONE);
        bindings.assign(ShortcutAction::CaptureAndAdvance, Some(0), chord);

        assert_eq!(
            bindings.action_for(chord, false),
            Some(ShortcutAction::CaptureAndAdvance)
        );
        assert_eq!(
            bindings.action_for(Chord::new(Key::Space, ChordModifiers::NONE), false),
            None
        );
    }
}
