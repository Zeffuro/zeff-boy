mod native;
pub(crate) use native::GamepadHandler;

use crate::settings::{GamepadAction, WonderSwanButton};
use zeff_gb_core::hardware::joypad::JoypadKey;

pub(crate) struct GamepadPoll {
    pub(crate) events: Vec<(JoypadKey, bool)>,
    pub(crate) ws_events: Vec<(WonderSwanButton, bool)>,
    pub(crate) action_events: Vec<(GamepadAction, bool)>,
    pub(crate) left_stick: (f32, f32),
    pub(crate) raw_pressed: Vec<&'static str>,
}
