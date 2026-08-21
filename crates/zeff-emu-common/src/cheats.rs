use crate::address::Address;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheatType {
    GameShark,
    GameGenie,
    ActionReplay,
    XPloder, // Also known as CodeBreaker overseas
    Raw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    pub fn from_mask_base_preserve(mask: u8, base: u8) -> Self {
        if mask == 0 {
            Self::Constant(base)
        } else {
            Self::PreserveWithCurrent { mask, base }
        }
    }

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
    char::from_digit(v as u32, 16)
        .unwrap_or('0')
        .to_ascii_uppercase()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheatPatch {
    RamWrite {
        address: u16,
        value: CheatValue,
    },
    WideRamWrite {
        address: Address,
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
    WideRamWriteIfEquals {
        address: Address,
        value: CheatValue,
        compare: CheatValue,
    },
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CheatApplyStats {
    pub applied: usize,
    pub skipped_compare: usize,
    pub ignored: usize,
}

pub trait CheatByteTarget<A: Copy> {
    fn cheat_peek8(&self, address: A) -> u8;
    fn cheat_write8(&mut self, address: A, value: u8);
}

impl CheatPatch {
    pub fn is_rom_patch(self) -> bool {
        matches!(self, Self::RomWrite { .. } | Self::RomWriteIfEquals { .. })
    }

    pub fn constant_rom_write(self) -> Option<(u16, u8, Option<u8>)> {
        match self {
            Self::RomWrite { address, value } => {
                let CheatValue::Constant(value) = value else {
                    return None;
                };
                Some((address, value, None))
            }
            Self::RomWriteIfEquals {
                address,
                value,
                compare,
            } => {
                let CheatValue::Constant(value) = value else {
                    return None;
                };
                let CheatValue::Constant(compare) = compare else {
                    return None;
                };
                Some((address, value, Some(compare)))
            }
            _ => None,
        }
    }

    pub fn has_user_parameter(self) -> bool {
        match self {
            Self::RamWrite { value, .. }
            | Self::WideRamWrite { value, .. }
            | Self::RomWrite { value, .. } => value.has_user_parameter(),
            Self::RomWriteIfEquals { value, compare, .. }
            | Self::RamWriteIfEquals { value, compare, .. }
            | Self::WideRamWriteIfEquals { value, compare, .. } => {
                value.has_user_parameter() || compare.has_user_parameter()
            }
        }
    }

    pub fn default_user_value(self) -> Option<u8> {
        match self {
            Self::RamWrite { value, .. }
            | Self::WideRamWrite { value, .. }
            | Self::RomWrite { value, .. } => value.default_user_value(),
            Self::RomWriteIfEquals { value, compare, .. }
            | Self::RamWriteIfEquals { value, compare, .. }
            | Self::WideRamWriteIfEquals { value, compare, .. } => value
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
            Self::WideRamWrite { address, value } => Self::WideRamWrite {
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
            Self::WideRamWriteIfEquals {
                address,
                value,
                compare,
            } => Self::WideRamWriteIfEquals {
                address,
                value: value.resolve_user_parameter(user_value),
                compare: compare.resolve_user_parameter(user_value),
            },
        }
    }
}

pub fn apply_ram_cheats_16<T: CheatByteTarget<u16>>(
    target: &mut T,
    patches: &[CheatPatch],
) -> CheatApplyStats {
    let mut stats = CheatApplyStats::default();

    for patch in patches {
        match *patch {
            CheatPatch::RamWrite { address, value } => {
                apply_byte_write(target, address, value);
                stats.applied += 1;
            }
            CheatPatch::RamWriteIfEquals {
                address,
                value,
                compare,
            } => {
                if apply_byte_write_if_equals(target, address, value, compare) {
                    stats.applied += 1;
                } else {
                    stats.skipped_compare += 1;
                }
            }
            _ => {
                stats.ignored += 1;
            }
        }
    }

    stats
}

pub fn apply_wide_ram_cheats<T: CheatByteTarget<Address>>(
    target: &mut T,
    patches: &[CheatPatch],
) -> CheatApplyStats {
    let mut stats = CheatApplyStats::default();

    for patch in patches {
        match *patch {
            CheatPatch::WideRamWrite { address, value } => {
                apply_byte_write(target, address, value);
                stats.applied += 1;
            }
            CheatPatch::WideRamWriteIfEquals {
                address,
                value,
                compare,
            } => {
                if apply_byte_write_if_equals(target, address, value, compare) {
                    stats.applied += 1;
                } else {
                    stats.skipped_compare += 1;
                }
            }
            _ => {
                stats.ignored += 1;
            }
        }
    }

    stats
}

fn apply_byte_write<A: Copy, T: CheatByteTarget<A>>(target: &mut T, address: A, value: CheatValue) {
    let current = target.cheat_peek8(address);
    target.cheat_write8(address, value.resolve_with_current(current));
}

fn apply_byte_write_if_equals<A: Copy, T: CheatByteTarget<A>>(
    target: &mut T,
    address: A,
    value: CheatValue,
    compare: CheatValue,
) -> bool {
    let current = target.cheat_peek8(address);
    if !compare.matches(current) {
        return false;
    }

    target.cheat_write8(address, value.resolve_with_current(current));
    true
}

pub fn parse_raw16_cheats(input: &str) -> Option<Vec<CheatPatch>> {
    parse_raw_cheat_parts(input, parse_raw16_cheat)
}

pub fn parse_wide_raw_cheats(input: &str) -> Option<Vec<CheatPatch>> {
    parse_raw_cheat_parts(input, parse_wide_raw_cheat)
}

mod gba_codebreaker;

pub use gba_codebreaker::{
    GbaCodeBreakerState, parse_gba_codebreaker_cheats, parse_gba_codebreaker_cheats_with_state,
};

pub fn parse_raw16_cheat(input: &str) -> Option<CheatPatch> {
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    let (address_text, value_text) = cleaned
        .split_once(':')
        .or_else(|| cleaned.split_once('='))?;
    let address = parse_hex_u16_exact(address_text)?;
    let value = parse_hex_u8_exact(value_text)?;
    Some(CheatPatch::RamWrite {
        address,
        value: CheatValue::constant(value),
    })
}

pub fn parse_wide_raw_cheat(input: &str) -> Option<CheatPatch> {
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    let (address_text, value_text) = cleaned
        .split_once(':')
        .or_else(|| cleaned.split_once('='))?;
    let address = parse_hex_u32_exact(address_text)?;
    let value = parse_hex_u8_exact(value_text)?;
    Some(CheatPatch::WideRamWrite {
        address,
        value: CheatValue::constant(value),
    })
}

fn parse_raw_cheat_parts(
    input: &str,
    parser: fn(&str) -> Option<CheatPatch>,
) -> Option<Vec<CheatPatch>> {
    let mut patches = Vec::new();
    for part in input.split('+') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        patches.push(parser(part)?);
    }
    (!patches.is_empty()).then_some(patches)
}

fn parse_hex_u16_exact(input: &str) -> Option<u16> {
    let token = normalize_hex_token(input)?;
    if token.len() != 4 {
        return None;
    }
    u16::from_str_radix(token, 16).ok()
}

fn parse_hex_u32_exact(input: &str) -> Option<u32> {
    let token = normalize_hex_token(input)?;
    if token.len() != 8 {
        return None;
    }
    u32::from_str_radix(token, 16).ok()
}

fn parse_hex_u8_exact(input: &str) -> Option<u8> {
    let token = normalize_hex_token(input)?;
    if token.len() != 2 {
        return None;
    }
    u8::from_str_radix(token, 16).ok()
}

fn normalize_hex_token(input: &str) -> Option<&str> {
    let trimmed = input.trim();
    let token = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .or_else(|| trimmed.strip_prefix('$'))
        .unwrap_or(trimmed);
    if token.is_empty() || !token.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(token)
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

#[cfg(test)]
mod tests {
    use super::{
        CheatByteTarget, CheatPatch, CheatValue, apply_ram_cheats_16, apply_wide_ram_cheats,
        parse_raw16_cheats, parse_wide_raw_cheats,
    };

    struct NarrowMemory {
        bytes: [u8; 0x1_0000],
    }

    impl Default for NarrowMemory {
        fn default() -> Self {
            Self {
                bytes: [0; 0x1_0000],
            }
        }
    }

    impl CheatByteTarget<u16> for NarrowMemory {
        fn cheat_peek8(&self, address: u16) -> u8 {
            self.bytes[usize::from(address)]
        }

        fn cheat_write8(&mut self, address: u16, value: u8) {
            self.bytes[usize::from(address)] = value;
        }
    }

    #[derive(Default)]
    struct WideMemory {
        bytes: std::collections::HashMap<u32, u8>,
    }

    impl CheatByteTarget<u32> for WideMemory {
        fn cheat_peek8(&self, address: u32) -> u8 {
            self.bytes.get(&address).copied().unwrap_or(0)
        }

        fn cheat_write8(&mut self, address: u32, value: u8) {
            self.bytes.insert(address, value);
        }
    }

    #[test]
    fn apply_ram_cheats_16_handles_write_compare_and_ignored_patches() {
        let mut memory = NarrowMemory::default();
        memory.cheat_write8(0xC000, 0xA5);
        memory.cheat_write8(0xC001, 0x11);

        let stats = apply_ram_cheats_16(
            &mut memory,
            &[
                CheatPatch::RamWrite {
                    address: 0xC000,
                    value: CheatValue::Constant(0x42),
                },
                CheatPatch::RamWriteIfEquals {
                    address: 0xC001,
                    value: CheatValue::Constant(0x55),
                    compare: CheatValue::Constant(0x99),
                },
                CheatPatch::RomWrite {
                    address: 0x1234,
                    value: CheatValue::Constant(0x66),
                },
            ],
        );

        assert_eq!(memory.cheat_peek8(0xC000), 0x42);
        assert_eq!(memory.cheat_peek8(0xC001), 0x11);
        assert_eq!(stats.applied, 1);
        assert_eq!(stats.skipped_compare, 1);
        assert_eq!(stats.ignored, 1);
    }

    #[test]
    fn apply_ram_cheats_16_resolves_masked_values_with_current_memory() {
        let mut memory = NarrowMemory::default();
        memory.cheat_write8(0xC000, 0xA5);

        let stats = apply_ram_cheats_16(
            &mut memory,
            &[CheatPatch::RamWrite {
                address: 0xC000,
                value: CheatValue::PreserveWithCurrent {
                    mask: 0xF0,
                    base: 0x0B,
                },
            }],
        );

        assert_eq!(memory.cheat_peek8(0xC000), 0xAB);
        assert_eq!(stats.applied, 1);
    }

    #[test]
    fn apply_wide_ram_cheats_handles_wide_addresses() {
        let mut memory = WideMemory::default();
        memory.cheat_write8(0x0200_0000, 0x01);

        let stats = apply_wide_ram_cheats(
            &mut memory,
            &[CheatPatch::WideRamWriteIfEquals {
                address: 0x0200_0000,
                value: CheatValue::Constant(0x42),
                compare: CheatValue::Constant(0x01),
            }],
        );

        assert_eq!(memory.cheat_peek8(0x0200_0000), 0x42);
        assert_eq!(stats.applied, 1);
    }

    #[test]
    fn constant_rom_write_extracts_only_constant_rom_patches() {
        assert_eq!(
            CheatPatch::RomWriteIfEquals {
                address: 0x1234,
                value: CheatValue::Constant(0x42),
                compare: CheatValue::Constant(0x99),
            }
            .constant_rom_write(),
            Some((0x1234, 0x42, Some(0x99)))
        );

        assert_eq!(
            CheatPatch::RomWrite {
                address: 0x1234,
                value: CheatValue::UserParameterized {
                    mask: 0x0F,
                    base: 0x40,
                },
            }
            .constant_rom_write(),
            None
        );
    }

    #[test]
    fn parse_raw16_cheats_accepts_prefixes_separator_variants_and_multi_code() {
        let patches = parse_raw16_cheats("$C000:01 + 0xD000 = 02")
            .expect("raw 16-bit multi-code should parse");

        assert_eq!(
            patches,
            vec![
                CheatPatch::RamWrite {
                    address: 0xC000,
                    value: CheatValue::Constant(0x01),
                },
                CheatPatch::RamWrite {
                    address: 0xD000,
                    value: CheatValue::Constant(0x02),
                },
            ]
        );
    }

    #[test]
    fn parse_wide_raw_cheats_accepts_full_width_addresses_only() {
        assert_eq!(
            parse_wide_raw_cheats("02000000:42"),
            Some(vec![CheatPatch::WideRamWrite {
                address: 0x0200_0000,
                value: CheatValue::Constant(0x42),
            }])
        );
        assert_eq!(parse_wide_raw_cheats("2000000:42"), None);
        assert_eq!(parse_wide_raw_cheats("02000000:4"), None);
    }
}
