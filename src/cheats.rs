#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CheatType {
    GameShark,
    GameGenie,
    XPloder, // Also known as CodeBreaker overseas
    Raw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CheatValue {
    Constant(u8),
    PreserveWithCurrent {
        mask: u8,
        base: u8,
    },
    UserParameterized {
        mask: u8,
        base: u8,
    },
}

impl CheatValue {
    pub(crate) const fn constant(value: u8) -> Self {
        Self::Constant(value)
    }

    pub(crate) fn from_gameshark_value(token: &str) -> Option<Self> {
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

    pub(crate) fn from_mask_base_preserve(mask: u8, base: u8) -> Self {
        if mask == 0 {
            Self::Constant(base)
        } else {
            Self::PreserveWithCurrent { mask, base }
        }
    }

    pub(crate) fn from_mask_base_user(mask: u8, base: u8) -> Self {
        if mask == 0 {
            Self::Constant(base)
        } else {
            Self::UserParameterized { mask, base }
        }
    }

    pub(crate) fn has_user_parameter(self) -> bool {
        matches!(self, Self::UserParameterized { .. })
    }

    pub(crate) fn default_user_value(self) -> Option<u8> {
        match self {
            Self::UserParameterized { base, .. } => Some(base),
            _ => None,
        }
    }

    pub(crate) fn resolve_user_parameter(self, user_value: u8) -> Self {
        match self {
            Self::UserParameterized { mask, base } => {
                Self::Constant((user_value & mask) | base)
            }
            _ => self,
        }
    }

    pub(crate) fn resolve_with_current(self, current: u8) -> u8 {
        match self {
            Self::Constant(value) => value,
            Self::PreserveWithCurrent { mask, base }
            | Self::UserParameterized { mask, base } => (current & mask) | base,
        }
    }

    pub(crate) fn matches(self, observed: u8) -> bool {
        match self {
            Self::Constant(value) => observed == value,
            Self::PreserveWithCurrent { mask, base }
            | Self::UserParameterized { mask, base } => (observed & !mask) == base,
        }
    }

    pub(crate) fn display(self) -> String {
        match self {
            Self::Constant(value) => format!("{value:02X}"),
            Self::PreserveWithCurrent { mask, base }
            | Self::UserParameterized { mask, base } => {
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

impl CheatPatch {
    pub(crate) fn has_user_parameter(self) -> bool {
        match self {
            Self::RamWrite { value, .. } | Self::RomWrite { value, .. } => value.has_user_parameter(),
            Self::RomWriteIfEquals { value, compare, .. }
            | Self::RamWriteIfEquals { value, compare, .. } => {
                value.has_user_parameter() || compare.has_user_parameter()
            }
        }
    }

    pub(crate) fn default_user_value(self) -> Option<u8> {
        match self {
            Self::RamWrite { value, .. } | Self::RomWrite { value, .. } => value.default_user_value(),
            Self::RomWriteIfEquals { value, compare, .. }
            | Self::RamWriteIfEquals { value, compare, .. } => {
                value.default_user_value().or_else(|| compare.default_user_value())
            }
        }
    }

    pub(crate) fn resolve_user_parameter(self, user_value: u8) -> Self {
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

fn nybble_to_hex(v: u8) -> char {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    HEX[v as usize] as char
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CheatPatch {
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

#[derive(Debug, Clone)]
pub(crate) struct CheatCode {
    pub(crate) name: String,
    pub(crate) code_text: String,
    pub(crate) enabled: bool,
    // User-selected value for wildcard templates like 01??AAAA.
    pub(crate) parameter_value: Option<u8>,
    pub(crate) code_type: CheatType,
    pub(crate) patches: Vec<CheatPatch>,
}

fn gg_char_to_nybble(c: char) -> Option<u8> {
    match c.to_ascii_uppercase() {
        'D' => Some(0x0),
        'E' => Some(0x1),
        'F' => Some(0x2),
        'G' => Some(0x3),
        'H' => Some(0x4),
        'I' => Some(0x5),
        'J' => Some(0x6),
        'K' => Some(0x7),
        'L' => Some(0x8),
        'M' => Some(0x9),
        'N' => Some(0xA),
        'O' => Some(0xB),
        'P' => Some(0xC),
        'Q' => Some(0xD),
        'R' => Some(0xE),
        'S' => Some(0xF),
        _ => None,
    }
}

fn gg_hex_char_to_nybble(c: char) -> Option<u8> {
    c.to_digit(16).map(|v| v as u8)
}

fn parse_gg_nybbles(cleaned: &str) -> Option<Vec<u8>> {
    cleaned
        .chars()
        .map(gg_char_to_nybble)
        .collect::<Option<Vec<_>>>()
        .or_else(|| {
            cleaned
                .chars()
                .map(gg_hex_char_to_nybble)
                .collect::<Option<Vec<_>>>()
        })
}

fn try_parse_game_genie(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    let cleaned: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();

    if cleaned.len() != 6 && cleaned.len() != 9 {
        return None;
    }

    let n = parse_gg_nybbles(&cleaned)?;

    let value = CheatValue::constant((n[0] << 4) | n[1]);

    let address = ((u16::from(n[4]) | (u16::from(n[5] ^ 0xF) << 4)) << 8)
        | u16::from(n[2])
        | (u16::from(n[3]) << 4);

    let patch = if cleaned.len() == 9 {
        let op3 = (u16::from(n[6]) << 8) | (u16::from(n[7]) << 4) | u16::from(n[8]);
        let compare = CheatValue::constant(decode_gg_compare(op3));

        CheatPatch::RomWriteIfEquals {
            address,
            value,
            compare,
        }
    } else {
        CheatPatch::RomWrite { address, value }
    };

    Some((vec![patch], CheatType::GameGenie))
}

#[inline]
fn decode_gg_compare(op3: u16) -> u8 {
    let op = op3 as u32;
    let packed = ((op & 0x0F00) << 20) | (op & 0x000F);
    let rotated = packed.rotate_right(2);
    let folded = rotated | (rotated >> 24);
    (folded as u8) ^ 0xBA
}

fn try_parse_raw(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    let (addr_str, val_str) = cleaned.split_once(':')?;

    let addr = u16::from_str_radix(addr_str, 16).ok()?;
    let value = CheatValue::constant(u8::from_str_radix(val_str, 16).ok()?);

    Some((vec![CheatPatch::RamWrite { address: addr, value }], CheatType::Raw))
}

fn try_parse_gameshark(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    let parts: Vec<&str> = input.split('+').collect();
    let mut patches = Vec::new();

    for part in parts {
        let cleaned: String = part
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .collect();

        if cleaned.len() != 8 {
            return None;
        }

        let code_type_byte = u8::from_str_radix(&cleaned[0..2], 16).ok()?;
        let value = CheatValue::from_gameshark_value(&cleaned[2..4])?;
        let addr_hi = u8::from_str_radix(&cleaned[4..6], 16).ok()?;
        let addr_lo = u8::from_str_radix(&cleaned[6..8], 16).ok()?;
        let address = (u16::from(addr_hi) << 8) | u16::from(addr_lo);

        let patch = match code_type_byte {
            0x01 | 0x80 | 0x90 | 0x91 => CheatPatch::RamWrite { address, value },
            _ => {
                log::warn!(
                    "Unsupported GameShark opcode {:02X}, treating as RAM write",
                    code_type_byte
                );
                CheatPatch::RamWrite { address, value }
            }
        };

        patches.push(patch);
    }

    Some((patches, CheatType::GameShark))
}

fn try_parse_xploder(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    let parts: Vec<&str> = input.split('+').collect();
    let mut patches = Vec::new();

    for part in parts {
        let cleaned: String = part.chars().filter(|c| !c.is_whitespace()).collect();
        let hex = cleaned.strip_prefix('$')?;

        if hex.len() != 8 {
            return None;
        }

        let code_type = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let value = CheatValue::constant(u8::from_str_radix(&hex[2..4], 16).ok()?);
        let addr_hi = u8::from_str_radix(&hex[4..6], 16).ok()?;
        let addr_lo = u8::from_str_radix(&hex[6..8], 16).ok()?;
        let address = (u16::from(addr_hi) << 8) | u16::from(addr_lo);

        let patch = match code_type {
            0x0D => CheatPatch::RamWrite { address, value },

            _ => {
                log::warn!(
                    "Unknown XPloder opcode {:02X}, treating as RAM write",
                    code_type
                );
                CheatPatch::RamWrite { address, value }
            }
        };

        patches.push(patch);
    }

    Some((patches, CheatType::XPloder))
}

pub(crate) fn parse_cheat(input: &str) -> Result<(Vec<CheatPatch>, CheatType), &'static str> {
    if let Some(result) = try_parse_game_genie(input) {
        return Ok(result);
    }

    if let Some(result) = try_parse_raw(input) {
        return Ok(result);
    }

    if let Some(result) = try_parse_xploder(input) {
        return Ok(result);
    }

    if let Some(result) = try_parse_gameshark(input) {
        return Ok(result);
    }

    Err("Unrecognized format. Use GameShark (01VVAAAA, supports ??/?0/0? values), Game Genie (XXX-YYY), XPloder ($XXXXXXXX), or raw (AAAA:VV)")
}

pub(crate) fn collect_enabled_patches(cheats: &[CheatCode]) -> Vec<CheatPatch> {
    cheats
        .iter()
        .filter(|c| c.enabled)
        .flat_map(|c| {
            c.patches.iter().copied().map(|patch| {
                if patch.has_user_parameter() {
                    let value = c
                        .parameter_value
                        .or_else(|| patch.default_user_value())
                        .unwrap_or(0);
                    patch.resolve_user_parameter(value)
                } else {
                    patch
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gameshark() {
        let (patches, ty) = parse_cheat("01FF C0DE").unwrap();
        assert_eq!(ty, CheatType::GameShark);
        assert_eq!(patches.len(), 1);
        match patches[0] {
            CheatPatch::RamWrite { address, value } => {
                assert_eq!(address, 0xC0DE);
                assert_eq!(value, CheatValue::Constant(0xFF));
            }
            _ => panic!("Expected RamWrite"),
        }
    }

    #[test]
    fn parse_gameshark_parameterized_full_byte() {
        let (patches, ty) = parse_cheat("01??A5C6").unwrap();
        assert_eq!(ty, CheatType::GameShark);
        assert_eq!(patches.len(), 1);
        match patches[0] {
            CheatPatch::RamWrite { address, value } => {
                assert_eq!(address, 0xA5C6);
                assert_eq!(
                    value,
                    CheatValue::UserParameterized {
                        mask: 0xFF,
                        base: 0x00,
                    }
                );
            }
            _ => panic!("Expected RamWrite"),
        }
    }

    #[test]
    fn parse_gameshark_parameterized_nibble() {
        let (patches, _) = parse_cheat("01?0A5C6").unwrap();
        match patches[0] {
            CheatPatch::RamWrite { value, .. } => {
                assert_eq!(
                    value,
                    CheatValue::UserParameterized {
                        mask: 0xF0,
                        base: 0x00,
                    }
                );
            }
            _ => panic!("Expected RamWrite"),
        }
    }

    #[test]
    fn parse_gameshark_multi_part() {
        let (patches, ty) = parse_cheat("01FFC0DE+01AAC0DF").unwrap();
        assert_eq!(ty, CheatType::GameShark);
        assert_eq!(patches.len(), 2);
    }

    #[test]
    fn parse_gameshark_91_opcode() {
        let (patches, ty) = parse_cheat("91??C8C6").unwrap();
        assert_eq!(ty, CheatType::GameShark);
        assert_eq!(patches.len(), 1);
        assert!(matches!(patches[0], CheatPatch::RamWrite { .. }));
    }

    #[test]
    fn parse_raw() {
        let (patches, ty) = parse_cheat("C000:42").unwrap();
        assert_eq!(ty, CheatType::Raw);
        assert_eq!(patches.len(), 1);
        match patches[0] {
            CheatPatch::RamWrite { address, value } => {
                assert_eq!(address, 0xC000);
                assert_eq!(value, CheatValue::Constant(0x42));
            }
            _ => panic!("Expected RamWrite"),
        }
    }

    #[test]
    fn parse_game_genie_6_digit() {
        let (patches, ty) = parse_cheat("DEF-GHI").unwrap();
        assert_eq!(ty, CheatType::GameGenie);
        assert_eq!(patches.len(), 1);
        assert!(matches!(patches[0], CheatPatch::RomWrite { .. }));
    }

    #[test]
    fn parse_game_genie_9_digit() {
        let (patches, ty) = parse_cheat("DEF-GHI-JKL").unwrap();
        assert_eq!(ty, CheatType::GameGenie);
        assert_eq!(patches.len(), 1);
        assert!(matches!(patches[0], CheatPatch::RomWriteIfEquals { .. }));
    }

    #[test]
    fn parse_game_genie_9_digit_hex_variant() {
        let (patches, ty) = parse_cheat("006-CEB-3BE").unwrap();
        assert_eq!(ty, CheatType::GameGenie);
        assert_eq!(patches.len(), 1);
        assert!(matches!(patches[0], CheatPatch::RomWriteIfEquals { .. }));
    }

    #[test]
    fn parse_xploder() {
        let (patches, ty) = parse_cheat("$0D2ACA55").unwrap();
        assert_eq!(ty, CheatType::XPloder);
        assert_eq!(patches.len(), 1);
        match patches[0] {
            CheatPatch::RamWrite { address, value } => {
                assert_eq!(address, 0xCA55);
                assert_eq!(value, CheatValue::Constant(0x2A));
            }
            _ => panic!("Expected RamWrite"),
        }
    }

    #[test]
    fn cheat_value_resolution_and_matching() {
        let masked = CheatValue::from_mask_base_preserve(0xF0, 0x0A);
        assert_eq!(masked.resolve_with_current(0xB7), 0xBA);
        assert!(masked.matches(0x2A));
        assert!(!masked.matches(0x2B));
    }

    #[test]
    fn collect_enabled_patches_resolves_user_parameter_value() {
        let cheat = CheatCode {
            name: "Param cheat".to_string(),
            code_text: "01??A5C6".to_string(),
            enabled: true,
            parameter_value: Some(0x3C),
            code_type: CheatType::GameShark,
            patches: vec![CheatPatch::RamWrite {
                address: 0xA5C6,
                value: CheatValue::from_mask_base_user(0xFF, 0x00),
            }],
        };

        let patches = collect_enabled_patches(&[cheat]);
        assert_eq!(patches.len(), 1);
        match patches[0] {
            CheatPatch::RamWrite { address, value } => {
                assert_eq!(address, 0xA5C6);
                assert_eq!(value, CheatValue::Constant(0x3C));
            }
            _ => panic!("Expected RamWrite"),
        }
    }

    #[test]
    fn cheat_value_resolution_and_matching_user_parameterized() {
        let masked = CheatValue::UserParameterized {
            mask: 0xF0,
            base: 0x0A,
        };
        assert_eq!(masked.resolve_with_current(0xB7), 0xBA);
        assert!(masked.matches(0x2A));
        assert!(!masked.matches(0x2B));
    }

    #[test]
    fn parse_invalid() {
        assert!(parse_cheat("not a code").is_err());
    }
}