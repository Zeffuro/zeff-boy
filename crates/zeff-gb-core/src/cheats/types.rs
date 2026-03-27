#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheatType {
    GameShark,
    GameGenie,
    XPloder, // Also known as CodeBreaker overseas
    Raw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum CheatValue {
    Constant(u8),
    PreserveWithCurrent { mask: u8, base: u8 },
    UserParameterized { mask: u8, base: u8 },
}

impl CheatValue {
    pub const fn constant(value: u8) -> Self {
        Self::Constant(value)
    }

    pub fn from_gameshark_value(token: &str) -> Option<Self> {
        if token.len() != 2 {
            return None;
        }

        let mut mask = 0u8;
        let mut base = 0u8;

        for (i, c) in token.chars().enumerate() {
            let shift = ((1 - i) * 4) as u8;
            match c {
                '?' | 'X' | 'x' | 'Y' | 'y' => {
                    mask |= 0x0F << shift;
                }
                _ => {
                    let nibble = c.to_digit(16)? as u8;
                    base |= nibble << shift;
                }
            }
        }

        if mask == 0 {
            Some(Self::Constant(base))
        } else {
            Some(Self::UserParameterized { mask, base })
        }
    }

    #[allow(dead_code)]
    pub fn from_mask_base_preserve(mask: u8, base: u8) -> Self {
        if mask == 0 {
            Self::Constant(base)
        } else {
            Self::PreserveWithCurrent { mask, base }
        }
    }

    #[allow(dead_code)]
    pub fn from_mask_base_user(mask: u8, base: u8) -> Self {
        if mask == 0 {
            Self::Constant(base)
        } else {
            Self::UserParameterized { mask, base }
        }
    }

    pub fn has_user_parameter(self) -> bool {
        matches!(self, Self::UserParameterized { .. })
    }

    pub fn default_user_value(self) -> Option<u8> {
        match self {
            Self::UserParameterized { base, .. } => Some(base),
            _ => None,
        }
    }

    pub fn resolve_user_parameter(self, user_value: u8) -> Self {
        match self {
            Self::UserParameterized { mask, base } => Self::Constant((user_value & mask) | base),
            _ => self,
        }
    }

    pub fn resolve_with_current(self, current: u8) -> u8 {
        match self {
            Self::Constant(value) => value,
            Self::PreserveWithCurrent { mask, base } | Self::UserParameterized { mask, base } => {
                (current & mask) | base
            }
        }
    }

    pub fn matches(self, observed: u8) -> bool {
        match self {
            Self::Constant(value) => observed == value,
            Self::PreserveWithCurrent { mask, base } | Self::UserParameterized { mask, base } => {
                (observed & !mask) == base
            }
        }
    }

    pub fn display(self) -> String {
        match self {
            Self::Constant(value) => format!("{value:02X}"),
            Self::PreserveWithCurrent { mask, base } | Self::UserParameterized { mask, base } => {
                let hi = if (mask & 0xF0) == 0xF0 {
                    '?'
                } else {
                    nybble_to_hex((base >> 4) & 0x0F)
                };
                let lo = if (mask & 0x0F) == 0x0F {
                    '?'
                } else {
                    nybble_to_hex(base & 0x0F)
                };
                format!("{hi}{lo}")
            }
        }
    }
}

fn nybble_to_hex(v: u8) -> char {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    HEX[v as usize] as char
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheatPatch {
    RamWrite {
        address: u16,
        value: CheatValue,
    },
    RomWrite {
        address: u16,
        value: CheatValue,
    },
    RomWriteIfEquals {
        address: u16,
        value: CheatValue,
        compare: CheatValue,
    },
    RamWriteIfEquals {
        address: u16,
        value: CheatValue,
        compare: CheatValue,
    },
}

impl CheatPatch {
    pub fn has_user_parameter(self) -> bool {
        match self {
            Self::RamWrite { value, .. } | Self::RomWrite { value, .. } => {
                value.has_user_parameter()
            }
            Self::RomWriteIfEquals { value, compare, .. }
            | Self::RamWriteIfEquals { value, compare, .. } => {
                value.has_user_parameter() || compare.has_user_parameter()
            }
        }
    }

    pub fn default_user_value(self) -> Option<u8> {
        match self {
            Self::RamWrite { value, .. } | Self::RomWrite { value, .. } => {
                value.default_user_value()
            }
            Self::RomWriteIfEquals { value, compare, .. }
            | Self::RamWriteIfEquals { value, compare, .. } => value
                .default_user_value()
                .or_else(|| compare.default_user_value()),
        }
    }

    pub fn resolve_user_parameter(self, user_value: u8) -> Self {
        match self {
            Self::RamWrite { address, value } => Self::RamWrite {
                address,
                value: value.resolve_user_parameter(user_value),
            },
            Self::RomWrite { address, value } => Self::RomWrite {
                address,
                value: value.resolve_user_parameter(user_value),
            },
            Self::RomWriteIfEquals {
                address,
                value,
                compare,
            } => Self::RomWriteIfEquals {
                address,
                value: value.resolve_user_parameter(user_value),
                compare: compare.resolve_user_parameter(user_value),
            },
            Self::RamWriteIfEquals {
                address,
                value,
                compare,
            } => Self::RamWriteIfEquals {
                address,
                value: value.resolve_user_parameter(user_value),
                compare: compare.resolve_user_parameter(user_value),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct CheatCode {
    pub name: String,
    pub code_text: String,
    pub enabled: bool,

    pub parameter_value: Option<u8>,
    pub code_type: CheatType,
    pub patches: Vec<CheatPatch>,
}

