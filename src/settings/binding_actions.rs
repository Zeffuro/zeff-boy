use super::tilt_bindings::TiltBindingAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindingAction {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    X,
    Y,
    L,
    R,
    Start,
    Select,
}

impl BindingAction {
    pub(crate) const ALL: &'static [Self] = &[
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
        Self::A,
        Self::B,
        Self::X,
        Self::Y,
        Self::L,
        Self::R,
        Self::Start,
        Self::Select,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WonderSwanButton {
    X1,
    X2,
    X3,
    X4,
    Y1,
    Y2,
    Y3,
    Y4,
    A,
    B,
    Start,
}

impl WonderSwanButton {
    pub(crate) const ALL: &'static [WonderSwanButton] = &[
        Self::X1,
        Self::X2,
        Self::X3,
        Self::X4,
        Self::Y1,
        Self::Y2,
        Self::Y3,
        Self::Y4,
        Self::A,
        Self::B,
        Self::Start,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::X1 => "X1 / Up",
            Self::X2 => "X2 / Right",
            Self::X3 => "X3 / Down",
            Self::X4 => "X4 / Left",
            Self::Y1 => "Y1 / Up",
            Self::Y2 => "Y2 / Right",
            Self::Y3 => "Y3 / Down",
            Self::Y4 => "Y4 / Left",
            Self::A => "A",
            Self::B => "B",
            Self::Start => "Start",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputBindingAction {
    Joypad(BindingAction),
    JoypadP2(BindingAction),
    PceMultitap { player: u8, action: BindingAction },
    Tilt(TiltBindingAction),
    WonderSwan(WonderSwanButton),
}
