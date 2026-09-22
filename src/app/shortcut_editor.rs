use eframe::egui::{self, Event, Key};

use crate::shortcuts::{
    Chord, MAXIMUM_CHORDS_PER_ACTION, SHORTCUTS, ShortcutAction, ShortcutBindings,
    ShortcutCategory, chord_from_event,
};

use super::CropDeckApp;
use super::dialogs::dialog_footer;

const DIALOG_WIDTH: f32 = 700.0;
const LIST_HEIGHT: f32 = 420.0;
const BINDING_COLUMN_WIDTH: f32 = 290.0;
const CHORD_BUTTON_WIDTH: f32 = 104.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BindingSlot {
    action: ShortcutAction,
    slot: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingBinding {
    target: BindingSlot,
    chord: Chord,
    holder: ShortcutAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordedInput {
    Cancelled,
    Cleared,
    Chord(Chord),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorIntent {
    Record(BindingSlot),
    Remove(ShortcutAction, usize),
    Reset(ShortcutAction),
}

#[derive(Debug, Default)]
pub(super) struct ShortcutEditor {
    open: bool,
    filter: String,
    recording: Option<BindingSlot>,
    pending: Option<PendingBinding>,
    confirming_reset: bool,
}

impl ShortcutEditor {
    pub(super) fn open(&mut self) {
        self.open = true;
    }

    pub(super) const fn is_open(&self) -> bool {
        self.open
    }

    fn close(&mut self) {
        *self = Self::default();
    }
}

fn classify(chord: Chord) -> RecordedInput {
    match chord.key() {
        Key::Escape => RecordedInput::Cancelled,
        Key::Backspace | Key::Delete => RecordedInput::Cleared,
        _other => RecordedInput::Chord(chord),
    }
}

fn take_recorded_input(context: &egui::Context) -> Option<RecordedInput> {
    context.input_mut(|input| {
        let modifiers = input.modifiers;
        let mut recorded = None;
        input.events.retain(|event| {
            let chord = chord_from_event(event, modifiers);
            if chord.is_none() && !matches!(event, Event::Text(_)) {
                return true;
            }
            if let Some((chord, _repeat)) = chord
                && recorded.is_none()
            {
                recorded = Some(classify(chord));
            }
            false
        });
        recorded
    })
}

fn matches_filter(action: ShortcutAction, bindings: &ShortcutBindings, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    action.label().to_lowercase().contains(filter)
        || action.category().title().to_lowercase().contains(filter)
        || bindings
            .chords(action)
            .iter()
            .any(|chord| chord.to_string().to_lowercase().contains(filter))
}

fn chord_button(ui: &mut egui::Ui, text: impl Into<String>, recording: bool) -> egui::Response {
    let label = egui::RichText::new(text).monospace();
    ui.add_sized(
        [CHORD_BUTTON_WIDTH, ui.spacing().interact_size.y],
        egui::Button::selectable(recording, label),
    )
}

fn binding_cell(
    ui: &mut egui::Ui,
    action: ShortcutAction,
    bindings: &ShortcutBindings,
    recording: Option<BindingSlot>,
) -> Option<EditorIntent> {
    let mut intent = None;
    let size = egui::Vec2::new(BINDING_COLUMN_WIDTH, ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(
        size,
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            for (slot, chord) in bindings.chords(action).iter().enumerate() {
                let target = BindingSlot {
                    action,
                    slot: Some(slot),
                };
                let active = recording == Some(target);
                let text = if active {
                    String::from("Press keys")
                } else {
                    chord.to_string()
                };
                if chord_button(ui, text, active)
                    .on_hover_text("Click to record a replacement")
                    .clicked()
                {
                    intent = Some(EditorIntent::Record(target));
                }
                if ui
                    .small_button("x")
                    .on_hover_text("Remove this shortcut")
                    .clicked()
                {
                    intent = Some(EditorIntent::Remove(action, slot));
                }
            }
            let append = BindingSlot { action, slot: None };
            if recording == Some(append) {
                chord_button(ui, "Press keys", true);
            } else if bindings.chords(action).len() < MAXIMUM_CHORDS_PER_ACTION
                && ui
                    .small_button("+")
                    .on_hover_text("Add another shortcut")
                    .clicked()
            {
                intent = Some(EditorIntent::Record(append));
            }
            if bindings.chords(action).is_empty() && recording != Some(append) {
                ui.weak("unassigned");
            }
        },
    );
    intent
}

fn shortcut_list(
    ui: &mut egui::Ui,
    bindings: &ShortcutBindings,
    editor: &ShortcutEditor,
) -> Option<EditorIntent> {
    let mut intent = None;
    let filter = editor.filter.to_lowercase();
    egui::ScrollArea::vertical()
        .max_height(LIST_HEIGHT)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("cropdeck_shortcut_bindings")
                .num_columns(3)
                .spacing([12.0, 6.0])
                .striped(true)
                .show(ui, |ui| {
                    for category in ShortcutCategory::ALL {
                        let mut rows = SHORTCUTS
                            .iter()
                            .filter(|spec| spec.category() == category)
                            .map(|spec| spec.action())
                            .filter(|action| matches_filter(*action, bindings, &filter))
                            .peekable();
                        if rows.peek().is_none() {
                            continue;
                        }
                        ui.strong(category.title());
                        ui.end_row();
                        for action in rows {
                            ui.add(
                                egui::Label::new(action.label())
                                    .wrap_mode(egui::TextWrapMode::Extend),
                            );
                            if let Some(cell) = binding_cell(ui, action, bindings, editor.recording)
                            {
                                intent = Some(cell);
                            }
                            if ui
                                .add_enabled(
                                    !bindings.is_default(action),
                                    egui::Button::new("Reset"),
                                )
                                .on_hover_text("Restore the default shortcut")
                                .clicked()
                            {
                                intent = Some(EditorIntent::Reset(action));
                            }
                            ui.end_row();
                        }
                    }
                });
        });
    intent
}

impl CropDeckApp {
    pub(super) fn shortcut_dialog(&mut self, context: &egui::Context) {
        self.record_shortcut(context);
        let modal = egui::Modal::new(egui::Id::new("cropdeck_shortcuts")).show(context, |ui| {
            ui.set_width(DIALOG_WIDTH);
            ui.heading("Keyboard shortcuts");
            ui.weak("Click a shortcut to record a replacement. Esc cancels, Backspace clears.");
            ui.add_space(4.0);
            self.shortcut_toolbar(ui);
            self.pending_notice(ui);
            ui.separator();
            let intent = shortcut_list(ui, self.config.shortcuts(), &self.shortcut_editor);
            if let Some(intent) = intent {
                self.apply_intent(intent);
            }
            dialog_footer(ui)
        });
        if modal.inner || modal.should_close() {
            self.shortcut_editor.close();
        }
    }

    fn shortcut_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.shortcut_editor.filter)
                    .desired_width(220.0)
                    .hint_text("Filter shortcuts"),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.shortcut_editor.confirming_reset {
                    if ui.button("Cancel").clicked() {
                        self.shortcut_editor.confirming_reset = false;
                    }
                    if ui
                        .button(egui::RichText::new("Reset all").color(ui.visuals().error_fg_color))
                        .clicked()
                    {
                        self.config.shortcuts_mut().reset_all();
                        self.shortcut_editor.confirming_reset = false;
                        self.notify("Shortcuts restored to defaults");
                    }
                    ui.weak("Discard every customized shortcut?");
                } else if ui
                    .button("Reset to defaults")
                    .on_hover_text("Restore every shortcut to its default")
                    .clicked()
                {
                    self.shortcut_editor.confirming_reset = true;
                }
            });
        });
    }

    fn pending_notice(&mut self, ui: &mut egui::Ui) {
        let Some(pending) = self.shortcut_editor.pending else {
            return;
        };
        ui.horizontal(|ui| {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!(
                    "{} is assigned to {}.",
                    pending.chord,
                    pending.holder.label()
                ),
            );
            if ui
                .button("Reassign")
                .on_hover_text("Take the shortcut from the other action")
                .clicked()
            {
                self.config
                    .shortcuts_mut()
                    .unbind_collisions(pending.target.action, pending.chord);
                self.config.shortcuts_mut().assign(
                    pending.target.action,
                    pending.target.slot,
                    pending.chord,
                );
                self.shortcut_editor.pending = None;
            }
            if ui.button("Keep current").clicked() {
                self.shortcut_editor.pending = None;
            }
        });
    }

    fn record_shortcut(&mut self, context: &egui::Context) {
        let Some(target) = self.shortcut_editor.recording else {
            return;
        };
        let Some(recorded) = take_recorded_input(context) else {
            return;
        };
        self.shortcut_editor.recording = None;
        match recorded {
            RecordedInput::Cancelled => {}
            RecordedInput::Cleared => {
                if let Some(slot) = target.slot {
                    self.config.shortcuts_mut().remove(target.action, slot);
                }
            }
            RecordedInput::Chord(chord) => {
                match self.config.shortcuts().conflict(target.action, chord) {
                    Some(holder) => {
                        self.shortcut_editor.pending = Some(PendingBinding {
                            target,
                            chord,
                            holder,
                        });
                    }
                    None => self
                        .config
                        .shortcuts_mut()
                        .assign(target.action, target.slot, chord),
                }
            }
        }
    }

    fn apply_intent(&mut self, intent: EditorIntent) {
        self.shortcut_editor.pending = None;
        self.shortcut_editor.confirming_reset = false;
        match intent {
            EditorIntent::Record(target) => self.shortcut_editor.recording = Some(target),
            EditorIntent::Remove(action, slot) => self.config.shortcuts_mut().remove(action, slot),
            EditorIntent::Reset(action) => self.config.shortcuts_mut().reset(action),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcuts::ChordModifiers;

    fn recorded_from(events: Vec<Event>) -> Option<RecordedInput> {
        let context = egui::Context::default();
        context.begin_pass(egui::RawInput {
            events,
            ..Default::default()
        });
        take_recorded_input(&context)
    }

    fn key_event(key: Key, modifiers: egui::Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn recording_reads_the_first_chord_and_swallows_typed_text() {
        let context = egui::Context::default();
        context.begin_pass(egui::RawInput {
            events: vec![
                key_event(Key::G, egui::Modifiers::COMMAND),
                Event::Text(String::from("g")),
                key_event(Key::H, egui::Modifiers::NONE),
            ],
            ..Default::default()
        });

        let recorded = take_recorded_input(&context);

        assert_eq!(
            recorded,
            Some(RecordedInput::Chord(Chord::new(
                Key::G,
                ChordModifiers::COMMAND
            )))
        );
        context.input(|input| assert!(input.events.is_empty()));
    }

    #[test]
    fn escape_cancels_and_backspace_clears() {
        assert_eq!(
            recorded_from(vec![key_event(Key::Escape, egui::Modifiers::NONE)]),
            Some(RecordedInput::Cancelled)
        );
        assert_eq!(
            recorded_from(vec![key_event(Key::Backspace, egui::Modifiers::NONE)]),
            Some(RecordedInput::Cleared)
        );
        assert_eq!(recorded_from(Vec::new()), None);
    }

    #[test]
    fn pointer_input_survives_a_recording_pass() {
        let context = egui::Context::default();
        let moved = Event::PointerMoved(egui::Pos2::new(4.0, 8.0));
        context.begin_pass(egui::RawInput {
            events: vec![moved.clone()],
            ..Default::default()
        });

        assert_eq!(take_recorded_input(&context), None);
        context.input(|input| assert_eq!(input.events, vec![moved]));
    }

    #[test]
    fn the_filter_matches_labels_categories_and_chords() {
        let bindings = ShortcutBindings::default();

        assert!(matches_filter(ShortcutAction::ZoomIn, &bindings, ""));
        assert!(matches_filter(ShortcutAction::ZoomIn, &bindings, "zoom"));
        assert!(matches_filter(ShortcutAction::ZoomIn, &bindings, "view"));
        assert!(matches_filter(
            ShortcutAction::OpenImage,
            &bindings,
            "ctrl+o"
        ));
        assert!(!matches_filter(ShortcutAction::ZoomIn, &bindings, "export"));
    }

    #[test]
    fn closing_the_editor_discards_transient_state() {
        let mut editor = ShortcutEditor::default();
        editor.open();
        editor.filter = String::from("zoom");
        editor.recording = Some(BindingSlot {
            action: ShortcutAction::ZoomIn,
            slot: None,
        });

        editor.close();

        assert!(!editor.is_open());
        assert!(editor.filter.is_empty());
        assert_eq!(editor.recording, None);
    }
}
