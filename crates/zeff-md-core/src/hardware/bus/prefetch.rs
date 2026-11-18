#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Prefetch {
    #[default]
    Empty,
    UpperOnly(u8),
    Full(u16),
}

impl Prefetch {
    pub(crate) fn sample(self, lanes: zeff_m68k::bus::ByteLanes) -> Option<u16> {
        use zeff_m68k::bus::ByteLanes;
        match lanes {
            ByteLanes::Both => self.word(),
            ByteLanes::Upper => self.upper().map(|value| u16::from(value) << 8),
            ByteLanes::Lower => self.word().map(|value| value & 0xff),
        }
    }

    pub(crate) fn upper(self) -> Option<u8> {
        match self {
            Self::Empty => None,
            Self::UpperOnly(value) => Some(value),
            Self::Full(value) => Some((value >> 8) as u8),
        }
    }

    pub(crate) fn word(self) -> Option<u16> {
        match self {
            Self::Full(value) => Some(value),
            _ => None,
        }
    }
}
