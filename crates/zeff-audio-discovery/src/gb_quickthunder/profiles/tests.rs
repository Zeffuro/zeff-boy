use std::sync::atomic::AtomicBool;

use crate::Budget;

use super::*;

static ALIASES: &[&[usize]] = &[&[0x30, 0x32], &[0x34, 0x36], &[0x38, 0x3a]];

fn word(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn tagged_code(bank: u8, wram: u16, rom_pointer: u16) -> Vec<u8> {
    let mut code = vec![0; 0x80];
    code[..6].copy_from_slice(&[bank, 0x54, 0x47, 0xc3, 0x78, 0x48]);
    code[6..12].copy_from_slice(&[0x21, wram as u8, (wram >> 8) as u8, 0x35, 0x28, 3]);
    code[12..15].copy_from_slice(&[0x21, rom_pointer as u8, (rom_pointer >> 8) as u8]);
    code[15..18].copy_from_slice(&[0xea, (wram + 16) as u8, ((wram + 16) >> 8) as u8]);
    for (at, value) in [
        (0x30, 0x4100),
        (0x32, 0x4100),
        (0x34, 0x4200),
        (0x36, 0x4200),
        (0x38, 0x4300),
        (0x3a, 0x4300),
        (0x3c, 0x4400),
        (0x40, 0x4500),
        (0x42, 0x4600),
    ] {
        word(&mut code, at, value);
    }
    code
}

fn tagged_normal_form(mut code: Vec<u8>) -> Vec<u8> {
    code[0] = 0;
    code[7..9].fill(0);
    code[13..15].fill(0);
    code[16..18].copy_from_slice(&[16, 0]);
    code
}

fn tagged_profile(code: &[u8]) -> &'static Profile {
    Box::leak(Box::new(Profile {
        name: "gb-quickthunder-synthetic-bank-tag",
        header: HeaderKind::BankTagged {
            prefix: [0xc3, 0x78, 0x48],
            banks: &[5, 0x17],
        },
        len: code.len(),
        hash: Box::leak(
            zeff_firmware::sha256_hex(&tagged_normal_form(code.to_vec())).into_boxed_str(),
        ),
        selector: 0x4754,
        tick: 0x4006,
        stride: 14,
        pattern_bytes: 2,
        operands: [0x30, 0x34, 0x38, 0x3c, 0x40],
        aliases: ALIASES,
        empty_operand: None,
        default_empty: 0x4400,
        release_operand: 0x42,
    }))
}

fn executable_profile(code: &[u8]) -> &'static Profile {
    let mut normalized = code.to_vec();
    normalized[1..3].fill(0);
    normalized[7..9].fill(0);
    normalized[10..12].copy_from_slice(&[16, 0]);
    Box::leak(Box::new(Profile {
        name: "gb-quickthunder-synthetic-executable",
        header: HeaderKind::Executable,
        len: code.len(),
        hash: Box::leak(zeff_firmware::sha256_hex(&normalized).into_boxed_str()),
        selector: 0x4000,
        tick: 0x4000,
        stride: 14,
        pattern_bytes: 2,
        operands: [0x2a, 0x2e, 0x32, 0x36, 0x3a],
        aliases: &[&[0x2a, 0x2c], &[0x2e, 0x30], &[0x32, 0x34]],
        empty_operand: None,
        default_empty: 0x4400,
        release_operand: 0x3c,
    }))
}

fn in_bank(bank: u16, code: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; (usize::from(bank) + 1) * 0x4000];
    let base = usize::from(bank) * 0x4000;
    bytes[base..base + code.len()].copy_from_slice(code);
    bytes
}

#[test]
fn bank_tagged_headers_match_only_the_tagged_bank_and_selector() {
    let code = tagged_code(5, 0xc200, 0x4100);
    let profile = tagged_profile(&code);
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000,
    };
    let bytes = in_bank(5, &code);
    assert_eq!(
        recognize(&bytes, 5, profile, &mut budget)
            .unwrap()
            .unwrap()
            .profile
            .name,
        profile.name
    );
    let other_tag = tagged_code(0x17, 0xc200, 0x4100);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000,
    };
    assert!(
        recognize(&in_bank(0x17, &other_tag), 0x17, profile, &mut budget)
            .unwrap()
            .is_some()
    );
    for (at, value) in [(0, 6), (1, 0x55), (3, 0xc2)] {
        let mut changed = code.clone();
        changed[at] = value;
        let bytes = in_bank(5, &changed);
        let mut budget = Budget {
            cancel: &cancel,
            remaining: 1_000,
        };
        assert!(
            recognize(&bytes, 5, profile, &mut budget)
                .unwrap()
                .is_none()
        );
    }
    assert!(!profile.header_matches(&code, 0));
    assert!(!profile.header_matches(&code, 256));
    assert!(!profile.header_matches(&tagged_code(6, 0xc200, 0x4100), 6));
}

#[test]
fn bank_tagged_normalization_accepts_rom_and_wram_relocation() {
    let code = tagged_code(5, 0xc200, 0x4100);
    let profile = tagged_profile(&code);
    let relocated = tagged_code(5, 0xd000, 0x4200);
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000,
    };
    let driver = recognize(&in_bank(5, &relocated), 5, profile, &mut budget)
        .unwrap()
        .unwrap();
    assert_eq!(driver.wram, 0xd000);
    assert_eq!(driver.table, 0x4100);
}

#[test]
fn executable_headers_still_reject_mutated_code() {
    let tagged = tagged_code(5, 0xc200, 0x4100);
    let code = tagged[6..].to_vec();
    let profile = executable_profile(&code);
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000,
    };
    assert!(
        recognize(&in_bank(1, &code), 1, profile, &mut budget)
            .unwrap()
            .is_some()
    );
    let mut changed = code;
    changed[0x70] ^= 1;
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000,
    };
    assert!(
        recognize(&in_bank(1, &changed), 1, profile, &mut budget)
            .unwrap()
            .is_none()
    );
}

#[test]
fn executable_headers_admit_jump_prologues() {
    let profile = PROFILES
        .iter()
        .find(|profile| profile.name == "gb-quickthunder-14-01")
        .unwrap();
    assert!(profile.header_matches(&[0xc3, 0x57, 0x47], 2));
    assert!(!profile.header_matches(&[0x00, 0x57, 0x47], 2));
}

#[test]
fn bank_tagged_recognition_honors_work_and_cancellation() {
    let code = tagged_code(5, 0xc200, 0x4100);
    let profile = tagged_profile(&code);
    let bytes = in_bank(5, &code);
    let active = AtomicBool::new(false);
    let mut exhausted = Budget {
        cancel: &active,
        remaining: 0,
    };
    assert!(recognize(&bytes, 5, profile, &mut exhausted).is_err());
    let cancelled = AtomicBool::new(true);
    let mut budget = Budget {
        cancel: &cancelled,
        remaining: 1_000,
    };
    assert!(recognize(&bytes, 5, profile, &mut budget).is_err());
}

#[test]
fn bank_tagged_data_remains_a_candidate_profile() {
    let tagged = PROFILES
        .iter()
        .find(|profile| profile.name == "gb-quickthunder-14-bank-tag")
        .unwrap();
    let ordinary = PROFILES
        .iter()
        .find(|profile| profile.name == "gb-quickthunder-14-01")
        .unwrap();
    assert_eq!(
        tagged.header,
        HeaderKind::BankTagged {
            prefix: [0xc3, 0x78, 0x48],
            banks: &[0x05, 0x17],
        }
    );
    assert_eq!(
        (tagged.len, tagged.selector, tagged.tick),
        (2168, 0x4754, 0x4006)
    );
    assert_eq!(tagged.stride, ordinary.stride);
    assert_eq!(tagged.pattern_bytes, ordinary.pattern_bytes);
    assert_eq!(tagged.operands, ordinary.operands);
    assert_eq!(tagged.aliases, ordinary.aliases);
    assert_eq!(tagged.release_operand, ordinary.release_operand);
    assert_eq!(tagged.default_empty, ordinary.default_empty);
}
