mod button;
mod native;
pub(crate) use button::HostButton;
pub(crate) use native::GamepadHandler;

use crate::settings::{GamepadAction, WonderSwanButton};

pub(crate) struct GamepadPoll {
    pub(crate) events: Vec<(HostButton, bool)>,
    pub(crate) events_p2: Vec<(HostButton, bool)>,
    pub(crate) ws_events: Vec<(WonderSwanButton, bool)>,
    pub(crate) action_events: Vec<(GamepadAction, bool)>,
    pub(crate) left_stick: (f32, f32),
    pub(crate) raw_pressed: Vec<&'static str>,
}
