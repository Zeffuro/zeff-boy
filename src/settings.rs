use std::fs;
use std::path::{Path, PathBuf};

use serde::de::Deserializer;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use winit::keyboard::KeyCode;

use crate::hardware::types::hardware_mode::HardwareModePreference;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum AudioRecordingFormat {
    #[default]
    Wav16,
    WavFloat,
    OggVorbis,
    Midi,
}

impl AudioRecordingFormat {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Wav16 => "WAV 16-bit PCM",
            Self::WavFloat => "WAV 32-bit Float",
            Self::OggVorbis => "OGG Vorbis",
            Self::Midi => "MIDI (APU channels)",
        }
    }

    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Wav16 | Self::WavFloat => "wav",
            Self::OggVorbis => "ogg",
            Self::Midi => "mid",
        }
    }

    pub(crate) fn is_midi(self) -> bool {
        matches!(self, Self::Midi)
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindingAction {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    Start,
    Select,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GamepadAction {
    SpeedUp,
    Rewind,
    Pause,
    Turbo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TiltBindingAction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputBindingAction {
    Joypad(BindingAction),
    Tilt(TiltBindingAction),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum ScalingMode {
    #[default]
    PixelPerfect,
    HQ2xLike,
    XBR2x,
    Eagle2x,
    Bilinear,
}

impl ScalingMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::PixelPerfect => "Pixel Perfect",
            Self::HQ2xLike => "HQ2x-like",
            Self::XBR2x => "xBR 2x",
            Self::Eagle2x => "Eagle 2x",
            Self::Bilinear => "Bilinear",
        }
    }

    pub(crate) fn is_upscaler(self) -> bool {
        matches!(self, Self::HQ2xLike | Self::XBR2x | Self::Eagle2x)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum EffectPreset {
    #[default]
    None,
    CRT,
    Scanlines,
    LCDGrid,
    GbcPalette,
    Custom,
}

impl EffectPreset {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::CRT => "CRT",
            Self::Scanlines => "Scanlines",
            Self::LCDGrid => "LCD Grid",
            Self::GbcPalette => "GBC Palette",
            Self::Custom => "Custom (file)",
        }
    }
}

/// Legacy enum kept for backward-compatible deserialization of old settings.
/// Maps to the new ScalingMode + EffectPreset pair.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum ShaderPreset {
    #[default]
    None,
    CRT,
    Scanlines,
    LCDGrid,
    HQ2xLike,
    XBR2x,
    Eagle2x,
    GbcPalette,
    Custom,
}

impl ShaderPreset {
    /// Convert legacy preset to the new scaling + effect pair.
    pub(crate) fn to_scaling_and_effect(self) -> (ScalingMode, EffectPreset) {
        match self {
            Self::None => (ScalingMode::PixelPerfect, EffectPreset::None),
            Self::CRT => (ScalingMode::PixelPerfect, EffectPreset::CRT),
            Self::Scanlines => (ScalingMode::PixelPerfect, EffectPreset::Scanlines),
            Self::LCDGrid => (ScalingMode::PixelPerfect, EffectPreset::LCDGrid),
            Self::HQ2xLike => (ScalingMode::HQ2xLike, EffectPreset::None),
            Self::XBR2x => (ScalingMode::XBR2x, EffectPreset::None),
            Self::Eagle2x => (ScalingMode::Eagle2x, EffectPreset::None),
            Self::GbcPalette => (ScalingMode::PixelPerfect, EffectPreset::GbcPalette),
            Self::Custom => (ScalingMode::PixelPerfect, EffectPreset::Custom),
        }
    }
}

fn default_offscreen_scale() -> u32 {
    4
}


#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum ColorCorrection {
    #[default]
    None,
    GbcLcd,
    Custom,
}


impl ColorCorrection {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::None => "None (raw RGB)",
            Self::GbcLcd => "GBC LCD panel",
            Self::Custom => "Custom matrix",
        }
    }
}

fn default_color_correction_matrix() -> [f32; 9] {
    [
        1.0, 0.0, 0.0,
        0.0, 1.0, 0.0,
        0.0, 0.0, 1.0,
    ]
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub(crate) struct ShaderParams {
    #[serde(default = "default_scanline_intensity")]
    pub(crate) scanline_intensity: f32,
    #[serde(default = "default_crt_curvature")]
    pub(crate) crt_curvature: f32,
    #[serde(default = "default_grid_intensity")]
    pub(crate) grid_intensity: f32,
    #[serde(default = "default_upscale_edge_strength")]
    pub(crate) upscale_edge_strength: f32,
    #[serde(default = "default_palette_mix")]
    pub(crate) palette_mix: f32,
    #[serde(default = "default_palette_warmth")]
    pub(crate) palette_warmth: f32,
}

fn default_scanline_intensity() -> f32 {
    0.18
}
fn default_crt_curvature() -> f32 {
    0.3
}
fn default_grid_intensity() -> f32 {
    0.3
}
fn default_upscale_edge_strength() -> f32 {
    0.65
}
fn default_palette_mix() -> f32 {
    1.0
}
fn default_palette_warmth() -> f32 {
    0.15
}

impl Default for ShaderParams {
    fn default() -> Self {
        Self {
            scanline_intensity: default_scanline_intensity(),
            crt_curvature: default_crt_curvature(),
            grid_intensity: default_grid_intensity(),
            upscale_edge_strength: default_upscale_edge_strength(),
            palette_mix: default_palette_mix(),
            palette_warmth: default_palette_warmth(),
        }
    }
}

impl ShaderParams {
    #[cfg(test)]
    pub(crate) fn to_gpu_bytes(&self) -> [u8; 32] {
        let mut buf = [0u8; 32];
        buf[0..4].copy_from_slice(&self.scanline_intensity.to_le_bytes());
        buf[4..8].copy_from_slice(&self.crt_curvature.to_le_bytes());
        buf[8..12].copy_from_slice(&self.grid_intensity.to_le_bytes());
        buf[12..16].copy_from_slice(&self.upscale_edge_strength.to_le_bytes());
        buf[16..20].copy_from_slice(&self.palette_mix.to_le_bytes());
        buf[20..24].copy_from_slice(&self.palette_warmth.to_le_bytes());
        buf
    }
}

pub(crate) fn gbc_lcd_matrix() -> [f32; 9] {
    [
        26.0 / 32.0,  4.0 / 32.0, 2.0 / 32.0,
         0.0,         24.0 / 32.0, 8.0 / 32.0,
         6.0 / 32.0,  4.0 / 32.0, 22.0 / 32.0,
    ]
}

pub(crate) fn build_gpu_params(
    params: &ShaderParams,
    color_correction: ColorCorrection,
    color_correction_matrix: [f32; 9],
) -> [u8; 96] {
    let mut buf = [0u8; 96];
    buf[0..4].copy_from_slice(&params.scanline_intensity.to_le_bytes());
    buf[4..8].copy_from_slice(&params.crt_curvature.to_le_bytes());
    buf[8..12].copy_from_slice(&params.grid_intensity.to_le_bytes());
    buf[12..16].copy_from_slice(&params.upscale_edge_strength.to_le_bytes());
    buf[16..20].copy_from_slice(&params.palette_mix.to_le_bytes());
    buf[20..24].copy_from_slice(&params.palette_warmth.to_le_bytes());
    buf[24..28].copy_from_slice(&160.0_f32.to_le_bytes());
    buf[28..32].copy_from_slice(&144.0_f32.to_le_bytes());

    let mode: u32 = match color_correction {
        ColorCorrection::None => 0,
        ColorCorrection::GbcLcd => 1,
        ColorCorrection::Custom => 2,
    };
    buf[32..36].copy_from_slice(&mode.to_le_bytes());
    buf[36..40].copy_from_slice(&0u32.to_le_bytes());

    let matrix = match color_correction {
        ColorCorrection::None => [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        ColorCorrection::GbcLcd => gbc_lcd_matrix(),
        ColorCorrection::Custom => color_correction_matrix,
    };

    buf[48..52].copy_from_slice(&matrix[0].to_le_bytes());
    buf[52..56].copy_from_slice(&matrix[1].to_le_bytes());
    buf[56..60].copy_from_slice(&matrix[2].to_le_bytes());
    buf[60..64].copy_from_slice(&0.0_f32.to_le_bytes());

    buf[64..68].copy_from_slice(&matrix[3].to_le_bytes());
    buf[68..72].copy_from_slice(&matrix[4].to_le_bytes());
    buf[72..76].copy_from_slice(&matrix[5].to_le_bytes());
    buf[76..80].copy_from_slice(&0.0_f32.to_le_bytes());

    buf[80..84].copy_from_slice(&matrix[6].to_le_bytes());
    buf[84..88].copy_from_slice(&matrix[7].to_le_bytes());
    buf[88..92].copy_from_slice(&matrix[8].to_le_bytes());
    buf[92..96].copy_from_slice(&0.0_f32.to_le_bytes());

    buf
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum LeftStickMode {
    Dpad,
    Tilt,
    #[default]
    Auto,
}


#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum TiltInputMode {
    #[default]
    Keyboard,
    Mouse,
    Auto,
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeyBindings {
    pub(crate) up: KeyCode,
    pub(crate) down: KeyCode,
    pub(crate) left: KeyCode,
    pub(crate) right: KeyCode,
    pub(crate) a: KeyCode,
    pub(crate) b: KeyCode,
    pub(crate) start: KeyCode,
    pub(crate) select: KeyCode,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            up: KeyCode::ArrowUp,
            down: KeyCode::ArrowDown,
            left: KeyCode::ArrowLeft,
            right: KeyCode::ArrowRight,
            a: KeyCode::KeyX,
            b: KeyCode::KeyZ,
            start: KeyCode::Enter,
            select: KeyCode::ShiftRight,
        }
    }
}

impl KeyBindings {
    pub(crate) fn get(&self, action: BindingAction) -> KeyCode {
        match action {
            BindingAction::Up => self.up,
            BindingAction::Down => self.down,
            BindingAction::Left => self.left,
            BindingAction::Right => self.right,
            BindingAction::A => self.a,
            BindingAction::B => self.b,
            BindingAction::Start => self.start,
            BindingAction::Select => self.select,
        }
    }

    pub(crate) fn set(&mut self, action: BindingAction, key: KeyCode) {
        match action {
            BindingAction::Up => self.up = key,
            BindingAction::Down => self.down = key,
            BindingAction::Left => self.left = key,
            BindingAction::Right => self.right = key,
            BindingAction::A => self.a = key,
            BindingAction::B => self.b = key,
            BindingAction::Start => self.start = key,
            BindingAction::Select => self.select = key,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TiltKeyBindings {
    pub(crate) up: KeyCode,
    pub(crate) down: KeyCode,
    pub(crate) left: KeyCode,
    pub(crate) right: KeyCode,
}

impl Default for TiltKeyBindings {
    fn default() -> Self {
        Self {
            up: KeyCode::KeyW,
            down: KeyCode::KeyS,
            left: KeyCode::KeyA,
            right: KeyCode::KeyD,
        }
    }
}

impl TiltKeyBindings {
    pub(crate) fn get(&self, action: TiltBindingAction) -> KeyCode {
        match action {
            TiltBindingAction::Up => self.up,
            TiltBindingAction::Down => self.down,
            TiltBindingAction::Left => self.left,
            TiltBindingAction::Right => self.right,
        }
    }

    pub(crate) fn set(&mut self, action: TiltBindingAction, key: KeyCode) {
        match action {
            TiltBindingAction::Up => self.up = key,
            TiltBindingAction::Down => self.down = key,
            TiltBindingAction::Left => self.left = key,
            TiltBindingAction::Right => self.right = key,
        }
    }

    pub(crate) fn set_wasd_defaults(&mut self) {
        *self = Self::default();
    }
}

impl Serialize for TiltKeyBindings {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("TiltKeyBindings", 4)?;
        state.serialize_field("up", &keycode_to_string(self.up))?;
        state.serialize_field("down", &keycode_to_string(self.down))?;
        state.serialize_field("left", &keycode_to_string(self.left))?;
        state.serialize_field("right", &keycode_to_string(self.right))?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for TiltKeyBindings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawTiltKeyBindings {
            up: Option<String>,
            down: Option<String>,
            left: Option<String>,
            right: Option<String>,
        }

        let raw = RawTiltKeyBindings::deserialize(deserializer)?;
        let defaults = Self::default();
        Ok(Self {
            up: raw
                .up
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.up),
            down: raw
                .down
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.down),
            left: raw
                .left
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.left),
            right: raw
                .right
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.right),
        })
    }
}

impl Serialize for KeyBindings {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("KeyBindings", 8)?;
        state.serialize_field("up", &keycode_to_string(self.up))?;
        state.serialize_field("down", &keycode_to_string(self.down))?;
        state.serialize_field("left", &keycode_to_string(self.left))?;
        state.serialize_field("right", &keycode_to_string(self.right))?;
        state.serialize_field("a", &keycode_to_string(self.a))?;
        state.serialize_field("b", &keycode_to_string(self.b))?;
        state.serialize_field("start", &keycode_to_string(self.start))?;
        state.serialize_field("select", &keycode_to_string(self.select))?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for KeyBindings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawKeyBindings {
            up: Option<String>,
            down: Option<String>,
            left: Option<String>,
            right: Option<String>,
            a: Option<String>,
            b: Option<String>,
            start: Option<String>,
            select: Option<String>,
        }

        let raw = RawKeyBindings::deserialize(deserializer)?;
        let defaults = Self::default();
        Ok(Self {
            up: raw
                .up
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.up),
            down: raw
                .down
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.down),
            left: raw
                .left
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.left),
            right: raw
                .right
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.right),
            a: raw
                .a
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.a),
            b: raw
                .b
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.b),
            start: raw
                .start
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.start),
            select: raw
                .select
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.select),
        })
    }
}

fn keycode_to_string(code: KeyCode) -> String {
    format!("{code:?}")
}

static KEYCODE_MAP: phf::Map<&'static str, KeyCode> = phf::phf_map! {
    "ArrowUp" => KeyCode::ArrowUp,
    "ArrowDown" => KeyCode::ArrowDown,
    "ArrowLeft" => KeyCode::ArrowLeft,
    "ArrowRight" => KeyCode::ArrowRight,
    "Enter" => KeyCode::Enter,
    "ShiftLeft" => KeyCode::ShiftLeft,
    "ShiftRight" => KeyCode::ShiftRight,
    "ControlLeft" => KeyCode::ControlLeft,
    "ControlRight" => KeyCode::ControlRight,
    "AltLeft" => KeyCode::AltLeft,
    "AltRight" => KeyCode::AltRight,
    "Space" => KeyCode::Space,
    "Backspace" => KeyCode::Backspace,
    "Escape" => KeyCode::Escape,
    "Tab" => KeyCode::Tab,
    "CapsLock" => KeyCode::CapsLock,
    "Minus" => KeyCode::Minus,
    "Equal" => KeyCode::Equal,
    "BracketLeft" => KeyCode::BracketLeft,
    "BracketRight" => KeyCode::BracketRight,
    "Backslash" => KeyCode::Backslash,
    "Semicolon" => KeyCode::Semicolon,
    "Quote" => KeyCode::Quote,
    "Comma" => KeyCode::Comma,
    "Period" => KeyCode::Period,
    "Slash" => KeyCode::Slash,
    "Backquote" => KeyCode::Backquote,
    "KeyA" => KeyCode::KeyA,
    "KeyB" => KeyCode::KeyB,
    "KeyC" => KeyCode::KeyC,
    "KeyD" => KeyCode::KeyD,
    "KeyE" => KeyCode::KeyE,
    "KeyF" => KeyCode::KeyF,
    "KeyG" => KeyCode::KeyG,
    "KeyH" => KeyCode::KeyH,
    "KeyI" => KeyCode::KeyI,
    "KeyJ" => KeyCode::KeyJ,
    "KeyK" => KeyCode::KeyK,
    "KeyL" => KeyCode::KeyL,
    "KeyM" => KeyCode::KeyM,
    "KeyN" => KeyCode::KeyN,
    "KeyO" => KeyCode::KeyO,
    "KeyP" => KeyCode::KeyP,
    "KeyQ" => KeyCode::KeyQ,
    "KeyR" => KeyCode::KeyR,
    "KeyS" => KeyCode::KeyS,
    "KeyT" => KeyCode::KeyT,
    "KeyU" => KeyCode::KeyU,
    "KeyV" => KeyCode::KeyV,
    "KeyW" => KeyCode::KeyW,
    "KeyX" => KeyCode::KeyX,
    "KeyY" => KeyCode::KeyY,
    "KeyZ" => KeyCode::KeyZ,
    "Digit0" => KeyCode::Digit0,
    "Digit1" => KeyCode::Digit1,
    "Digit2" => KeyCode::Digit2,
    "Digit3" => KeyCode::Digit3,
    "Digit4" => KeyCode::Digit4,
    "Digit5" => KeyCode::Digit5,
    "Digit6" => KeyCode::Digit6,
    "Digit7" => KeyCode::Digit7,
    "Digit8" => KeyCode::Digit8,
    "Digit9" => KeyCode::Digit9,
    "F1" => KeyCode::F1,
    "F2" => KeyCode::F2,
    "F3" => KeyCode::F3,
    "F4" => KeyCode::F4,
    "F5" => KeyCode::F5,
    "F6" => KeyCode::F6,
    "F7" => KeyCode::F7,
    "F8" => KeyCode::F8,
    "F9" => KeyCode::F9,
    "F10" => KeyCode::F10,
    "F11" => KeyCode::F11,
    "F12" => KeyCode::F12,
    "Pause" => KeyCode::Pause,
};

fn keycode_from_string(name: &str) -> Option<KeyCode> {
    KEYCODE_MAP.get(name).copied()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShortcutAction {
    Pause,
    Fullscreen,
    UncappedSpeed,
    MuteToggle,
    Screenshot,
    ResetGame,
    FrameAdvance,
    QuickSave,
    QuickLoad,
    SlotNext,
    SlotPrev,
    DebugContinue,
    DebugStep,
}

impl ShortcutAction {
    pub(crate) const ALL: &'static [ShortcutAction] = &[
        Self::Pause,
        Self::Fullscreen,
        Self::UncappedSpeed,
        Self::MuteToggle,
        Self::Screenshot,
        Self::ResetGame,
        Self::FrameAdvance,
        Self::QuickSave,
        Self::QuickLoad,
        Self::SlotNext,
        Self::SlotPrev,
        Self::DebugContinue,
        Self::DebugStep,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Pause => "Pause / Resume",
            Self::Fullscreen => "Fullscreen",
            Self::UncappedSpeed => "Toggle uncapped",
            Self::MuteToggle => "Mute toggle",
            Self::Screenshot => "Screenshot",
            Self::ResetGame => "Reset game",
            Self::FrameAdvance => "Frame advance",
            Self::QuickSave => "Quick save",
            Self::QuickLoad => "Quick load",
            Self::SlotNext => "Next save slot",
            Self::SlotPrev => "Prev save slot",
            Self::DebugContinue => "Run (debug)",
            Self::DebugStep => "Step (debug)",
        }
    }

    fn default_keycode(self) -> KeyCode {
        match self {
            Self::Pause => KeyCode::KeyP,
            Self::Fullscreen => KeyCode::F11,
            Self::UncappedSpeed => KeyCode::F10,
            Self::MuteToggle => KeyCode::KeyM,
            Self::Screenshot => KeyCode::F12,
            Self::ResetGame => KeyCode::F6,
            Self::FrameAdvance => KeyCode::KeyN,
            Self::QuickSave => KeyCode::F5,
            Self::QuickLoad => KeyCode::F8,
            Self::SlotNext => KeyCode::BracketRight,
            Self::SlotPrev => KeyCode::BracketLeft,
            Self::DebugContinue => KeyCode::F9,
            Self::DebugStep => KeyCode::F7,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct ShortcutBindings {
    pub(crate) pause: String,
    pub(crate) fullscreen: String,
    pub(crate) uncapped_speed: String,
    pub(crate) mute_toggle: String,
    pub(crate) screenshot: String,
    pub(crate) reset_game: String,
    pub(crate) frame_advance: String,
    pub(crate) quick_save: String,
    pub(crate) quick_load: String,
    pub(crate) slot_next: String,
    pub(crate) slot_prev: String,
    pub(crate) debug_continue: String,
    pub(crate) debug_step: String,
}

impl Default for ShortcutBindings {
    fn default() -> Self {
        Self {
            pause: "KeyP".to_string(),
            fullscreen: "F11".to_string(),
            uncapped_speed: "F10".to_string(),
            mute_toggle: "KeyM".to_string(),
            screenshot: "F12".to_string(),
            reset_game: "F6".to_string(),
            frame_advance: "KeyN".to_string(),
            quick_save: "F5".to_string(),
            quick_load: "F8".to_string(),
            slot_next: "BracketRight".to_string(),
            slot_prev: "BracketLeft".to_string(),
            debug_continue: "F9".to_string(),
            debug_step: "F7".to_string(),
        }
    }
}

impl ShortcutBindings {
    pub(crate) fn get(&self, action: ShortcutAction) -> KeyCode {
        let s = self.key_str(action);
        keycode_from_string(s).unwrap_or(action.default_keycode())
    }

    pub(crate) fn set(&mut self, action: ShortcutAction, key: KeyCode) {
        let s = keycode_to_string(key);
        match action {
            ShortcutAction::Pause => self.pause = s,
            ShortcutAction::Fullscreen => self.fullscreen = s,
            ShortcutAction::UncappedSpeed => self.uncapped_speed = s,
            ShortcutAction::MuteToggle => self.mute_toggle = s,
            ShortcutAction::Screenshot => self.screenshot = s,
            ShortcutAction::ResetGame => self.reset_game = s,
            ShortcutAction::FrameAdvance => self.frame_advance = s,
            ShortcutAction::QuickSave => self.quick_save = s,
            ShortcutAction::QuickLoad => self.quick_load = s,
            ShortcutAction::SlotNext => self.slot_next = s,
            ShortcutAction::SlotPrev => self.slot_prev = s,
            ShortcutAction::DebugContinue => self.debug_continue = s,
            ShortcutAction::DebugStep => self.debug_step = s,
        }
    }

    pub(crate) fn key_str(&self, action: ShortcutAction) -> &str {
        match action {
            ShortcutAction::Pause => &self.pause,
            ShortcutAction::Fullscreen => &self.fullscreen,
            ShortcutAction::UncappedSpeed => &self.uncapped_speed,
            ShortcutAction::MuteToggle => &self.mute_toggle,
            ShortcutAction::Screenshot => &self.screenshot,
            ShortcutAction::ResetGame => &self.reset_game,
            ShortcutAction::FrameAdvance => &self.frame_advance,
            ShortcutAction::QuickSave => &self.quick_save,
            ShortcutAction::QuickLoad => &self.quick_load,
            ShortcutAction::SlotNext => &self.slot_next,
            ShortcutAction::SlotPrev => &self.slot_prev,
            ShortcutAction::DebugContinue => &self.debug_continue,
            ShortcutAction::DebugStep => &self.debug_step,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct GamepadBindings {
    pub(crate) a: String,
    pub(crate) b: String,
    pub(crate) start: String,
    pub(crate) select: String,
    pub(crate) up: String,
    pub(crate) down: String,
    pub(crate) left: String,
    pub(crate) right: String,
    #[serde(default)]
    pub(crate) speedup: String,
    #[serde(default)]
    pub(crate) rewind: String,
    #[serde(default)]
    pub(crate) pause: String,
    #[serde(default)]
    pub(crate) turbo: String,
}

impl Default for GamepadBindings {
    fn default() -> Self {
        Self {
            a: "South".to_string(),
            b: "East".to_string(),
            start: "Start".to_string(),
            select: "Select".to_string(),
            up: "DPadUp".to_string(),
            down: "DPadDown".to_string(),
            left: "DPadLeft".to_string(),
            right: "DPadRight".to_string(),
            speedup: String::new(),
            rewind: String::new(),
            pause: String::new(),
            turbo: String::new(),
        }
    }
}

impl GamepadBindings {
    pub(crate) fn map_button_name(&self, name: &str) -> Option<crate::hardware::joypad::JoypadKey> {
        use crate::hardware::joypad::JoypadKey;
        if name == self.a {
            return Some(JoypadKey::A);
        }
        if name == self.b {
            return Some(JoypadKey::B);
        }
        if name == self.start {
            return Some(JoypadKey::Start);
        }
        if name == self.select {
            return Some(JoypadKey::Select);
        }
        if name == self.up {
            return Some(JoypadKey::Up);
        }
        if name == self.down {
            return Some(JoypadKey::Down);
        }
        if name == self.left {
            return Some(JoypadKey::Left);
        }
        if name == self.right {
            return Some(JoypadKey::Right);
        }
        None
    }

    pub(crate) fn map_action_button_name(&self, name: &str) -> Option<GamepadAction> {
        if !self.speedup.is_empty() && name == self.speedup {
            return Some(GamepadAction::SpeedUp);
        }
        if !self.rewind.is_empty() && name == self.rewind {
            return Some(GamepadAction::Rewind);
        }
        if !self.pause.is_empty() && name == self.pause {
            return Some(GamepadAction::Pause);
        }
        if !self.turbo.is_empty() && name == self.turbo {
            return Some(GamepadAction::Turbo);
        }
        None
    }

    pub(crate) fn get(&self, action: BindingAction) -> &str {
        match action {
            BindingAction::A => &self.a,
            BindingAction::B => &self.b,
            BindingAction::Start => &self.start,
            BindingAction::Select => &self.select,
            BindingAction::Up => &self.up,
            BindingAction::Down => &self.down,
            BindingAction::Left => &self.left,
            BindingAction::Right => &self.right,
        }
    }

    pub(crate) fn set(&mut self, action: BindingAction, button_name: &str) {
        let s = button_name.to_string();
        match action {
            BindingAction::A => self.a = s,
            BindingAction::B => self.b = s,
            BindingAction::Start => self.start = s,
            BindingAction::Select => self.select = s,
            BindingAction::Up => self.up = s,
            BindingAction::Down => self.down = s,
            BindingAction::Left => self.left = s,
            BindingAction::Right => self.right = s,
        }
    }

    pub(crate) fn get_action(&self, action: GamepadAction) -> &str {
        match action {
            GamepadAction::SpeedUp => &self.speedup,
            GamepadAction::Rewind => &self.rewind,
            GamepadAction::Pause => &self.pause,
            GamepadAction::Turbo => &self.turbo,
        }
    }

    pub(crate) fn set_action(&mut self, action: GamepadAction, button_name: &str) {
        let s = button_name.to_string();
        match action {
            GamepadAction::SpeedUp => self.speedup = s,
            GamepadAction::Rewind => self.rewind = s,
            GamepadAction::Pause => self.pause = s,
            GamepadAction::Turbo => self.turbo = s,
        }
    }
}

const MAX_RECENT_ROMS: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct RecentRomEntry {
    pub(crate) path: String,
    pub(crate) name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct Settings {
    pub(crate) hardware_mode_preference: HardwareModePreference,
    pub(crate) fast_forward_multiplier: usize,
    pub(crate) uncapped_frames_per_tick: usize,
    pub(crate) uncapped_speed: bool,
    pub(crate) show_fps: bool,
    pub(crate) key_bindings: KeyBindings,
    pub(crate) tilt_key_bindings: TiltKeyBindings,
    pub(crate) left_stick_mode: LeftStickMode,
    pub(crate) tilt_input_mode: TiltInputMode,
    pub(crate) tilt_sensitivity: f32,
    pub(crate) tilt_lerp: f32,
    pub(crate) tilt_deadzone: f32,
    pub(crate) tilt_invert_x: bool,
    pub(crate) tilt_invert_y: bool,
    pub(crate) stick_tilt_bypass_lerp: bool,
    pub(crate) master_volume: f32,
    #[serde(skip)]
    pub(crate) pre_mute_volume: Option<f32>,
    pub(crate) mute_audio_during_fast_forward: bool,
    #[serde(default)]
    pub(crate) audio_recording_format: AudioRecordingFormat,
    pub(crate) frame_skip: bool,
    pub(crate) enable_memory_editing: bool,
    pub(crate) auto_save_state: bool,
    pub(crate) recent_roms: Vec<RecentRomEntry>,
    pub(crate) speedup_key: String,
    pub(crate) rewind_enabled: bool,
    pub(crate) rewind_key: String,
    #[serde(default = "default_rewind_speed")]
    pub(crate) rewind_speed: usize,
    #[serde(default = "default_rewind_seconds")]
    pub(crate) rewind_seconds: usize,
    pub(crate) shader_preset: ShaderPreset,
    #[serde(default)]
    pub(crate) scaling_mode: ScalingMode,
    #[serde(default)]
    pub(crate) effect_preset: EffectPreset,
    #[serde(default = "default_offscreen_scale")]
    pub(crate) offscreen_scale: u32,
    #[serde(default)]
    pub(crate) shader_params: ShaderParams,
    #[serde(default)]
    pub(crate) custom_shader_path: String,
    #[serde(default)]
    pub(crate) color_correction: ColorCorrection,
    #[serde(default = "default_color_correction_matrix")]
    pub(crate) color_correction_matrix: [f32; 9],
    #[serde(default)]
    pub(crate) autohide_menu_bar: bool,
    #[serde(default = "default_ui_scale")]
    pub(crate) ui_scale: f32,
    #[serde(skip)]
    pub(crate) ui_scale_needs_auto: bool,
    #[serde(default)]
    pub(crate) shortcut_bindings: ShortcutBindings,
    #[serde(default)]
    pub(crate) gamepad_bindings: GamepadBindings,
    pub(crate) open_debug_tabs: Vec<String>,
}

fn default_rewind_speed() -> usize {
    3
}
fn default_rewind_seconds() -> usize {
    10
}
fn default_ui_scale() -> f32 {
    1.0
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hardware_mode_preference: HardwareModePreference::Auto,
            fast_forward_multiplier: 4,
            uncapped_frames_per_tick: 60,
            uncapped_speed: false,
            show_fps: true,
            key_bindings: KeyBindings::default(),
            tilt_key_bindings: TiltKeyBindings::default(),
            left_stick_mode: LeftStickMode::Auto,
            tilt_input_mode: TiltInputMode::default(),
            tilt_sensitivity: 1.0,
            tilt_lerp: 0.25,
            tilt_deadzone: 0.12,
            tilt_invert_x: false,
            tilt_invert_y: false,
            stick_tilt_bypass_lerp: true,
            master_volume: 1.0,
            pre_mute_volume: None,
            mute_audio_during_fast_forward: false,
            audio_recording_format: AudioRecordingFormat::default(),
            frame_skip: false,
            enable_memory_editing: false,
            auto_save_state: false,
            recent_roms: Vec::new(),
            speedup_key: "Space".to_string(),
            rewind_enabled: true,
            rewind_key: "KeyR".to_string(),
            rewind_speed: default_rewind_speed(), // 3 = normal
            rewind_seconds: default_rewind_seconds(),
            shader_preset: ShaderPreset::None,
            scaling_mode: ScalingMode::PixelPerfect,
            effect_preset: EffectPreset::None,
            offscreen_scale: default_offscreen_scale(),
            shader_params: ShaderParams::default(),
            custom_shader_path: String::new(),
            color_correction: ColorCorrection::None,
            color_correction_matrix: default_color_correction_matrix(),
            autohide_menu_bar: false,
            ui_scale: default_ui_scale(),
            ui_scale_needs_auto: false,
            shortcut_bindings: ShortcutBindings::default(),
            gamepad_bindings: GamepadBindings::default(),
            open_debug_tabs: Vec::new(),
        }
    }
}

impl Settings {
    pub(crate) fn speedup_key_code(&self) -> KeyCode {
        keycode_from_string(&self.speedup_key).unwrap_or(KeyCode::Backquote)
    }

    pub(crate) fn rewind_key_code(&self) -> KeyCode {
        keycode_from_string(&self.rewind_key).unwrap_or(KeyCode::KeyR)
    }

    pub(crate) fn rewind_capture_interval(&self) -> usize {
        4
    }

    pub(crate) fn add_recent_rom(&mut self, path: &Path) {
        let path_str = path.to_string_lossy().to_string();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();

        self.recent_roms.retain(|r| r.path != path_str);

        self.recent_roms.insert(
            0,
            RecentRomEntry {
                path: path_str,
                name,
            },
        );

        self.recent_roms.truncate(MAX_RECENT_ROMS);
    }

    fn legacy_path() -> PathBuf {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("settings.json")
    }

    fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|base| base.join("zeff-boy").join("settings.json"))
    }

    fn active_path() -> PathBuf {
        Self::config_path().unwrap_or_else(Self::legacy_path)
    }

    pub(crate) fn settings_dir() -> PathBuf {
        Self::active_path()
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    fn load_from_path(path: &Path) -> Option<Self> {
        let bytes = fs::read(path).ok()?;
        serde_json::from_slice::<Self>(&bytes).ok()
    }

    fn save_to_path(&self, path: &Path) {
        let Ok(serialized) = serde_json::to_vec_pretty(self) else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, serialized);
    }

    pub(crate) fn load_or_default() -> Self {
        let mut settings = if let Some(config_path) = Self::config_path() {
            if let Some(settings) = Self::load_from_path(&config_path) {
                settings
            } else {
                let legacy_path = Self::legacy_path();
                if let Some(settings) = Self::load_from_path(&legacy_path) {
                    settings.save_to_path(&config_path);
                    settings
                } else {
                    let mut s = Self::default();
                    s.ui_scale_needs_auto = true;
                    s
                }
            }
        } else {
            Self::load_from_path(&Self::legacy_path()).unwrap_or_else(|| {
                let mut s = Self::default();
                s.ui_scale_needs_auto = true;
                s
            })
        };

        settings.migrate_shader_preset();
        settings
    }

    /// Choose a suitable UI scale based on the monitor resolution and OS scale factor.
    /// Called once at first launch after the window is created.
    pub(crate) fn auto_detect_ui_scale(&mut self, monitor_height: u32, os_scale_factor: f64) {
        if !self.ui_scale_needs_auto {
            return;
        }
        self.ui_scale_needs_auto = false;

        if os_scale_factor > 1.1 {
            self.ui_scale = 1.0;
            return;
        }

        self.ui_scale = match monitor_height {
            0..=900 => 1.0,
            901..=1600 => 1.0,
            _ => 1.25,
        };
    }

    fn migrate_shader_preset(&mut self) {
        if self.shader_preset != ShaderPreset::None
            && self.scaling_mode == ScalingMode::PixelPerfect
            && self.effect_preset == EffectPreset::None
        {
            let (scaling, effect) = self.shader_preset.to_scaling_and_effect();
            self.scaling_mode = scaling;
            self.effect_preset = effect;
        }
    }

    pub(crate) fn save(&self) {
        self.save_to_path(&Self::active_path());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_roundtrip() {
        let defaults = Settings::default();
        let json = serde_json::to_string_pretty(&defaults).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(defaults, restored);
    }

    #[test]
    fn settings_with_modified_values_roundtrip() {
        let mut s = Settings::default();
        s.fast_forward_multiplier = 8;
        s.master_volume = 0.5;
        s.rewind_speed = 5;
        s.rewind_seconds = 30;
        s.rewind_enabled = false;
        s.shader_preset = ShaderPreset::CRT;
        s.custom_shader_path = "C:/shaders/custom.wgsl".to_string();
        s.autohide_menu_bar = true;
        s.frame_skip = true;

        let json = serde_json::to_string(&s).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, restored);
    }

    #[test]
    fn settings_backward_compat_missing_fields_use_defaults() {
        let json = r#"{"hardware_mode_preference":"Auto","fast_forward_multiplier":4}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.rewind_speed, default_rewind_speed());
        assert_eq!(s.rewind_seconds, default_rewind_seconds());
        assert_eq!(s.shader_preset, ShaderPreset::None);
        assert!(!s.autohide_menu_bar);
    }

    #[test]
    fn key_bindings_serde_roundtrip() {
        let mut bindings = KeyBindings::default();
        bindings.a = KeyCode::KeyQ;
        bindings.b = KeyCode::KeyE;

        let json = serde_json::to_string(&bindings).unwrap();
        let restored: KeyBindings = serde_json::from_str(&json).unwrap();
        assert_eq!(bindings, restored);
    }

    #[test]
    fn key_bindings_deserialize_unknown_falls_back_to_defaults() {
        let json = r#"{"up":"ArrowUp","down":"ArrowDown","left":"UNKNOWN_KEY","right":"ArrowRight","a":"KeyZ","b":"KeyX","start":"Enter","select":"ShiftRight"}"#;
        let bindings: KeyBindings = serde_json::from_str(json).unwrap();
        assert_eq!(bindings.left, KeyCode::ArrowLeft);
        assert_eq!(bindings.up, KeyCode::ArrowUp);
    }

    #[test]
    fn shortcut_bindings_get_returns_default_for_unknown_string() {
        let mut bindings = ShortcutBindings::default();
        bindings.fullscreen = "NONSENSE".to_string();

        assert_eq!(bindings.get(ShortcutAction::Fullscreen), KeyCode::F11);
    }

    #[test]
    fn shortcut_bindings_set_and_get() {
        let mut bindings = ShortcutBindings::default();
        bindings.set(ShortcutAction::Pause, KeyCode::KeyP);
        assert_eq!(bindings.get(ShortcutAction::Pause), KeyCode::KeyP);
    }

    #[test]
    fn gamepad_bindings_roundtrip() {
        let mut gb = GamepadBindings::default();
        gb.set(BindingAction::A, "West");
        let json = serde_json::to_string(&gb).unwrap();
        let restored: GamepadBindings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.get(BindingAction::A), "West");
        assert_eq!(restored.get(BindingAction::B), "East");
    }

    #[test]
    fn tilt_key_bindings_serde_roundtrip() {
        let mut bindings = TiltKeyBindings::default();
        bindings.up = KeyCode::KeyI;
        let json = serde_json::to_string(&bindings).unwrap();
        let restored: TiltKeyBindings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.up, KeyCode::KeyI);
        assert_eq!(restored.down, KeyCode::KeyS);
    }

    #[test]
    fn recent_roms_add_and_dedup() {
        let mut s = Settings::default();
        s.add_recent_rom(Path::new("game1.gb"));
        s.add_recent_rom(Path::new("game2.gb"));
        s.add_recent_rom(Path::new("game1.gb"));
        assert_eq!(s.recent_roms.len(), 2);
        assert_eq!(s.recent_roms[0].name, "game1.gb");
        assert_eq!(s.recent_roms[1].name, "game2.gb");
    }

    #[test]
    fn recent_roms_truncates_at_max() {
        let mut s = Settings::default();
        for i in 0..15 {
            s.add_recent_rom(Path::new(&format!("game{i}.gb")));
        }
        assert_eq!(s.recent_roms.len(), MAX_RECENT_ROMS);
    }

    #[test]
    fn default_rewind_speed_is_3() {
        assert_eq!(Settings::default().rewind_speed, 3);
    }

    #[test]
    fn pre_mute_volume_is_skipped_in_serde() {
        let mut s = Settings::default();
        s.pre_mute_volume = Some(0.75);
        let json = serde_json::to_string(&s).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.pre_mute_volume, None);
    }

    #[test]
    fn shader_params_roundtrip() {
        let params = ShaderParams {
            scanline_intensity: 0.5,
            crt_curvature: 0.8,
            grid_intensity: 0.1,
            upscale_edge_strength: 0.75,
            palette_mix: 0.9,
            palette_warmth: 0.2,
        };
        let json = serde_json::to_string(&params).unwrap();
        let restored: ShaderParams = serde_json::from_str(&json).unwrap();
        assert_eq!(params, restored);
    }

    #[test]
    fn shader_params_to_gpu_bytes() {
        let params = ShaderParams::default();
        let bytes = params.to_gpu_bytes();
        let scanline = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let curvature = f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let edge = f32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        let mix = f32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        assert!((scanline - params.scanline_intensity).abs() < f32::EPSILON);
        assert!((curvature - params.crt_curvature).abs() < f32::EPSILON);
        assert!((edge - params.upscale_edge_strength).abs() < f32::EPSILON);
        assert!((mix - params.palette_mix).abs() < f32::EPSILON);
    }

    #[test]
    fn build_gpu_params_includes_color_correction() {
        let params = ShaderParams::default();
        let buf = build_gpu_params(&params, ColorCorrection::GbcLcd, default_color_correction_matrix());
        let mode = u32::from_le_bytes([buf[32], buf[33], buf[34], buf[35]]);
        assert_eq!(mode, 1);
        let r00 = f32::from_le_bytes([buf[48], buf[49], buf[50], buf[51]]);
        assert!((r00 - 26.0 / 32.0).abs() < f32::EPSILON);
    }

    #[test]
    fn build_gpu_params_none_mode_is_identity() {
        let params = ShaderParams::default();
        let buf = build_gpu_params(&params, ColorCorrection::None, default_color_correction_matrix());
        let mode = u32::from_le_bytes([buf[32], buf[33], buf[34], buf[35]]);
        assert_eq!(mode, 0);
        let r00 = f32::from_le_bytes([buf[48], buf[49], buf[50], buf[51]]);
        assert!((r00 - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rewind_capture_interval_is_4() {
        let s = Settings::default();
        assert_eq!(s.rewind_capture_interval(), 4);
    }

    #[test]
    fn color_correction_serde_roundtrip() {
        let mut s = Settings::default();
        s.color_correction = ColorCorrection::GbcLcd;
        let json = serde_json::to_string(&s).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.color_correction, ColorCorrection::GbcLcd);
    }

    #[test]
    fn color_correction_defaults_to_none_when_missing() {
        let json = r#"{"hardware_mode_preference":"Auto","fast_forward_multiplier":4}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.color_correction, ColorCorrection::None);
        assert_eq!(s.color_correction_matrix, default_color_correction_matrix());
    }

    #[test]
    fn custom_color_correction_matrix_roundtrip() {
        let mut s = Settings::default();
        s.color_correction = ColorCorrection::Custom;
        s.color_correction_matrix = [
            1.0, 0.2, 0.0,
            0.1, 0.9, 0.0,
            0.0, 0.3, 0.8,
        ];
        let json = serde_json::to_string(&s).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.color_correction, ColorCorrection::Custom);
        assert_eq!(restored.color_correction_matrix, s.color_correction_matrix);
    }
}
