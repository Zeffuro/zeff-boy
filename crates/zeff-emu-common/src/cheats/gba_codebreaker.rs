use super::{CheatPatch, CheatValue};

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
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[address, value]| parse_gba_codebreaker_split_pair(address, value))
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

#[cfg(test)]
mod tests {
    use super::*;

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
