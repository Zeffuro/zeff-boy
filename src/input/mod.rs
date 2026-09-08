mod button;
mod native;
mod routing;
pub(crate) mod timing;
pub(crate) mod transforms;
pub(crate) use button::HostButton;
pub(crate) use native::GamepadHandler;
pub(crate) use routing::{GamepadCommand, GamepadSnapshot};
// These names form the device/settings UI contract.
pub(crate) use routing::GamepadDeviceSnapshot;
pub(crate) use routing::{GamepadAssignmentStatus, RuntimeGamepadId};

use crate::settings::{GamepadAction, WonderSwanButton};

pub(crate) struct GamepadPoll {
    pub(crate) timing: Option<timing::InputPollTiming>,
    pub(crate) events: Vec<(HostButton, bool)>,
    pub(crate) events_p2: Vec<(HostButton, bool)>,
    pub(crate) events_p3: Vec<(HostButton, bool)>,
    pub(crate) events_p4: Vec<(HostButton, bool)>,
    pub(crate) events_p5: Vec<(HostButton, bool)>,
    pub(crate) ws_events: Vec<(WonderSwanButton, bool)>,
    pub(crate) action_events: Vec<(GamepadAction, bool)>,
    pub(crate) left_stick: (f32, f32),
    pub(crate) player_sticks: [(f32, f32); 5],
    pub(crate) raw_pressed: Vec<routing::GamepadRawPress>,
}
