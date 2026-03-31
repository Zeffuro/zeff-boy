use super::*;

#[test]
fn decode_6_letter_code() {
    let patch = decode_nes_game_genie("AAEAAG").unwrap();
    assert!(patch.compare.is_none());
    assert_eq!(patch.value, 0x00);
    assert_eq!(patch.address, 0x8408);
}

#[test]
fn decode_8_letter_code() {
    let patch = decode_nes_game_genie("AAEAAGAE").unwrap();
    assert!(patch.compare.is_some());
    assert_eq!(patch.value, 0x08);
    assert_eq!(patch.compare, Some(0x00));
}

#[test]
fn decode_all_bits_value() {
    let patch = decode_nes_game_genie("NYAAAE").unwrap();
    assert_eq!(patch.value, 0xFF);
}

#[test]
fn decode_address_bit_11() {
    let patch = decode_nes_game_genie("AEAAAA").unwrap();
    assert_eq!(patch.address & 0x0800, 0x0800);
}

#[test]
fn invalid_code_returns_none() {
    assert!(decode_nes_game_genie("QQQQQ").is_none());
    assert!(decode_nes_game_genie("ABC").is_none());
    assert!(decode_nes_game_genie("").is_none());
    assert!(decode_nes_game_genie("1234567").is_none());
}

#[test]
fn intercept_unconditional() {
    let mut state = NesCheatState::new();
    state.patches.push(NesGameGeniePatch {
        address: 0x8000,
        value: 0x42,
        compare: None,
    });
    assert_eq!(state.intercept(0x8000, 0x00), Some(0x42));
    assert_eq!(state.intercept(0x8001, 0x00), None);
}

#[test]
fn intercept_with_compare_match() {
    let mut state = NesCheatState::new();
    state.patches.push(NesGameGeniePatch {
        address: 0x8000,
        value: 0x42,
        compare: Some(0xAA),
    });
    assert_eq!(state.intercept(0x8000, 0xAA), Some(0x42));
}

#[test]
fn intercept_with_compare_mismatch() {
    let mut state = NesCheatState::new();
    state.patches.push(NesGameGeniePatch {
        address: 0x8000,
        value: 0x42,
        compare: Some(0xAA),
    });
    assert_eq!(state.intercept(0x8000, 0xBB), None);
}

#[test]
fn dashes_and_spaces_stripped() {
    let with_dash = decode_nes_game_genie("AAE-AAG").unwrap();
    let without = decode_nes_game_genie("AAEAAG").unwrap();
    assert_eq!(with_dash, without);
}

#[test]
fn case_insensitive() {
    let upper = decode_nes_game_genie("AAEAAG").unwrap();
    let lower = decode_nes_game_genie("aaeaag").unwrap();
    assert_eq!(upper, lower);
}
