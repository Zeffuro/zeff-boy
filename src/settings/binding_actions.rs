use super::tilt_bindings::TiltBindingAction;

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
pub(crate) enum InputBindingAction {
    Joypad(BindingAction),
    Tilt(TiltBindingAction),
}

