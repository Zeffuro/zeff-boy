#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::GamepadHandler;

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub(crate) use wasm::GamepadHandler;

use crate::settings::GamepadAction;
use zeff_gb_core::hardware::joypad::JoypadKey;

pub(crate) struct GamepadPoll {
    pub(crate) events: Vec<(JoypadKey, bool)>,
    pub(crate) action_events: Vec<(GamepadAction, bool)>,
    pub(crate) left_stick: (f32, f32),
    pub(crate) raw_pressed: Vec<&'static str>,
}
