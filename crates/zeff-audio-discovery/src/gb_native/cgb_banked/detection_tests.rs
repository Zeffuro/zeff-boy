use std::sync::atomic::AtomicBool;

use crate::{
    Budget, ScanStop,
    drivers::{DriverTableRowState, Qualification},
};

use super::{
    Cue,
    detection::scan_variants,
    recipe::{DriverVariant, KnownRomQualification, VariantParameters},
};

const CUES: &[Cue] = &[Cue {
    raw: 0x30,
    frames: 1,
    clocks: 1,
    loop_start: None,
    banks: &[1],
}];

fn fixture() -> (Vec<u8>, &'static DriverVariant) {
    let mut bytes = vec![0; 0x20_0000];
    bytes[0x143] = 0xc0;
    bytes[0x147..0x14a].copy_from_slice(&[0x19, 6, 0]);
    bytes[0x200..0x42a].fill(0x5a);
    bytes[0x200 + 0x12] = 0x21;
    bytes[0x200 + 0x13..0x200 + 0x15].copy_from_slice(&0x500_u16.to_le_bytes());
    bytes[0x200 + 0x15..0x200 + 0x18].fill(0x09);
    bytes[0x300 + 14..0x300 + 16].copy_from_slice(&0x600_u16.to_le_bytes());
    bytes[0x300 + 20..0x300 + 22].copy_from_slice(&0x600_u16.to_le_bytes());
    bytes[0x400 + 32..0x400 + 34].copy_from_slice(&0x200_u16.to_le_bytes());
    bytes[0x400 + 40..0x400 + 42].copy_from_slice(&0x21b_u16.to_le_bytes());
    bytes[0x500..0x50f].copy_from_slice(&[
        1, 0, 0x40, // mapped header
        1, 0, 0x80, // unmapped header
        1, 0, 0x42, // empty mask
        1, 0, 0x43, // invalid mask
        1, 0, 0x44, // unmapped channel
    ]);
    let bank = 0x4000;
    bytes[bank] = 0x0f;
    for channel in 0..4 {
        let offset = bank + 1 + channel * 2;
        bytes[offset..offset + 2].copy_from_slice(&(0x4100 + channel as u16).to_le_bytes());
    }
    bytes[bank + 0x200] = 0;
    bytes[bank + 0x300] = 0xf0;
    bytes[bank + 0x400] = 1;
    bytes[bank + 0x401..bank + 0x403].copy_from_slice(&0x8000_u16.to_le_bytes());
    let driver_sha256 = Box::leak(zeff_firmware::sha256_hex(&bytes[0x200..0x42a]).into_boxed_str());
    let variant = Box::leak(Box::new(DriverVariant {
        id: "cgb-banked-test",
        parameters: VariantParameters {
            cartridge_type: 0x19,
            init: 0x240,
            init_len: 0x43,
            selector: 0x300,
            tick: 0x400,
            driver: 0x200,
            driver_end: 0x42a,
            table: 0x500,
            table_rows: 5,
            hook: 0x180,
        },
        qualification: KnownRomQualification {
            sources: &["known-test-source"],
            cues: CUES,
        },
        driver_sha256: Some(driver_sha256),
    }));
    (bytes, variant)
}

fn scan(
    bytes: &[u8],
    variant: &'static DriverVariant,
    source: Option<&str>,
    work: u64,
    limit: usize,
) -> (Vec<crate::drivers::DriverFinding>, Result<(), ScanStop>) {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: work,
    };
    let mut findings = Vec::new();
    let result = scan_variants(bytes, source, &mut findings, &mut budget, limit, [variant]);
    (findings, result)
}

#[test]
fn fingerprinted_candidate_retains_malformed_rows_without_native_promotion() {
    let (bytes, variant) = fixture();
    let (findings, result) = scan(&bytes, variant, None, 20, 1);
    result.unwrap();
    let finding = findings.first().unwrap();
    assert_eq!(finding.qualification, Qualification::Candidate);
    assert_eq!(finding.rows.len(), 5);
    assert_eq!(
        finding.rows[0].state,
        DriverTableRowState::MappedChannelHeader
    );
    assert_eq!(finding.rows[0].channels.len(), 4);
    assert_eq!(finding.rows[1].state, DriverTableRowState::UnmappedHeader);
    assert!(finding.rows[1].header.is_none());
    assert_eq!(finding.rows[2].state, DriverTableRowState::EmptyChannelMask);
    assert_eq!(
        finding.rows[3].state,
        DriverTableRowState::InvalidChannelMask
    );
    assert_eq!(finding.rows[4].state, DriverTableRowState::UnmappedChannel);
}

#[test]
fn source_identity_only_strengthens_the_existing_selector_subset() {
    let (bytes, variant) = fixture();
    let finding = scan(&bytes, variant, Some("known-test-source"), 20, 1)
        .0
        .pop()
        .unwrap();
    assert_eq!(
        finding.qualification,
        Qualification::KnownRom {
            profile: "cgb-banked-test",
            native_playback_selectors: vec![0x30],
        }
    );
    let mut changed = bytes;
    changed[0x200] ^= 1;
    assert!(
        scan(&changed, variant, Some("known-test-source"), 20, 1)
            .0
            .is_empty()
    );
}

#[test]
fn candidate_limit_and_work_limit_do_not_append_partial_findings() {
    let (bytes, variant) = fixture();
    let (findings, result) = scan(&bytes, variant, None, 20, 0);
    assert_eq!(result, Err(ScanStop::CandidateLimit));
    assert!(findings.is_empty());
    let (findings, result) = scan(&bytes, variant, None, 1, 1);
    assert_eq!(result, Err(ScanStop::WorkLimit));
    assert!(findings.is_empty());
}

#[test]
fn cancellation_does_not_append_a_finding() {
    let (bytes, variant) = fixture();
    let cancel = AtomicBool::new(true);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 20,
    };
    let mut findings = Vec::new();
    assert_eq!(
        scan_variants(&bytes, None, &mut findings, &mut budget, 1, [variant],),
        Err(ScanStop::Cancelled)
    );
    assert!(findings.is_empty());
}
