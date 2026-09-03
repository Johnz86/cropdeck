use eframe::egui::{self, Event, Key, Modifiers};

use crate::crop::{AspectRatio, CropResizeDirection};

use super::CropDeckApp;
use super::crop_commands::FINE_RESIZE_PIXELS;
use super::workspace::{ZOOM_FACTOR, ZoomMode};

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

fn workspace_key_pressed(context: &egui::Context) -> bool {
    context.input(|input| {
        input.events.iter().any(
            |event| matches!(event, Event::Key { key, pressed: true, .. } if !is_menu_key(*key)),
        )
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
        if focus == InputFocus::Menu {
            if !workspace_key_pressed(context) {
                return;
            }
            egui::Popup::close_all(context);
        }
        self.handle_application_keys(context);
        self.handle_workspace_keys(context, available_width);
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

    fn handle_application_keys(&mut self, context: &egui::Context) {
        let consume = |modifiers, key| context.input_mut(|input| input.consume_key(modifiers, key));
        if consume(Modifiers::CTRL | Modifiers::SHIFT, Key::O) {
            self.open_folder_dialog();
        } else if consume(Modifiers::CTRL, Key::O) {
            self.open_image_dialog();
        } else if consume(Modifiers::CTRL, Key::Comma) {
            self.settings_open = true;
        }
    }

    fn handle_workspace_keys(&mut self, context: &egui::Context, available_width: f32) {
        let pressed = |key| context.input(|input| input.key_pressed(key));
        let (modifiers, pointer_down) =
            context.input(|input| (input.modifiers, input.pointer.any_down()));
        let movement = if modifiers.ctrl {
            1
        } else if modifiers.shift {
            50
        } else {
            10
        };

        if pressed(Key::ArrowLeft) || pressed(Key::A) {
            self.move_crop(-movement, 0);
        }
        if pressed(Key::ArrowRight) || pressed(Key::D) {
            self.move_crop(movement, 0);
        }
        if pressed(Key::ArrowUp) || pressed(Key::W) {
            self.move_crop(0, -movement);
        }
        if pressed(Key::ArrowDown) || pressed(Key::S) {
            self.move_crop(0, movement);
        }
        if pressed(Key::Q) {
            self.navigate(false);
        }
        if pressed(Key::E) {
            self.navigate(true);
        }
        if !modifiers.ctrl && !modifiers.shift && !modifiers.alt && !modifiers.command {
            for (key, ratio) in [
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
                Key::Num7,
                Key::Num8,
                Key::Num9,
            ]
            .into_iter()
            .zip(AspectRatio::PRESETS)
            {
                if pressed(key) {
                    self.apply_ratio(ratio);
                }
            }
        }
        if let Some(crop) = self.workspace.crop {
            if pressed(Key::PageUp) {
                self.move_crop(0, -i64::from(crop.height()));
            }
            if pressed(Key::PageDown) {
                self.move_crop(0, i64::from(crop.height()));
            }
        }
        if pressed(Key::Home) {
            self.move_crop(0, i64::MIN);
            self.workspace.requested_scroll_y = Some(0.0);
        }
        if pressed(Key::End) {
            self.move_crop(0, i64::MAX);
            self.workspace.requested_scroll_y = Some(f32::MAX);
        }
        if pressed(Key::R) {
            self.cycle_ratio(modifiers.shift);
        }
        let keyboard_resize_pixels = if modifiers.ctrl {
            1
        } else {
            i64::from(FINE_RESIZE_PIXELS)
        };
        if pressed(Key::OpenBracket) {
            if modifiers.shift {
                self.snap_crop(CropResizeDirection::Smaller);
            } else {
                self.resize_crop(-keyboard_resize_pixels);
            }
        }
        if pressed(Key::CloseBracket) {
            if modifiers.shift {
                self.snap_crop(CropResizeDirection::Larger);
            } else {
                self.resize_crop(keyboard_resize_pixels);
            }
        }
        if pressed(Key::F) {
            self.workspace.zoom = ZoomMode::FitWidth;
        }
        if let Some(source) = self.source_size() {
            if pressed(Key::Plus) || pressed(Key::Equals) {
                self.workspace.zoom =
                    self.workspace
                        .zoom
                        .zoomed(source.width(), available_width, ZOOM_FACTOR);
            }
            if pressed(Key::Minus) {
                self.workspace.zoom =
                    self.workspace
                        .zoom
                        .zoomed(source.width(), available_width, 1.0 / ZOOM_FACTOR);
            }
        }
        if (pressed(Key::Space) && !pointer_down) || pressed(Key::Enter) {
            self.capture_and_advance();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn focus_with(events: Vec<Event>, modal_open: bool) -> InputFocus {
        let context = egui::Context::default();
        context.begin_pass(egui::RawInput {
            events,
            ..Default::default()
        });
        InputFocus::current(&context, modal_open)
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
    fn menus_keep_navigation_keys_and_yield_the_rest() {
        assert!(is_menu_key(Key::ArrowDown));
        assert!(is_menu_key(Key::Escape));
        assert!(!is_menu_key(Key::Space));
        assert!(!is_menu_key(Key::Q));

        let key_event = |key| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let context = egui::Context::default();
        let begin = |events| {
            context.begin_pass(egui::RawInput {
                events,
                ..Default::default()
            });
        };
        begin(vec![key_event(Key::ArrowDown), key_event(Key::Enter)]);
        assert!(!workspace_key_pressed(&context));

        begin(vec![key_event(Key::ArrowDown), key_event(Key::Space)]);
        assert!(workspace_key_pressed(&context));
    }
}
