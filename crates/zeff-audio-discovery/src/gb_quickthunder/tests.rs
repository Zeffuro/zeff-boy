use super::*;

static PROFILE: profiles::Profile = profiles::Profile {
    name: "gb-quickthunder-synthetic-v1",
    len: 0x100,
    hash: "",
    selector: 0x4000,
    tick: 0x4040,
    stride: 13,
    pattern_bytes: 2,
    operands: [0; 5],
    aliases: &[],
    empty_operand: None,
    default_empty: 0x4400,
    release_operand: 0,
};

static WIDE_PROFILE: profiles::Profile = profiles::Profile {
    name: "gb-quickthunder-synthetic-wide-v1",
    len: 0x100,
    hash: "",
    selector: 0x4000,
    tick: 0x4040,
    stride: 14,
    pattern_bytes: 4,
    operands: [0; 5],
    aliases: &[],
    empty_operand: None,
    default_empty: 0x4400,
    release_operand: 0,
};

fn fixture_code() -> Vec<u8> {
    let mut code = vec![0; 0x100];
    let init = [
        0x3e, 0x80, 0xe0, 0x26, 0x3e, 0x77, 0xe0, 0x24, 0x3e, 0x11, 0xe0, 0x25, 0x3e, 0x80, 0xe0,
        0x11, 0x3e, 0xf0, 0xe0, 0x12, 0x3e, 0x20, 0xe0, 0x13, 0x3e, 0x87, 0xe0, 0x14, 0xc9,
    ];
    code[..init.len()].copy_from_slice(&init);
    code[0x40..0x48].copy_from_slice(&[0x21, 0x00, 0xc1, 0x34, 0x7e, 0xe0, 0x13, 0xc9]);
    code
}

pub(super) fn recognize(bytes: &[u8], drivers: &mut Vec<profiles::Driver>) {
    for bank in 1..bytes.len() / 0x4000 {
        let base = bank * 0x4000;
        for profile in [&PROFILE, &WIDE_PROFILE] {
            let mut code = fixture_code();
            code[0xff] = if profile.stride == 14 { 14 } else { 0 };
            let wram = word(bytes, base + 0x41);
            if !(0xc000..=0xdf80).contains(&wram) {
                continue;
            }
            code[0x41..0x43].copy_from_slice(&wram.to_le_bytes());
            if bytes[base..base + code.len()] != code {
                continue;
            }
            drivers.push(profiles::Driver {
                profile,
                bank: bank as u16,
                wram,
                table: 0x4100,
                sequences: 0x4110,
                instruments: 0x4500,
                noise: 0x4600,
                frequency: 0x4700,
                empty: 0x4400,
                release: 0x4410,
            });
        }
    }
}

#[cfg(test)]
fn wide_fixture() -> Vec<u8> {
    let mut bytes = synthetic_rom();
    bytes[0x80ff] = 14;
    let original = bytes[0x8100..0x810d].to_vec();
    bytes[0x8100] = original[8];
    bytes[0x8101] = 15;
    bytes[0x8102..0x810a].copy_from_slice(&original[..8]);
    bytes[0x810a..0x810e].copy_from_slice(&original[9..13]);
    for channel in 0..4 {
        bytes[0x8200 + channel * 16..0x8204 + channel * 16].copy_from_slice(&[
            0,
            0xff,
            channel as u8,
            0,
        ]);
    }
    bytes
}

#[test]
fn wide_headers_and_sixteen_bit_sequence_indices_follow_original_layout() {
    let bytes = wide_fixture();
    let songs = inventory(&bytes);
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].table_entry.byte_len, 14);
    assert!(songs[0].tracks.iter().all(|track| track.note_count > 0));
    validate_song(&bytes, &songs[0], &AtomicBool::new(false)).unwrap();
    for (at, value) in [(0x8101, 7), (0x8203, 0x80), (0x810d, 0x80)] {
        let mut changed = bytes.clone();
        changed[at] = value;
        assert!(inventory(&changed).is_empty(), "wide mutation at {at:x}");
    }
    let mut changed = bytes.clone();
    changed[0x8201] = 0x42;
    assert_eq!(inventory(&changed), songs);
}

#[test]
fn cancellation_work_and_candidate_limits_interrupt_without_inventories() {
    let bytes = wide_fixture();
    for (cancelled, work, maximum) in [
        (true, 1_000_000, 100),
        (false, 20, 100),
        (false, 1_000_000, 0),
    ] {
        let cancel = AtomicBool::new(cancelled);
        let mut budget = Budget {
            cancel: &cancel,
            remaining: work,
        };
        let mut songs = Vec::new();
        assert!(scan(&bytes, &mut songs, &mut budget, maximum).is_err());
        assert!(songs.is_empty());
    }
}

pub fn synthetic_rom() -> Vec<u8> {
    let mut bytes = vec![0; 0x10000];
    bytes[0x100..0x103].copy_from_slice(&[0xc3, 0, 1]);
    bytes[0x143] = 0x80;
    bytes[0x147] = 0x19;
    bytes[0x148] = 1;
    bytes[0x8000..0x8100].copy_from_slice(&fixture_code());
    for channel in 0..4 {
        let pattern = 0x4200u16 + channel * 16;
        let sequence = 0x4300u16 + channel * 16;
        let instrument = 0x4510u16 + channel * 16;
        let at = usize::from(channel) * 2;
        bytes[0x8100 + at..0x8102 + at].copy_from_slice(&pattern.to_le_bytes());
        bytes[0x8110 + at..0x8112 + at].copy_from_slice(&sequence.to_le_bytes());
        bytes[0x8500 + at..0x8502 + at].copy_from_slice(&instrument.to_le_bytes());
        bytes[0x8600 + at..0x8602 + at].copy_from_slice(&instrument.to_le_bytes());
        let p = usize::from(pattern) + 0x4000;
        bytes[p..p + 2].copy_from_slice(&[0, channel as u8]);
        let p = usize::from(sequence) + 0x4000;
        bytes[p..p + 6].copy_from_slice(&[
            1,
            48,
            channel as u8,
            0xff,
            pattern as u8,
            (pattern >> 8) as u8,
        ]);
        let p = usize::from(instrument) + 0x4000;
        if channel == 2 {
            bytes[p..p + 10]
                .copy_from_slice(&[0x10, 0x44, 0x10, 0x44, 0x10, 0x44, 0x30, 0x44, 0x10, 0x44]);
        } else {
            bytes[p..p + 9].copy_from_slice(&[0x80, 0xf0, 0, 0x10, 0x44, 0x30, 0x44, 0x10, 0x44]);
            if channel == 3 {
                bytes[p + 5..p + 7].copy_from_slice(&[0x10, 0x44]);
            }
        }
    }
    bytes[0x8108..0x810d].copy_from_slice(&[1, 0x40, 0x44, 0, 0x48]);
    bytes[0x8411..0x8416].copy_from_slice(&[255, 0, 0, 0x11, 0x44]);
    bytes[0x8432..0x8438].copy_from_slice(&[255, 0, 0, 0, 0x32, 0x44]);
    bytes[0x8442..0x8448].copy_from_slice(&[255, 0x30, 0x55, 0, 0x42, 0x44]);
    bytes
}

#[cfg(test)]
fn inventory(bytes: &[u8]) -> Vec<GbQuickThunderSong> {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000_000,
    };
    let mut songs = Vec::new();
    scan(bytes, &mut songs, &mut budget, 100).unwrap();
    songs
}

#[test]
fn source_inventory_revalidates_and_relocates_between_banks() {
    let mut bytes = synthetic_rom();
    let songs = inventory(&bytes);
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].bank, 2);
    assert!(songs[0].tracks.iter().all(|track| track.note_count > 0));
    validate_song(&bytes, &songs[0], &AtomicBool::new(false)).unwrap();
    bytes[0x3fff] = 1;
    assert_eq!(inventory(&bytes), songs);
    bytes.copy_within(0x8000..0xc000, 0xc000);
    assert_eq!(inventory(&bytes).len(), 2);
    bytes[0x8001] ^= 1;
    assert_eq!(inventory(&bytes).len(), 1);
    assert!(validate_song(&bytes, &songs[0], &AtomicBool::new(false)).is_err());
}

#[test]
fn malformed_tables_instruments_effects_and_hardware_are_rejected() {
    for (at, value) in [
        (0x143, 0),
        (0x147, 1),
        (0x148, 6),
        (0x8108, 0),
        (0x8101, 0x80),
        (0x8111, 0x80),
        (0x8302, 255),
        (0x8415, 0x80),
        (0x8443, 0x80),
        (0x810c, 0x80),
        (0x8301, 0xfe),
    ] {
        let mut bytes = synthetic_rom();
        bytes[at] = value;
        assert!(inventory(&bytes).is_empty(), "mutation at {at:x}");
    }
    let mut bytes = synthetic_rom();
    bytes.truncate(0x9000);
    assert!(inventory(&bytes).is_empty());
}

#[test]
fn waveform_register_writes_and_all_effect_branches_are_mapped() {
    let bytes = synthetic_rom();
    let songs = inventory(&bytes);
    let covered = |at| {
        songs[0]
            .mapped_spans
            .iter()
            .any(|span| at >= span.effective_offset && at < span.effective_offset + span.byte_len)
    };
    for at in [
        0x8000, 0x8100, 0x8200, 0x8300, 0x8411, 0x8432, 0x8443, 0x8500, 0x8606, 0x87ff, 0x880f,
    ] {
        assert!(covered(at), "unmapped byte {at:x}");
    }
}

#[test]
fn preparation_preserves_original_driver_and_requires_current_metadata() {
    let bytes = synthetic_rom();
    let songs = inventory(&bytes);
    let cancel = AtomicBool::new(false);
    let prepared = prepare_rom(&bytes, &songs[0], &cancel).unwrap();
    assert_eq!(&prepared.bytes[0x8000..], &bytes[0x8000..]);
    assert_eq!(prepared.hardware, GbQuickThunderHardware::CgbDouble);
    assert_eq!(
        serde_json::to_value(prepared.hardware).unwrap(),
        "cgb_double"
    );
    assert!((0x150..0x200).contains(&prepared.wait_start));
    assert!(prepared.wait_start < prepared.wait_end);
    let mut stale = songs[0].clone();
    stale.tracks[0].note_count += 1;
    assert!(prepare_rom(&bytes, &stale, &cancel).is_err());
    assert!(prepare_rom(&bytes, &songs[0], &AtomicBool::new(true)).is_err());
}

#[test]
fn native_stack_stays_outside_the_complete_relocated_state() {
    for wram in [0xcf90u16, 0xdf3f, 0xdf40, 0xdf60, 0xdf6f, 0xdf80] {
        let mut bytes = synthetic_rom();
        bytes[0x8041..0x8043].copy_from_slice(&wram.to_le_bytes());
        let songs = inventory(&bytes);
        assert_eq!(songs.len(), 1);
        let prepared = prepare_rom(&bytes, &songs[0], &AtomicBool::new(false)).unwrap();
        let stack = word(&prepared.bytes, 0x152);
        assert!(
            stack <= wram || stack - 12 >= wram + 128,
            "stack at {stack:x}, state at {wram:x}"
        );
        assert_eq!(stack, if wram >= 0xdf40 { 0xd000 } else { 0xdff0 });
    }
}
