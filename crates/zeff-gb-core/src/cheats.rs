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

#[derive(Debug, Clone)]
pub struct CheatCode {
    pub name: String,
    pub code_text: String,
    pub enabled: bool,
    // User-selected value for wildcard templates like 01??AAAA.
    pub parameter_value: Option<u8>,
    pub code_type: CheatType,
    pub patches: Vec<CheatPatch>,
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

    Some((
        vec![CheatPatch::RamWrite {
            address: addr,
            value,
        }],
        CheatType::Raw,
    ))
}

fn try_parse_gameshark(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    let parts: Vec<&str> = input.split('+').collect();
    let mut patches = Vec::new();

    for part in parts {
        let (p, _) = try_parse_gameshark_single(part)?;
        patches.extend(p);
    }

    Some((patches, CheatType::GameShark))
}

fn try_parse_gameshark_single(input: &str) -> Option<(Vec<CheatPatch>, CheatType)> {
    let cleaned: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();

    if cleaned.len() != 8 {
        return None;
    }

    let code_type_byte = u8::from_str_radix(&cleaned[0..2], 16).ok()?;
    let value = CheatValue::from_gameshark_value(&cleaned[2..4])?;
    let addr_low = u8::from_str_radix(&cleaned[4..6], 16).ok()?;
    let addr_high = u8::from_str_radix(&cleaned[6..8], 16).ok()?;
    let address = (u16::from(addr_high) << 8) | u16::from(addr_low);

    let patch = match code_type_byte {
        0x01 | 0x80 | 0x90 | 0x91 | 0x96 | 0x95 => CheatPatch::RamWrite { address, value },
        _ => {
            log::warn!(
                "Unsupported GameShark opcode {:02X}, treating as RAM write",
                code_type_byte
            );
            CheatPatch::RamWrite { address, value }
        }
    };

    Some((vec![patch], CheatType::GameShark))
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

pub fn parse_cht_file(content: &str) -> Vec<CheatCode> {
    let mut cheats = Vec::new();
    let mut entries: std::collections::HashMap<usize, (Option<String>, Option<String>, bool)> =
        std::collections::HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(rest) = line.strip_prefix("cheat")
            && let Some(idx_end) = rest.find('_')
                && let Ok(idx) = rest[..idx_end].parse::<usize>() {
                    let field = &rest[idx_end + 1..];
                    if let Some(value) = field.strip_prefix("desc = ") {
                        let value = value.trim().trim_matches('"').to_string();
                        entries.entry(idx).or_insert((None, None, false)).0 = Some(value);
                    } else if let Some(value) = field.strip_prefix("code = ") {
                        let value = value.trim().trim_matches('"').to_string();
                        entries.entry(idx).or_insert((None, None, false)).1 = Some(value);
                    } else if let Some(value) = field.strip_prefix("enable = ") {
                        let enabled = value.trim() == "true";
                        entries.entry(idx).or_insert((None, None, false)).2 = enabled;
                    }
                }
    }

    let mut indices: Vec<usize> = entries.keys().copied().collect();
    indices.sort_unstable();

    for idx in indices {
        if let Some((desc, code, enabled)) = entries.remove(&idx) {
            let code_text = code.unwrap_or_default();
            if code_text.is_empty() {
                continue;
            }
            let name = desc.unwrap_or_else(|| code_text.clone());

            match parse_cheat(&code_text) {
                Ok((patches, code_type)) => {
                    let parameter_value =
                        patches.iter().copied().find_map(|p| p.default_user_value());
                    cheats.push(CheatCode {
                        name,
                        code_text,
                        enabled,
                        parameter_value,
                        code_type,
                        patches,
                    });
                }
                Err(e) => {
                    log::warn!(
                        "Failed to parse cheat '{}': {} (code: {})",
                        name,
                        e,
                        code_text
                    );
                }
            }
        }
    }

    cheats
}

pub fn export_cht_file(cheats: &[CheatCode]) -> String {
    let mut out = String::new();
    out.push_str(&format!("cheats = {}\n\n", cheats.len()));

    for (i, cheat) in cheats.iter().enumerate() {
        out.push_str(&format!("cheat{}_desc = \"{}\"\n", i, cheat.name));
        out.push_str(&format!("cheat{}_code = \"{}\"\n", i, cheat.code_text));
        out.push_str(&format!("cheat{}_enable = {}\n\n", i, cheat.enabled));
    }

    out
}

pub fn parse_cheat(input: &str) -> Result<(Vec<CheatPatch>, CheatType), &'static str> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Empty cheat code");
    }

    let parts: Vec<&str> = trimmed.split('+').collect();

    if parts.len() == 1 {
        if let Some(result) = try_parse_game_genie(trimmed) {
            return Ok(result);
        }
        if let Some(result) = try_parse_raw(trimmed) {
            return Ok(result);
        }
        if let Some(result) = try_parse_xploder(trimmed) {
            return Ok(result);
        }
        if let Some(result) = try_parse_gameshark(trimmed) {
            return Ok(result);
        }
    } else {
        let mut all_patches = Vec::new();
        let mut detected_type: Option<CheatType> = None;

        for part in &parts {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let result = try_parse_game_genie(part)
                .or_else(|| try_parse_raw(part))
                .or_else(|| try_parse_xploder(part))
                .or_else(|| try_parse_gameshark_single(part));

            match result {
                Some((patches, ty)) => {
                    if let Some(prev) = detected_type
                        && prev != ty {}
                    detected_type = Some(ty);
                    all_patches.extend(patches);
                }
                None => {
                    return Err(
                        "Unrecognized format in multi-code. Use GameShark (01VVAAAA), Game Genie (XXX-YYY), XPloder ($XXXXXXXX), or raw (AAAA:VV)",
                    );
                }
            }
        }

        if let Some(ty) = detected_type
            && !all_patches.is_empty() {
                return Ok((all_patches, ty));
            }
    }

    Err(
        "Unrecognized format. Use GameShark (01VVAAAA, supports ??/?0/0? values), Game Genie (XXX-YYY), XPloder ($XXXXXXXX), or raw (AAAA:VV)",
    )
}

pub fn collect_enabled_patches(
    user: &[CheatCode],
    libretro: &[CheatCode],
) -> Vec<CheatPatch> {
    user.iter()
        .chain(libretro.iter())
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
        let (patches, ty) = parse_cheat("01FF DEC0").unwrap();
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
    fn parse_gameshark_no_spaces() {
        let (patches, ty) = parse_cheat("010CA2C6").unwrap();
        assert_eq!(ty, CheatType::GameShark);
        assert_eq!(patches.len(), 1);
        match patches[0] {
            CheatPatch::RamWrite { address, value } => {
                assert_eq!(address, 0xC6A2);
                assert_eq!(value, CheatValue::Constant(0x0C));
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
                assert_eq!(address, 0xC6A5);
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
    fn parse_gameshark_from_libretro_zelda() {
        let (patches, ty) = parse_cheat("010CA2C6").unwrap();
        assert_eq!(ty, CheatType::GameShark);
        assert_eq!(patches.len(), 1);
        match patches[0] {
            CheatPatch::RamWrite { address, value } => {
                assert_eq!(address, 0xC6A2);
                assert_eq!(value, CheatValue::Constant(0x0C));
            }
            _ => panic!("Expected RamWrite"),
        }
    }

    #[test]
    fn parse_gameshark_91_from_libretro() {
        let (patches, ty) = parse_cheat("9199BAC6").unwrap();
        assert_eq!(ty, CheatType::GameShark);
        assert_eq!(patches.len(), 1);
        match patches[0] {
            CheatPatch::RamWrite { address, value } => {
                assert_eq!(address, 0xC6BA);
                assert_eq!(value, CheatValue::Constant(0x99));
            }
            _ => panic!("Expected RamWrite"),
        }
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
    fn parse_game_genie_multi_code_9_digit() {
        let (patches, ty) = parse_cheat("181-5DA-6EA+061-5EA-2AE+001-82A-E62").unwrap();
        assert_eq!(ty, CheatType::GameGenie);
        assert_eq!(patches.len(), 3);
        for patch in &patches {
            assert!(matches!(patch, CheatPatch::RomWriteIfEquals { .. }));
        }
    }

    #[test]
    fn parse_game_genie_multi_code_6_digit() {
        let (patches, ty) = parse_cheat("01B-13B+C3B-14B+5FB-15B").unwrap();
        assert_eq!(ty, CheatType::GameGenie);
        assert_eq!(patches.len(), 3);
        for patch in &patches {
            assert!(matches!(patch, CheatPatch::RomWrite { .. }));
        }
    }

    #[test]
    fn parse_game_genie_multi_code_mixed_lengths() {
        let (patches, ty) = parse_cheat("DEF-GHI+DEF-GHI-JKL").unwrap();
        assert_eq!(ty, CheatType::GameGenie);
        assert_eq!(patches.len(), 2);
        assert!(matches!(patches[0], CheatPatch::RomWrite { .. }));
        assert!(matches!(patches[1], CheatPatch::RomWriteIfEquals { .. }));
    }

    #[test]
    fn parse_game_genie_long_multi_code() {
        let input = "00A-32A-4C5+304-8EB-3BA+007-808-A29+FE4-CCB-190+C32-9DB-801+007-499-19E+00E-3F9-A29+C96-3FB-6E3";
        let (patches, ty) = parse_cheat(input).unwrap();
        assert_eq!(ty, CheatType::GameGenie);
        assert_eq!(patches.len(), 8);
    }

    #[test]
    fn parse_game_genie_6_digit_from_libretro() {
        let (patches, ty) = parse_cheat("1E3-18B").unwrap();
        assert_eq!(ty, CheatType::GameGenie);
        assert_eq!(patches.len(), 1);
        assert!(matches!(patches[0], CheatPatch::RomWrite { .. }));
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
    fn parse_xploder_multi_code() {
        let input = "$0D20502A+$0D20932A+$0D20A12A+$0D202C2A+$0D20BD2A+$0D20492A+$0D20AF2A";
        let (patches, ty) = parse_cheat(input).unwrap();
        assert_eq!(ty, CheatType::XPloder);
        assert_eq!(patches.len(), 7);
        for patch in &patches {
            assert!(matches!(patch, CheatPatch::RamWrite { .. }));
        }
    }

    #[test]
    fn parse_xploder_from_libretro() {
        let (patches, ty) = parse_cheat("$0D61C82A").unwrap();
        assert_eq!(ty, CheatType::XPloder);
        assert_eq!(patches.len(), 1);
        match patches[0] {
            CheatPatch::RamWrite { address, value } => {
                assert_eq!(address, 0xC82A);
                assert_eq!(value, CheatValue::Constant(0x61));
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
    fn cheat_value_constant_display() {
        assert_eq!(CheatValue::Constant(0xFF).display(), "FF");
        assert_eq!(CheatValue::Constant(0x00).display(), "00");
        assert_eq!(CheatValue::Constant(0xAB).display(), "AB");
    }

    #[test]
    fn cheat_value_parameterized_display() {
        let full = CheatValue::UserParameterized {
            mask: 0xFF,
            base: 0x00,
        };
        assert_eq!(full.display(), "??");

        let hi = CheatValue::UserParameterized {
            mask: 0xF0,
            base: 0x0A,
        };
        assert_eq!(hi.display(), "?A");

        let lo = CheatValue::UserParameterized {
            mask: 0x0F,
            base: 0xA0,
        };
        assert_eq!(lo.display(), "A?");
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

        let patches = collect_enabled_patches(&[cheat], &[]);
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
    fn collect_enabled_patches_skips_disabled() {
        let cheats = vec![
            CheatCode {
                name: "Disabled".to_string(),
                code_text: "01FFC0DE".to_string(),
                enabled: false,
                parameter_value: None,
                code_type: CheatType::GameShark,
                patches: vec![CheatPatch::RamWrite {
                    address: 0xC0DE,
                    value: CheatValue::Constant(0xFF),
                }],
            },
            CheatCode {
                name: "Enabled".to_string(),
                code_text: "01AAC0DF".to_string(),
                enabled: true,
                parameter_value: None,
                code_type: CheatType::GameShark,
                patches: vec![CheatPatch::RamWrite {
                    address: 0xC0DF,
                    value: CheatValue::Constant(0xAA),
                }],
            },
        ];
        let patches = collect_enabled_patches(&cheats, &[]);
        assert_eq!(patches.len(), 1);
        match patches[0] {
            CheatPatch::RamWrite { address, .. } => assert_eq!(address, 0xC0DF),
            _ => panic!("Expected RamWrite"),
        }
    }

    #[test]
    fn collect_enabled_patches_merges_user_and_libretro() {
        let user = vec![CheatCode {
            name: "User".to_string(),
            code_text: "01FFC0DE".to_string(),
            enabled: true,
            parameter_value: None,
            code_type: CheatType::GameShark,
            patches: vec![CheatPatch::RamWrite {
                address: 0xC0DE,
                value: CheatValue::Constant(0xFF),
            }],
        }];
        let libretro = vec![CheatCode {
            name: "Libretro".to_string(),
            code_text: "01AAC0DF".to_string(),
            enabled: true,
            parameter_value: None,
            code_type: CheatType::GameShark,
            patches: vec![CheatPatch::RamWrite {
                address: 0xC0DF,
                value: CheatValue::Constant(0xAA),
            }],
        }];
        let patches = collect_enabled_patches(&user, &libretro);
        assert_eq!(patches.len(), 2);
    }

    #[test]
    fn parse_cht_file_basic() {
        let content = r#"cheats = 2

            cheat0_desc = "Infinite Health"
            cheat0_code = "010CA2C6"
            cheat0_enable = false

            cheat1_desc = "Walk Through Walls"
            cheat1_code = "010033D0"
            cheat1_enable = true
            "#;
        let cheats = parse_cht_file(content);
        assert_eq!(cheats.len(), 2);
        assert_eq!(cheats[0].name, "Infinite Health");
        assert_eq!(cheats[0].code_text, "010CA2C6");
        assert!(!cheats[0].enabled);
        assert_eq!(cheats[1].name, "Walk Through Walls");
        assert!(cheats[1].enabled);
    }

    #[test]
    fn parse_cht_file_game_genie_multi() {
        let content = r#"cheats = 1

            cheat0_desc = "Moon Jump"
            cheat0_code = "181-5DA-6EA+061-5EA-2AE+001-82A-E62"
            cheat0_enable = false
            "#;
        let cheats = parse_cht_file(content);
        assert_eq!(cheats.len(), 1);
        assert_eq!(cheats[0].patches.len(), 3);
        assert_eq!(cheats[0].code_type, CheatType::GameGenie);
    }

    #[test]
    fn parse_cht_file_xploder() {
        let content = r#"cheats = 1

            cheat0_desc = "Max Health"
            cheat0_code = "$0D61C82A"
            cheat0_enable = false
            "#;
        let cheats = parse_cht_file(content);
        assert_eq!(cheats.len(), 1);
        assert_eq!(cheats[0].code_type, CheatType::XPloder);
    }

    #[test]
    fn parse_cht_file_skips_empty_code() {
        let content = r#"cheats = 2

            cheat0_desc = "Has code"
            cheat0_code = "01FFC0DE"
            cheat0_enable = false

            cheat1_desc = "No code"
            cheat1_code = ""
            cheat1_enable = false
            "#;
        let cheats = parse_cht_file(content);
        assert_eq!(cheats.len(), 1);
    }

    #[test]
    fn export_cht_file_roundtrip() {
        let original = vec![CheatCode {
            name: "Test Cheat".to_string(),
            code_text: "01FFC0DE".to_string(),
            enabled: true,
            parameter_value: None,
            code_type: CheatType::GameShark,
            patches: vec![CheatPatch::RamWrite {
                address: 0xC0DE,
                value: CheatValue::Constant(0xFF),
            }],
        }];
        let exported = export_cht_file(&original);
        let reimported = parse_cht_file(&exported);
        assert_eq!(reimported.len(), 1);
        assert_eq!(reimported[0].name, "Test Cheat");
        assert_eq!(reimported[0].code_text, "01FFC0DE");
        assert!(reimported[0].enabled);
    }

    #[test]
    fn parse_invalid() {
        assert!(parse_cheat("not a code").is_err());
    }

    #[test]
    fn parse_empty() {
        assert!(parse_cheat("").is_err());
        assert!(parse_cheat("   ").is_err());
    }
}
