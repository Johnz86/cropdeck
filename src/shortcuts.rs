use std::collections::BTreeMap;
use std::fmt;

use eframe::egui::{Event, Key, Modifiers};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAXIMUM_CHORDS_PER_ACTION: usize = 3;

const COMMAND_LABEL: &str = if cfg!(target_os = "macos") {
    "Cmd"
} else {
    "Ctrl"
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChordModifiers(u8);

impl ChordModifiers {
    pub const NONE: Self = Self(0);
    pub const COMMAND: Self = Self(1 << 0);
    pub const ALT: Self = Self(1 << 1);
    pub const SHIFT: Self = Self(1 << 2);

    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl From<Modifiers> for ChordModifiers {
    fn from(modifiers: Modifiers) -> Self {
        let mut chord = Self::NONE;
        if modifiers.command || modifiers.ctrl || modifiers.mac_cmd {
            chord = chord.with(Self::COMMAND);
        }
        if modifiers.alt {
            chord = chord.with(Self::ALT);
        }
        if modifiers.shift {
            chord = chord.with(Self::SHIFT);
        }
        chord
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    modifiers: ChordModifiers,
    key: Key,
}

impl Chord {
    #[must_use]
    pub const fn new(key: Key, modifiers: ChordModifiers) -> Self {
        Self { modifiers, key }
    }

    #[must_use]
    pub fn pressed(key: Key, modifiers: Modifiers) -> Self {
        Self::new(key, ChordModifiers::from(modifiers))
    }

    #[must_use]
    pub const fn key(self) -> Key {
        self.key
    }

    #[must_use]
    pub const fn modifiers(self) -> ChordModifiers {
        self.modifiers
    }

    #[must_use]
    pub fn config_name(self) -> String {
        format_chord(self.modifiers, self.key.name())
    }

    pub fn parse(text: &str) -> Result<Self, ChordParseError> {
        if text.trim().is_empty() {
            return Err(ChordParseError::Empty);
        }
        let parts: Vec<&str> = text.split('+').collect();
        let mut modifiers = ChordModifiers::NONE;
        let mut key = None;
        for (index, part) in parts.iter().enumerate() {
            let token = part.trim();
            if index + 1 == parts.len() {
                key = Some(match Key::from_name(token) {
                    Some(named) => named,
                    None if token.is_empty() => Key::Plus,
                    None if parse_modifier(token).is_some() => {
                        return Err(ChordParseError::ModifierOnly);
                    }
                    None => return Err(ChordParseError::UnknownKey(token.to_owned())),
                });
            } else if !token.is_empty() {
                modifiers = modifiers.with(
                    parse_modifier(token)
                        .ok_or_else(|| ChordParseError::UnknownModifier(token.to_owned()))?,
                );
            }
        }
        let key = key.ok_or(ChordParseError::Empty)?;
        if is_modifier_key(key) {
            return Err(ChordParseError::ModifierOnly);
        }
        Ok(Self::new(key, modifiers))
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&format_chord(self.modifiers, display_key_name(self.key)))
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ChordParseError {
    #[error("a shortcut needs at least one key")]
    Empty,

    #[error("unknown modifier: {0}")]
    UnknownModifier(String),

    #[error("unknown key: {0}")]
    UnknownKey(String),

    #[error("a shortcut needs a key beyond its modifiers")]
    ModifierOnly,
}

fn format_chord(modifiers: ChordModifiers, key_name: &str) -> String {
    let mut text = String::with_capacity(key_name.len() + 18);
    for (modifier, label) in [
        (ChordModifiers::COMMAND, COMMAND_LABEL),
        (ChordModifiers::ALT, "Alt"),
        (ChordModifiers::SHIFT, "Shift"),
    ] {
        if modifiers.contains(modifier) {
            text.push_str(label);
            text.push('+');
        }
    }
    text.push_str(key_name);
    text
}

fn parse_modifier(token: &str) -> Option<ChordModifiers> {
    match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" | "cmd" | "command" | "super" | "meta" | "win" => {
            Some(ChordModifiers::COMMAND)
        }
        "alt" | "option" | "opt" => Some(ChordModifiers::ALT),
        "shift" => Some(ChordModifiers::SHIFT),
        _unknown => None,
    }
}

#[must_use]
pub fn display_key_name(key: Key) -> &'static str {
    match key {
        Key::OpenBracket => "[",
        Key::CloseBracket => "]",
        Key::Comma => ",",
        Key::Period => ".",
        Key::Minus => "-",
        Key::Plus => "+",
        Key::Equals => "=",
        Key::Semicolon => ";",
        Key::Slash => "/",
        Key::Backslash => "\\",
        Key::Quote => "'",
        Key::Backtick => "`",
        other => other.name(),
    }
}

#[must_use]
pub const fn is_modifier_key(key: Key) -> bool {
    matches!(
        key,
        Key::ShiftLeft
            | Key::ShiftRight
            | Key::ControlLeft
            | Key::ControlRight
            | Key::AltLeft
            | Key::AltRight
            | Key::SuperLeft
            | Key::SuperRight
    )
}

#[must_use]
pub fn chord_from_event(event: &Event, modifiers: Modifiers) -> Option<(Chord, bool)> {
    match event {
        Event::Copy => Some((
            Chord::new(
                Key::C,
                ChordModifiers::from(modifiers).with(ChordModifiers::COMMAND),
            ),
            false,
        )),
        Event::Key {
            key,
            pressed: true,
            repeat,
            modifiers: pressed_modifiers,
            physical_key: _physical_key,
        } if !is_modifier_key(*key) => Some((Chord::pressed(*key, *pressed_modifiers), *repeat)),
        Event::Key { .. }
        | Event::Cut
        | Event::Paste(_)
        | Event::Text(_)
        | Event::PointerMoved(_)
        | Event::MouseMoved(_)
        | Event::PointerButton { .. }
        | Event::PointerGone
        | Event::Zoom(_)
        | Event::Rotate(_)
        | Event::ModifiersChanged(_)
        | Event::Ime(_)
        | Event::Touch { .. }
        | Event::MouseWheel { .. }
        | Event::WindowFocused(_)
        | Event::AccessKitActionRequest(_)
        | Event::Screenshot { .. } => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ShortcutCategory {
    Application,
    Source,
    CropPlacement,
    CropSize,
    AspectRatioPresets,
    View,
}

impl ShortcutCategory {
    pub const ALL: [Self; 6] = [
        Self::Application,
        Self::Source,
        Self::CropPlacement,
        Self::CropSize,
        Self::AspectRatioPresets,
        Self::View,
    ];

    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Application => "Application",
            Self::Source => "Sources",
            Self::CropPlacement => "Crop placement",
            Self::CropSize => "Crop size",
            Self::AspectRatioPresets => "Aspect ratio presets",
            Self::View => "View",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ShortcutAction {
    OpenImage,
    OpenFolder,
    OpenSettings,
    OpenShortcuts,
    CopyCrop,
    RevealLastExport,
    CaptureAndAdvance,
    PreviousSource,
    NextSource,
    MoveCropLeft,
    MoveCropRight,
    MoveCropUp,
    MoveCropDown,
    MoveCropPageUp,
    MoveCropPageDown,
    MoveCropToTop,
    MoveCropToBottom,
    ShrinkCrop,
    GrowCrop,
    SnapCropSmaller,
    SnapCropLarger,
    CycleRatioForward,
    CycleRatioBackward,
    AspectRatioSlot1,
    AspectRatioSlot2,
    AspectRatioSlot3,
    AspectRatioSlot4,
    AspectRatioSlot5,
    AspectRatioSlot6,
    AspectRatioSlot7,
    AspectRatioSlot8,
    AspectRatioSlot9,
    FitImageWidth,
    ZoomIn,
    ZoomOut,
}

impl ShortcutAction {
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[must_use]
    pub fn spec(self) -> &'static ShortcutSpec {
        &SHORTCUTS[self.index()]
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        self.spec().label
    }

    #[must_use]
    pub fn category(self) -> ShortcutCategory {
        self.spec().category
    }

    #[must_use]
    pub fn from_config_name(name: &str) -> Option<Self> {
        SHORTCUTS
            .iter()
            .find(|spec| spec.name == name)
            .map(|spec| spec.action)
    }

    #[must_use]
    pub fn config_name(self) -> &'static str {
        self.spec().name
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ShortcutSpec {
    action: ShortcutAction,
    category: ShortcutCategory,
    name: &'static str,
    label: &'static str,
    defaults: &'static [Chord],
    free_modifiers: ChordModifiers,
    repeats: bool,
}

impl ShortcutSpec {
    #[must_use]
    pub const fn action(&self) -> ShortcutAction {
        self.action
    }

    #[must_use]
    pub const fn category(&self) -> ShortcutCategory {
        self.category
    }

    #[must_use]
    pub const fn label(&self) -> &'static str {
        self.label
    }

    #[must_use]
    pub const fn defaults(&self) -> &'static [Chord] {
        self.defaults
    }
}

const fn chord(key: Key) -> Chord {
    Chord::new(key, ChordModifiers::NONE)
}

const fn command(key: Key) -> Chord {
    Chord::new(key, ChordModifiers::COMMAND)
}

const fn command_shift(key: Key) -> Chord {
    Chord::new(key, ChordModifiers::COMMAND.with(ChordModifiers::SHIFT))
}

const fn shift(key: Key) -> Chord {
    Chord::new(key, ChordModifiers::SHIFT)
}

const SPEED_MODIFIERS: ChordModifiers = ChordModifiers::COMMAND.with(ChordModifiers::SHIFT);

pub static SHORTCUTS: [ShortcutSpec; 35] = [
    ShortcutSpec {
        action: ShortcutAction::OpenImage,
        category: ShortcutCategory::Application,
        name: "open_image",
        label: "Open image",
        defaults: &[command(Key::O)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::OpenFolder,
        category: ShortcutCategory::Application,
        name: "open_folder",
        label: "Open folder",
        defaults: &[command_shift(Key::O)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::OpenSettings,
        category: ShortcutCategory::Application,
        name: "open_settings",
        label: "Open settings",
        defaults: &[command(Key::Comma)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::OpenShortcuts,
        category: ShortcutCategory::Application,
        name: "open_shortcuts",
        label: "Open keyboard shortcuts",
        defaults: &[command_shift(Key::K)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::CopyCrop,
        category: ShortcutCategory::Application,
        name: "copy_crop",
        label: "Copy crop to clipboard",
        defaults: &[command(Key::C)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::RevealLastExport,
        category: ShortcutCategory::Application,
        name: "reveal_last_export",
        label: "Reveal last export",
        defaults: &[command_shift(Key::R)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::CaptureAndAdvance,
        category: ShortcutCategory::Application,
        name: "capture_and_advance",
        label: "Capture and advance",
        defaults: &[chord(Key::Space), chord(Key::Enter)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::PreviousSource,
        category: ShortcutCategory::Source,
        name: "previous_source",
        label: "Previous source",
        defaults: &[chord(Key::Q)],
        free_modifiers: ChordModifiers::NONE,
        repeats: true,
    },
    ShortcutSpec {
        action: ShortcutAction::NextSource,
        category: ShortcutCategory::Source,
        name: "next_source",
        label: "Next source",
        defaults: &[chord(Key::E)],
        free_modifiers: ChordModifiers::NONE,
        repeats: true,
    },
    ShortcutSpec {
        action: ShortcutAction::MoveCropLeft,
        category: ShortcutCategory::CropPlacement,
        name: "move_crop_left",
        label: "Move crop left",
        defaults: &[chord(Key::ArrowLeft), chord(Key::A)],
        free_modifiers: SPEED_MODIFIERS,
        repeats: true,
    },
    ShortcutSpec {
        action: ShortcutAction::MoveCropRight,
        category: ShortcutCategory::CropPlacement,
        name: "move_crop_right",
        label: "Move crop right",
        defaults: &[chord(Key::ArrowRight), chord(Key::D)],
        free_modifiers: SPEED_MODIFIERS,
        repeats: true,
    },
    ShortcutSpec {
        action: ShortcutAction::MoveCropUp,
        category: ShortcutCategory::CropPlacement,
        name: "move_crop_up",
        label: "Move crop up",
        defaults: &[chord(Key::ArrowUp), chord(Key::W)],
        free_modifiers: SPEED_MODIFIERS,
        repeats: true,
    },
    ShortcutSpec {
        action: ShortcutAction::MoveCropDown,
        category: ShortcutCategory::CropPlacement,
        name: "move_crop_down",
        label: "Move crop down",
        defaults: &[chord(Key::ArrowDown), chord(Key::S)],
        free_modifiers: SPEED_MODIFIERS,
        repeats: true,
    },
    ShortcutSpec {
        action: ShortcutAction::MoveCropPageUp,
        category: ShortcutCategory::CropPlacement,
        name: "move_crop_page_up",
        label: "Move crop up one crop height",
        defaults: &[chord(Key::PageUp)],
        free_modifiers: ChordModifiers::NONE,
        repeats: true,
    },
    ShortcutSpec {
        action: ShortcutAction::MoveCropPageDown,
        category: ShortcutCategory::CropPlacement,
        name: "move_crop_page_down",
        label: "Move crop down one crop height",
        defaults: &[chord(Key::PageDown)],
        free_modifiers: ChordModifiers::NONE,
        repeats: true,
    },
    ShortcutSpec {
        action: ShortcutAction::MoveCropToTop,
        category: ShortcutCategory::CropPlacement,
        name: "move_crop_to_top",
        label: "Move crop to the top",
        defaults: &[chord(Key::Home)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::MoveCropToBottom,
        category: ShortcutCategory::CropPlacement,
        name: "move_crop_to_bottom",
        label: "Move crop to the bottom",
        defaults: &[chord(Key::End)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::ShrinkCrop,
        category: ShortcutCategory::CropSize,
        name: "shrink_crop",
        label: "Shrink crop",
        defaults: &[chord(Key::OpenBracket)],
        free_modifiers: ChordModifiers::COMMAND,
        repeats: true,
    },
    ShortcutSpec {
        action: ShortcutAction::GrowCrop,
        category: ShortcutCategory::CropSize,
        name: "grow_crop",
        label: "Grow crop",
        defaults: &[chord(Key::CloseBracket)],
        free_modifiers: ChordModifiers::COMMAND,
        repeats: true,
    },
    ShortcutSpec {
        action: ShortcutAction::SnapCropSmaller,
        category: ShortcutCategory::CropSize,
        name: "snap_crop_smaller",
        label: "Previous preset size",
        defaults: &[shift(Key::OpenBracket)],
        free_modifiers: ChordModifiers::NONE,
        repeats: true,
    },
    ShortcutSpec {
        action: ShortcutAction::SnapCropLarger,
        category: ShortcutCategory::CropSize,
        name: "snap_crop_larger",
        label: "Next preset size",
        defaults: &[shift(Key::CloseBracket)],
        free_modifiers: ChordModifiers::NONE,
        repeats: true,
    },
    ShortcutSpec {
        action: ShortcutAction::CycleRatioForward,
        category: ShortcutCategory::AspectRatioPresets,
        name: "cycle_ratio_forward",
        label: "Cycle quick ratios forward",
        defaults: &[chord(Key::R)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::CycleRatioBackward,
        category: ShortcutCategory::AspectRatioPresets,
        name: "cycle_ratio_backward",
        label: "Cycle quick ratios backward",
        defaults: &[shift(Key::R)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::AspectRatioSlot1,
        category: ShortcutCategory::AspectRatioPresets,
        name: "aspect_ratio_slot_1",
        label: "Quick ratio 1",
        defaults: &[chord(Key::Num1)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::AspectRatioSlot2,
        category: ShortcutCategory::AspectRatioPresets,
        name: "aspect_ratio_slot_2",
        label: "Quick ratio 2",
        defaults: &[chord(Key::Num2)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::AspectRatioSlot3,
        category: ShortcutCategory::AspectRatioPresets,
        name: "aspect_ratio_slot_3",
        label: "Quick ratio 3",
        defaults: &[chord(Key::Num3)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::AspectRatioSlot4,
        category: ShortcutCategory::AspectRatioPresets,
        name: "aspect_ratio_slot_4",
        label: "Quick ratio 4",
        defaults: &[chord(Key::Num4)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::AspectRatioSlot5,
        category: ShortcutCategory::AspectRatioPresets,
        name: "aspect_ratio_slot_5",
        label: "Quick ratio 5",
        defaults: &[chord(Key::Num5)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::AspectRatioSlot6,
        category: ShortcutCategory::AspectRatioPresets,
        name: "aspect_ratio_slot_6",
        label: "Quick ratio 6",
        defaults: &[chord(Key::Num6)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::AspectRatioSlot7,
        category: ShortcutCategory::AspectRatioPresets,
        name: "aspect_ratio_slot_7",
        label: "Quick ratio 7",
        defaults: &[chord(Key::Num7)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::AspectRatioSlot8,
        category: ShortcutCategory::AspectRatioPresets,
        name: "aspect_ratio_slot_8",
        label: "Quick ratio 8",
        defaults: &[chord(Key::Num8)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::AspectRatioSlot9,
        category: ShortcutCategory::AspectRatioPresets,
        name: "aspect_ratio_slot_9",
        label: "Quick ratio 9",
        defaults: &[chord(Key::Num9)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::FitImageWidth,
        category: ShortcutCategory::View,
        name: "fit_image_width",
        label: "Fit image width",
        defaults: &[chord(Key::F)],
        free_modifiers: ChordModifiers::NONE,
        repeats: false,
    },
    ShortcutSpec {
        action: ShortcutAction::ZoomIn,
        category: ShortcutCategory::View,
        name: "zoom_in",
        label: "Zoom in",
        defaults: &[chord(Key::Plus), chord(Key::Equals)],
        free_modifiers: ChordModifiers::SHIFT,
        repeats: true,
    },
    ShortcutSpec {
        action: ShortcutAction::ZoomOut,
        category: ShortcutCategory::View,
        name: "zoom_out",
        label: "Zoom out",
        defaults: &[chord(Key::Minus)],
        free_modifiers: ChordModifiers::SHIFT,
        repeats: true,
    },
];

fn chords_collide(
    left: Chord,
    left_free: ChordModifiers,
    right: Chord,
    right_free: ChordModifiers,
) -> bool {
    let free = left_free.with(right_free);
    left.key == right.key && left.modifiers.without(free) == right.modifiers.without(free)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    from = "BTreeMap<String, Vec<String>>",
    into = "BTreeMap<String, Vec<String>>"
)]
pub struct ShortcutBindings {
    chords: Vec<Vec<Chord>>,
}

impl Default for ShortcutBindings {
    fn default() -> Self {
        Self {
            chords: SHORTCUTS
                .iter()
                .map(|spec| spec.defaults.to_vec())
                .collect(),
        }
    }
}

impl ShortcutBindings {
    #[must_use]
    pub fn chords(&self, action: ShortcutAction) -> &[Chord] {
        &self.chords[action.index()]
    }

    #[must_use]
    pub fn primary(&self, action: ShortcutAction) -> Option<Chord> {
        self.chords(action).first().copied()
    }

    #[must_use]
    pub fn display(&self, action: ShortcutAction) -> String {
        self.primary(action)
            .map(|chord| chord.to_string())
            .unwrap_or_default()
    }

    #[must_use]
    pub fn is_default(&self, action: ShortcutAction) -> bool {
        self.chords(action) == action.spec().defaults
    }

    pub fn reset(&mut self, action: ShortcutAction) {
        self.chords[action.index()] = action.spec().defaults.to_vec();
    }

    pub fn reset_all(&mut self) {
        *self = Self::default();
    }

    pub fn remove(&mut self, action: ShortcutAction, slot: usize) {
        let chords = &mut self.chords[action.index()];
        if slot < chords.len() {
            chords.remove(slot);
        }
    }

    pub fn unbind_collisions(&mut self, action: ShortcutAction, chord: Chord) {
        let free = action.spec().free_modifiers;
        for spec in &SHORTCUTS {
            if spec.action == action {
                continue;
            }
            self.chords[spec.action.index()]
                .retain(|bound| !chords_collide(chord, free, *bound, spec.free_modifiers));
        }
    }

    pub fn assign(&mut self, action: ShortcutAction, slot: Option<usize>, chord: Chord) {
        let chords = &mut self.chords[action.index()];
        match slot.filter(|slot| *slot < chords.len()) {
            Some(slot) => {
                chords[slot] = chord;
                let mut seen = 0;
                chords.retain(|bound| {
                    let keep = *bound != chord || seen == slot;
                    seen += 1;
                    keep
                });
            }
            None => {
                chords.retain(|bound| *bound != chord);
                if chords.len() < MAXIMUM_CHORDS_PER_ACTION {
                    chords.push(chord);
                }
            }
        }
    }

    #[must_use]
    pub fn conflict(&self, action: ShortcutAction, chord: Chord) -> Option<ShortcutAction> {
        let free = action.spec().free_modifiers;
        SHORTCUTS
            .iter()
            .filter(|spec| spec.action != action)
            .find(|spec| {
                self.chords(spec.action)
                    .iter()
                    .any(|bound| chords_collide(chord, free, *bound, spec.free_modifiers))
            })
            .map(|spec| spec.action)
    }

    #[must_use]
    pub fn action_for(&self, chord: Chord, repeat: bool) -> Option<ShortcutAction> {
        SHORTCUTS
            .iter()
            .filter(|spec| spec.repeats || !repeat)
            .find(|spec| {
                self.chords(spec.action).iter().any(|bound| {
                    chord.key == bound.key
                        && chord.modifiers.without(spec.free_modifiers)
                            == bound.modifiers.without(spec.free_modifiers)
                })
            })
            .map(|spec| spec.action)
    }
}

impl From<BTreeMap<String, Vec<String>>> for ShortcutBindings {
    fn from(stored: BTreeMap<String, Vec<String>>) -> Self {
        let mut bindings = Self::default();
        for (name, chords) in stored {
            let Some(action) = ShortcutAction::from_config_name(&name) else {
                continue;
            };
            let mut parsed: Vec<Chord> = chords
                .iter()
                .filter_map(|text| Chord::parse(text).ok())
                .collect();
            parsed.truncate(MAXIMUM_CHORDS_PER_ACTION);
            bindings.chords[action.index()] = parsed;
        }
        bindings.drop_collisions();
        bindings
    }
}

impl From<ShortcutBindings> for BTreeMap<String, Vec<String>> {
    fn from(bindings: ShortcutBindings) -> Self {
        SHORTCUTS
            .iter()
            .map(|spec| {
                (
                    spec.name.to_owned(),
                    bindings
                        .chords(spec.action)
                        .iter()
                        .map(|chord| chord.config_name())
                        .collect(),
                )
            })
            .collect()
    }
}

impl ShortcutBindings {
    fn drop_collisions(&mut self) {
        let mut kept: Vec<(Chord, ChordModifiers)> = Vec::new();
        for spec in &SHORTCUTS {
            let free = spec.free_modifiers;
            self.chords[spec.action.index()].retain(|candidate| {
                if kept.iter().any(|(chord, other_free)| {
                    chords_collide(*candidate, free, *chord, *other_free)
                }) {
                    return false;
                }
                kept.push((*candidate, free));
                true
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_indexes_its_own_specification() {
        for (index, spec) in SHORTCUTS.iter().enumerate() {
            assert_eq!(spec.action.index(), index, "{}", spec.name);
            assert_eq!(spec.action.spec().name, spec.name);
        }
    }

    #[test]
    fn the_default_map_is_free_of_collisions() {
        let bindings = ShortcutBindings::default();

        for spec in &SHORTCUTS {
            for bound in bindings.chords(spec.action) {
                assert_eq!(
                    bindings.conflict(spec.action, *bound),
                    None,
                    "{} collides on {bound}",
                    spec.name
                );
            }
        }
    }

    #[test]
    fn chords_round_trip_through_their_configuration_name() {
        for chord in [
            Chord::new(Key::O, ChordModifiers::COMMAND.with(ChordModifiers::SHIFT)),
            Chord::new(Key::Plus, ChordModifiers::NONE),
            Chord::new(Key::Comma, ChordModifiers::COMMAND),
            Chord::new(Key::OpenBracket, ChordModifiers::ALT),
        ] {
            assert_eq!(Chord::parse(&chord.config_name()), Ok(chord));
        }
        assert_eq!(
            Chord::parse("ctrl++"),
            Ok(Chord::new(Key::Plus, ChordModifiers::COMMAND))
        );
        assert_eq!(
            Chord::parse("Ctrl+Shift+O").map(|chord| chord.to_string()),
            Ok(String::from("Ctrl+Shift+O"))
        );
        assert_eq!(Chord::parse("Ctrl"), Err(ChordParseError::ModifierOnly));
        assert_eq!(Chord::parse("   "), Err(ChordParseError::Empty));
        assert!(matches!(
            Chord::parse("Hyper+O"),
            Err(ChordParseError::UnknownModifier(_))
        ));
        assert!(matches!(
            Chord::parse("Ctrl+Nonsense"),
            Err(ChordParseError::UnknownKey(_))
        ));
    }

    #[test]
    fn free_modifiers_let_speed_variants_share_one_binding() {
        let bindings = ShortcutBindings::default();
        let arrow = Chord::new(Key::ArrowLeft, ChordModifiers::NONE);
        let fast_arrow = Chord::new(Key::ArrowLeft, ChordModifiers::SHIFT);
        let fine_bracket = Chord::new(Key::OpenBracket, ChordModifiers::COMMAND);
        let snap_bracket = Chord::new(Key::OpenBracket, ChordModifiers::SHIFT);

        assert_eq!(
            bindings.action_for(arrow, false),
            Some(ShortcutAction::MoveCropLeft)
        );
        assert_eq!(
            bindings.action_for(fast_arrow, true),
            Some(ShortcutAction::MoveCropLeft)
        );
        assert_eq!(
            bindings.action_for(fine_bracket, false),
            Some(ShortcutAction::ShrinkCrop)
        );
        assert_eq!(
            bindings.action_for(snap_bracket, false),
            Some(ShortcutAction::SnapCropSmaller)
        );
    }

    #[test]
    fn repeating_events_only_reach_actions_that_accept_them() {
        let bindings = ShortcutBindings::default();
        let space = Chord::new(Key::Space, ChordModifiers::NONE);

        assert_eq!(
            bindings.action_for(space, false),
            Some(ShortcutAction::CaptureAndAdvance)
        );
        assert_eq!(bindings.action_for(space, true), None);
    }

    #[test]
    fn assigning_a_chord_reports_and_replaces_its_previous_owner() {
        let mut bindings = ShortcutBindings::default();
        let open_image = Chord::new(Key::O, ChordModifiers::COMMAND);

        assert_eq!(
            bindings.conflict(ShortcutAction::OpenFolder, open_image),
            Some(ShortcutAction::OpenImage)
        );

        bindings.unbind_collisions(ShortcutAction::OpenFolder, open_image);
        bindings.assign(ShortcutAction::OpenFolder, Some(0), open_image);

        assert!(bindings.chords(ShortcutAction::OpenImage).is_empty());
        assert_eq!(
            bindings.action_for(open_image, false),
            Some(ShortcutAction::OpenFolder)
        );
        assert!(!bindings.is_default(ShortcutAction::OpenFolder));

        bindings.reset(ShortcutAction::OpenFolder);
        bindings.reset(ShortcutAction::OpenImage);
        assert_eq!(bindings, ShortcutBindings::default());
    }

    #[test]
    fn a_full_action_refuses_extra_chords_and_never_stores_duplicates() {
        let mut bindings = ShortcutBindings::default();
        let action = ShortcutAction::CaptureAndAdvance;

        bindings.assign(action, None, Chord::new(Key::Space, ChordModifiers::NONE));
        assert_eq!(bindings.chords(action).len(), 2);

        bindings.assign(action, None, Chord::new(Key::F5, ChordModifiers::NONE));
        bindings.assign(action, None, Chord::new(Key::F6, ChordModifiers::NONE));

        assert_eq!(bindings.chords(action).len(), MAXIMUM_CHORDS_PER_ACTION);
        assert!(
            !bindings
                .chords(action)
                .contains(&Chord::new(Key::F6, ChordModifiers::NONE))
        );
    }

    #[test]
    fn stored_bindings_survive_a_round_trip_and_reject_broken_entries() {
        let mut bindings = ShortcutBindings::default();
        bindings.remove(ShortcutAction::FitImageWidth, 0);
        bindings.assign(
            ShortcutAction::FitImageWidth,
            None,
            Chord::new(Key::F2, ChordModifiers::NONE),
        );

        let stored: BTreeMap<String, Vec<String>> = bindings.clone().into();
        assert_eq!(
            stored.get("fit_image_width"),
            Some(&vec![String::from("F2")])
        );
        assert_eq!(ShortcutBindings::from(stored), bindings);

        let broken = BTreeMap::from([
            (
                String::from("fit_image_width"),
                vec![String::from("Nonsense"), String::from("Ctrl+F2")],
            ),
            (String::from("retired_action"), vec![String::from("F9")]),
        ]);
        let recovered = ShortcutBindings::from(broken);

        assert_eq!(
            recovered.chords(ShortcutAction::FitImageWidth),
            &[Chord::new(Key::F2, ChordModifiers::COMMAND)]
        );
        assert!(recovered.is_default(ShortcutAction::ZoomIn));
    }

    #[test]
    fn colliding_stored_bindings_keep_the_first_owner() {
        let stored = BTreeMap::from([
            (String::from("zoom_in"), vec![String::from("F")]),
            (String::from("fit_image_width"), vec![String::from("F")]),
        ]);

        let bindings = ShortcutBindings::from(stored);

        assert_eq!(
            bindings.chords(ShortcutAction::FitImageWidth),
            &[Chord::new(Key::F, ChordModifiers::NONE)]
        );
        assert!(bindings.chords(ShortcutAction::ZoomIn).is_empty());
    }

    #[test]
    fn the_clipboard_event_resolves_to_the_copy_chord() {
        let (chord, repeat) =
            chord_from_event(&Event::Copy, Modifiers::COMMAND).expect("copy should map to a chord");

        assert!(!repeat);
        assert_eq!(
            ShortcutBindings::default().action_for(chord, repeat),
            Some(ShortcutAction::CopyCrop)
        );
        assert_eq!(
            chord_from_event(
                &Event::Key {
                    key: Key::ShiftLeft,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::SHIFT,
                },
                Modifiers::SHIFT
            ),
            None
        );
    }
}
