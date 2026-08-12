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

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct GbaCodeBreakerState {
    encryption: Option<GbaCodeBreakerEncryption>,
}

impl GbaCodeBreakerState {
    pub fn reset(&mut self) {
        self.encryption = None;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GbaCodeBreakerEncryption {
    decrypt_key: u32,
    bit_table: [u8; 48],
    seeds: [u32; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GbaCodeBreakerPair {
    address: u32,
    value: u16,
}

pub fn parse_gba_codebreaker_cheats(input: &str) -> Option<Vec<CheatPatch>> {
    let mut state = GbaCodeBreakerState::default();
    parse_gba_codebreaker_cheats_with_state(input, &mut state).filter(|patches| !patches.is_empty())
}

pub fn parse_gba_codebreaker_cheats_with_state(
    input: &str,
    state: &mut GbaCodeBreakerState,
) -> Option<Vec<CheatPatch>> {
    let pairs = parse_gba_codebreaker_pairs(input)?;
    let mut patches = Vec::new();
    let mut i = 0;

    while i < pairs.len() {
        let pair = match decode_gba_codebreaker_pair(pairs[i], state, true)? {
            Some(pair) => pair,
            None => {
                i += 1;
                continue;
            }
        };

        let op = pair.address >> 28;
        let address = pair.address & 0x0FFF_FFFF;
        match op {
            0x0 | 0x1 => {
                i += 1;
            }
            0x3 => {
                push_gba_byte_write(&mut patches, address, pair.value as u8);
                i += 1;
            }
            0x4 => {
                let next = pairs.get(i + 1).copied()?;
                let param = decode_gba_codebreaker_pair(next, state, false).flatten()?;
                let count = param.address & 0x0000_FFFF;
                let value_step = param.address >> 16;
                let address_step = u32::from(param.value);
                if count == 0 || count > 4096 {
                    return None;
                }
                for n in 0..count {
                    let write_address = address.checked_add(n.checked_mul(address_step)?)?;
                    let value = pair.value.wrapping_add((n.wrapping_mul(value_step)) as u16);
                    push_gba_halfword_write(&mut patches, write_address, value);
                }
                i += 2;
            }
            0x8 => {
                push_gba_halfword_write(&mut patches, address, pair.value);
                i += 1;
            }
            _ => return None,
        }
    }

    Some(patches)
}

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

fn push_gba_byte_write(patches: &mut Vec<CheatPatch>, address: u32, value: u8) {
    patches.push(CheatPatch::WideRamWrite {
        address,
        value: CheatValue::constant(value),
    });
}

fn push_gba_halfword_write(patches: &mut Vec<CheatPatch>, address: u32, value: u16) {
    let [lo, hi] = value.to_le_bytes();
    push_gba_byte_write(patches, address, lo);
    push_gba_byte_write(patches, address.wrapping_add(1), hi);
}

fn parse_gba_codebreaker_pairs(input: &str) -> Option<Vec<GbaCodeBreakerPair>> {
    let tokens: Vec<&str> = input
        .split(|c: char| !c.is_ascii_hexdigit())
        .filter(|token| !token.is_empty())
        .collect();

    if tokens.is_empty() {
        return None;
    }

    if tokens.iter().all(|token| token.len() == 12) {
        return tokens
            .iter()
            .map(|token| parse_gba_codebreaker_compact_pair(token))
            .collect();
    }

    if !tokens.len().is_multiple_of(2) {
        return None;
    }

    tokens
        .chunks_exact(2)
        .map(|chunk| {
            let [address, value] = chunk else {
                unreachable!("chunks_exact(2) always yields two tokens");
            };
            parse_gba_codebreaker_split_pair(address, value)
        })
        .collect()
}

fn parse_gba_codebreaker_compact_pair(input: &str) -> Option<GbaCodeBreakerPair> {
    let (address, value) = input.split_at(8);
    parse_gba_codebreaker_split_pair(address, value)
}

fn parse_gba_codebreaker_split_pair(address: &str, value: &str) -> Option<GbaCodeBreakerPair> {
    if address.len() != 8 || value.len() != 4 {
        return None;
    }
    Some(GbaCodeBreakerPair {
        address: u32::from_str_radix(address, 16).ok()?,
        value: u16::from_str_radix(value, 16).ok()?,
    })
}

fn decode_gba_codebreaker_pair(
    pair: GbaCodeBreakerPair,
    state: &mut GbaCodeBreakerState,
    allow_activation: bool,
) -> Option<Option<GbaCodeBreakerPair>> {
    if let Some(encryption) = state.encryption.as_ref() {
        return Some(Some(decrypt_gba_codebreaker_pair(pair, encryption)));
    }

    if allow_activation && (pair.address >> 28) == 0x9 {
        state.encryption = Some(calculate_gba_codebreaker_seeds(pair.address, pair.value)?);
        return Some(None);
    }

    Some(Some(pair))
}

fn decrypt_gba_codebreaker_pair(
    pair: GbaCodeBreakerPair,
    encryption: &GbaCodeBreakerEncryption,
) -> GbaCodeBreakerPair {
    let mut buf = [0; 6];
    buf[..4].copy_from_slice(&pair.address.to_be_bytes());
    buf[4..].copy_from_slice(&pair.value.to_be_bytes());

    for i in (0..48).rev() {
        let j = usize::from(encryption.bit_table[i]);
        let off1 = i >> 3;
        let off2 = j >> 3;
        let bit1 = i & 7;
        let bit2 = j & 7;

        let p1 = (buf[off1] >> bit1) & 1;
        let p2 = (buf[off2] >> bit2) & 1;
        buf[off1] = (buf[off1] & !(1 << bit1)) | (p2 << bit1);
        buf[off2] = (buf[off2] & !(1 << bit2)) | (p1 << bit2);
    }

    xor_gba_codebreaker_seed(&mut buf, encryption.seeds[0], encryption.seeds[1] as u16);

    let high_key = (encryption.decrypt_key >> 8) as u8;
    for i in 0..5 {
        buf[i] ^= high_key ^ buf[i + 1];
    }
    buf[5] ^= high_key;

    let low_key = encryption.decrypt_key as u8;
    for i in (1..=5).rev() {
        buf[i] ^= low_key ^ buf[i - 1];
    }
    buf[0] ^= low_key;

    xor_gba_codebreaker_seed(&mut buf, encryption.seeds[2], encryption.seeds[3] as u16);

    GbaCodeBreakerPair {
        address: u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]),
        value: u16::from_be_bytes([buf[4], buf[5]]),
    }
}

fn xor_gba_codebreaker_seed(buf: &mut [u8; 6], seed_address: u32, seed_value: u16) {
    let address = seed_address.to_be_bytes();
    let value = seed_value.to_be_bytes();
    for i in 0..4 {
        buf[i] ^= address[i];
    }
    buf[4] ^= value[0];
    buf[5] ^= value[1];
}

fn calculate_gba_codebreaker_seeds(address: u32, value: u16) -> Option<GbaCodeBreakerEncryption> {
    let mut table = [0; 48];
    for (i, slot) in table.iter_mut().enumerate() {
        *slot = i as u8;
    }

    let mut rng_state = u32::from(value & 0x00FF) ^ 0x1111;
    for _ in 0..80 {
        let (p1, next_state) = gba_codebreaker_next_table_index(rng_state)?;
        rng_state = next_state;
        let (p2, next_state) = gba_codebreaker_next_table_index(rng_state)?;
        rng_state = next_state;
        table.swap(p1, p2);
    }

    rng_state = 0x4EFA_D1C3;
    for _ in 0..((address >> 24) & 0x0F) {
        let (roll, _) = gba_codebreaker_lfsr_advance(rng_state);
        rng_state = roll;
    }
    let (seed2, next_state) = gba_codebreaker_lfsr_advance(rng_state);
    rng_state = next_state;
    let (seed3, _) = gba_codebreaker_lfsr_advance(rng_state);

    rng_state = u32::from(value >> 8) ^ 0xF254;
    for _ in 0..u32::from(value >> 8) {
        let (roll, _) = gba_codebreaker_lfsr_advance(rng_state);
        rng_state = roll;
    }
    let (seed0, next_state) = gba_codebreaker_lfsr_advance(rng_state);
    rng_state = next_state;
    let (seed1, _) = gba_codebreaker_lfsr_advance(rng_state);

    Some(GbaCodeBreakerEncryption {
        decrypt_key: address,
        bit_table: table,
        seeds: [seed0, seed1, seed2, seed3],
    })
}

fn gba_codebreaker_lfsr_advance(state0: u32) -> (u32, u32) {
    let state1 = state0.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
    let state2 = state1.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
    let state3 = state2.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
    let roll =
        ((state1 << 14) & 0xC000_0000) | ((state2 >> 1) & 0x3FFF_8000) | ((state3 >> 16) & 0x7FFF);
    (roll, state3)
}

fn gba_codebreaker_next_table_index(mut lfsr_state: u32) -> Option<(usize, u32)> {
    let (mut roll, next_state) = gba_codebreaker_lfsr_advance(lfsr_state);
    lfsr_state = next_state;
    let mut count = 48u32;

    if roll == count {
        roll = 0;
    }

    if roll >= count {
        let mut bit = 1u32;

        while count < 0x1000_0000 && count < roll {
            count = count.wrapping_shl(4);
            bit = bit.wrapping_shl(4);
        }

        while count < 0x8000_0000 && count < roll {
            count = count.wrapping_shl(1);
            bit = bit.wrapping_shl(1);
        }

        let mut mask;
        loop {
            mask = 0u32;
            if roll >= count {
                roll -= count;
            }
            if roll >= (count >> 1) {
                roll -= count >> 1;
                mask |= bit.rotate_right(1);
            }
            if roll >= (count >> 2) {
                roll -= count >> 2;
                mask |= bit.rotate_right(2);
            }
            if roll >= (count >> 3) {
                roll -= count >> 3;
                mask |= bit.rotate_right(3);
            }
            if roll == 0 || (bit >> 4) == 0 {
                break;
            }
            bit >>= 4;
            count >>= 4;
        }

        mask &= 0xE000_0000;
        if mask != 0 && (bit & 7) != 0 {
            if (mask & bit.rotate_right(3)) != 0 {
                roll += count >> 3;
            }
            if (mask & bit.rotate_right(2)) != 0 {
                roll += count >> 2;
            }
            if (mask & bit.rotate_right(1)) != 0 {
                roll += count >> 1;
            }
        }
    }

    let idx = usize::try_from(roll).ok()?;
    (idx < 48).then_some((idx, lfsr_state))
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
        CheatByteTarget, CheatPatch, CheatValue, GbaCodeBreakerPair, GbaCodeBreakerState,
        apply_ram_cheats_16, apply_wide_ram_cheats, calculate_gba_codebreaker_seeds,
        decrypt_gba_codebreaker_pair, parse_gba_codebreaker_cheats,
        parse_gba_codebreaker_cheats_with_state, parse_raw16_cheats, parse_wide_raw_cheats,
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

    #[test]
    fn parse_gba_codebreaker_cheats_accepts_byte_and_halfword_writes() {
        assert_eq!(
            parse_gba_codebreaker_cheats("3200E924+0096+8201A454+07B7"),
            Some(vec![
                CheatPatch::WideRamWrite {
                    address: 0x0200_E924,
                    value: CheatValue::Constant(0x96),
                },
                CheatPatch::WideRamWrite {
                    address: 0x0201_A454,
                    value: CheatValue::Constant(0xB7),
                },
                CheatPatch::WideRamWrite {
                    address: 0x0201_A455,
                    value: CheatValue::Constant(0x07),
                },
            ])
        );
    }

    #[test]
    fn parse_gba_codebreaker_cheats_accepts_serial_halfword_writes() {
        let patches = parse_gba_codebreaker_cheats("42035CBE+1388+00000005+0002")
            .expect("serial write should parse");

        assert_eq!(patches.len(), 10);
        for i in 0..5 {
            let address = 0x0203_5CBE + i * 2;
            assert_eq!(
                patches[usize::try_from(i * 2).unwrap()],
                CheatPatch::WideRamWrite {
                    address,
                    value: CheatValue::Constant(0x88),
                }
            );
            assert_eq!(
                patches[usize::try_from(i * 2 + 1).unwrap()],
                CheatPatch::WideRamWrite {
                    address: address + 1,
                    value: CheatValue::Constant(0x13),
                }
            );
        }
    }

    #[test]
    fn gba_codebreaker_decryption_matches_public_conversion_vector() {
        let encryption = calculate_gba_codebreaker_seeds(0x9F66_37CD, 0x47C3)
            .expect("activator should generate encryption data");
        let decoded = decrypt_gba_codebreaker_pair(
            GbaCodeBreakerPair {
                address: 0x1022_6CCB,
                value: 0x2BAA,
            },
            &encryption,
        );

        assert_eq!(
            decoded,
            GbaCodeBreakerPair {
                address: 0x0000_3067,
                value: 0x000A,
            }
        );
    }

    #[test]
    fn parse_gba_codebreaker_cheats_uses_state_for_encrypted_following_codes() {
        let mut state = GbaCodeBreakerState::default();
        let activator =
            parse_gba_codebreaker_cheats_with_state("9F6637CD47C3", &mut state).unwrap();
        assert!(activator.is_empty());

        assert_eq!(
            parse_gba_codebreaker_cheats_with_state("5B1005082B1B", &mut state),
            Some(vec![
                CheatPatch::WideRamWrite {
                    address: 0x0200_23BE,
                    value: CheatValue::Constant(0x00),
                },
                CheatPatch::WideRamWrite {
                    address: 0x0200_23BF,
                    value: CheatValue::Constant(0x00),
                },
            ])
        );
    }

    #[test]
    fn parse_gba_codebreaker_cheats_rejects_unsupported_conditionals() {
        assert_eq!(
            parse_gba_codebreaker_cheats("72035810+0C04+32039A28+000C"),
            None
        );
    }
}
