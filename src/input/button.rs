#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum HostButton {
    Right,
    Left,
    Up,
    Down,
    A,
    B,
    X,
    Y,
    Select,
    Start,
    L,
    R,
}

impl HostButton {
    pub(crate) const STANDARD: &'static [Self] = &[
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
        Self::A,
        Self::B,
        Self::Start,
        Self::Select,
    ];

    pub(crate) const WITH_SHOULDERS: &'static [Self] = &[
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
        Self::A,
        Self::B,
        Self::L,
        Self::R,
        Self::Start,
        Self::Select,
    ];

    pub(crate) const WITH_SIX_BUTTONS: &'static [Self] = &[
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

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match normalized_name(name).as_str() {
            "right" => Some(Self::Right),
            "left" => Some(Self::Left),
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            "a" => Some(Self::A),
            "b" => Some(Self::B),
            "x" => Some(Self::X),
            "y" => Some(Self::Y),
            "select" => Some(Self::Select),
            "start" => Some(Self::Start),
            "l" | "leftshoulder" | "shoulderleft" | "lb" | "l1" => Some(Self::L),
            "r" | "rightshoulder" | "shoulderright" | "rb" | "r1" => Some(Self::R),
            _ => None,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Right => "Right",
            Self::Left => "Left",
            Self::Up => "Up",
            Self::Down => "Down",
            Self::A => "A",
            Self::B => "B",
            Self::X => "X",
            Self::Y => "Y",
            Self::Select => "Select",
            Self::Start => "Start",
            Self::L => "L",
            Self::R => "R",
        }
    }

    pub(crate) fn host_mask_bit(self) -> u16 {
        match self {
            Self::Right => 1 << 0,
            Self::Left => 1 << 1,
            Self::Up => 1 << 2,
            Self::Down => 1 << 3,
            Self::A => 1 << 4,
            Self::B => 1 << 5,
            Self::Select => 1 << 6,
            Self::Start => 1 << 7,
            Self::L => 1 << 8,
            Self::R => 1 << 9,
            Self::X => 1 << 10,
            Self::Y => 1 << 11,
        }
    }
}

fn normalized_name(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '_' | '-' | ' '))
        .flat_map(char::to_lowercase)
        .collect()
}
