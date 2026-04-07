use crate::settings::GamepadBindings;

use super::GamepadPoll;

pub(crate) struct GamepadHandler;

impl GamepadHandler {
    pub(crate) fn new() -> anyhow::Result<Self> {
        anyhow::bail!("gamepad not supported on web")
    }

    pub(crate) fn poll(&mut self, _bindings: &GamepadBindings) -> GamepadPoll {
        GamepadPoll {
            events: Vec::new(),
            action_events: Vec::new(),
            left_stick: (0.0, 0.0),
            raw_pressed: Vec::new(),
        }
    }

    pub(crate) fn set_rumble(&mut self, _active: bool) {}
}
