use super::*;

pub(super) static PROFILE: profiles::Profile = profiles::Profile {
    name: "gb-sound-system-synthetic-v1",
    hash: "782629c1ddf6d3d55bd11b9ac7e6e05394cc1f20b6e24fbf45c3be7a633b1d34",
    end: 0x90,
    wram: 0xdf80,
    hardware: GbSoundSystemHardware::CgbDouble,
    table_low: 0x84,
    table_high: 0x85,
    dispatch_hash: "d81bfb50e59a9abbe66f6ae0c6b45c7b9c0bc6eead2cf982118ac4d62b6ffeda",
    operands: &[(0x81, 0x82, 0), (0x84, 0x85, -1)],
};

pub(super) static NORMAL_PROFILE: profiles::Profile = profiles::Profile {
    name: "gb-sound-system-synthetic-normal-v1",
    hash: "e417ca8575d01f6491c77f64198f27180999889442f046d4b8adea4f6d74dc7f",
    end: 0x90,
    wram: 0xdf80,
    hardware: GbSoundSystemHardware::CgbNormal,
    table_low: 0x84,
    table_high: 0x85,
    dispatch_hash: "d81bfb50e59a9abbe66f6ae0c6b45c7b9c0bc6eead2cf982118ac4d62b6ffeda",
    operands: &[(0x81, 0x82, 0), (0x84, 0x85, -1)],
};

pub fn synthetic_rom_with_hardware(hardware: GbSoundSystemHardware) -> Vec<u8> {
    let mut bytes = synthetic_rom();
    if hardware == GbSoundSystemHardware::CgbNormal {
        bytes[0x8075] = 0x28;
    }
    bytes
}

pub fn synthetic_rom() -> Vec<u8> {
    let mut bytes = vec![0; 0x10000];
    bytes[0x143] = 0xc0;
    bytes[0x147] = 0x19;
    bytes[0x148] = 1;
    bytes[0x8000..0x8009].copy_from_slice(&[0xc3, 0x10, 0x40, 0xc3, 0x40, 0x40, 0xc3, 0x60, 0x40]);
    bytes[0x8010..0x8017].copy_from_slice(&[0xaf, 0xe0, 0x26, 0xea, 0x80, 0xdf, 0xc9]);
    bytes[0x8040..0x8048].copy_from_slice(&[0x21, 0x80, 0xdf, 0x34, 0x7e, 0xe0, 0x13, 0xc9]);
    let play = [
        0x3e, 0x80, 0xe0, 0x26, 0x3e, 0x77, 0xe0, 0x24, 0x3e, 0x11, 0xe0, 0x25, 0x3e, 0x80, 0xe0,
        0x11, 0x3e, 0xf0, 0xe0, 0x12, 0x3e, 0x20, 0xe0, 0x13, 0x3e, 0x87, 0xe0, 0x14, 0xc9,
    ];
    bytes[0x8060..0x8060 + play.len()].copy_from_slice(&play);
    bytes[0x8080..0x8086].copy_from_slice(&[0x21, 0xb8, 0x4e, 0x21, 0, 0x42]);
    bytes[0x8200..0x8204].copy_from_slice(&[0, 0x44, 0, 0x43]);
    bytes[0x8300..0x8302].copy_from_slice(&[0x20, 0x43]);
    bytes[0x8320..0x8326].copy_from_slice(&[1, 24, 0, 0, 6, 9]);
    bytes[0x8400..0x8402].copy_from_slice(&[0x20, 0x44]);
    bytes[0x8420..0x8429].copy_from_slice(&[7, 5, 0xf0, 4, 0, 0x87, 0, 6, 2]);
    bytes
}

#[cfg(test)]
fn inventory(bytes: &[u8]) -> Vec<GbSoundSystemSong> {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 2_000_000,
    };
    let mut songs = Vec::new();
    scan(bytes, &mut songs, &mut budget, 100).unwrap();
    songs
}

#[test]
fn native_inventory_and_handoff_revalidate() {
    let bytes = synthetic_rom();
    let songs = inventory(&bytes);
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].tracks[0].note_count, 1);
    let prepared = prepare_rom(&bytes, &songs[0], &AtomicBool::new(false)).unwrap();
    assert_eq!(&prepared.bytes[0x100..0x103], &[0xc3, 0x50, 1]);
    assert_eq!(&prepared.bytes[0x8000..0xc000], &bytes[0x8000..0xc000]);
    assert_eq!(prepared.hardware, GbSoundSystemHardware::CgbDouble);
    assert!(prepared.wait_start < prepared.wait_end);
    for at in [0x8000, 0x8200, 0x8300, 0x8320, 0x8400, 0x8420, 0x8fff] {
        assert!(
            songs[0].mapped_spans.iter().any(|span| {
                at >= span.effective_offset && at < span.effective_offset + span.byte_len
            }),
            "unmapped {at:x}"
        );
    }
}

#[test]
fn hardware_is_bound_to_the_recognized_code_contract() {
    for hardware in [
        GbSoundSystemHardware::CgbNormal,
        GbSoundSystemHardware::CgbDouble,
    ] {
        let bytes = synthetic_rom_with_hardware(hardware);
        let mut songs = inventory(&bytes);
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].hardware, hardware);
        let prepared = prepare_rom(&bytes, &songs[0], &AtomicBool::new(false)).unwrap();
        assert_eq!(prepared.hardware, hardware);
        songs[0].hardware = if hardware == GbSoundSystemHardware::CgbNormal {
            GbSoundSystemHardware::CgbDouble
        } else {
            GbSoundSystemHardware::CgbNormal
        };
        assert!(validate_song(&bytes, &songs[0], &AtomicBool::new(false)).is_err());
    }
}

#[test]
fn malformed_code_mapping_patterns_and_instruments_reject() {
    for (at, value) in [
        (0x143, 0),
        (0x147, 1),
        (0x148, 4),
        (0x8001, 0xff),
        (0x8062, 0xe2),
        (0x8081, 0xb9),
        (0x8082, 0x7f),
        (0x8085, 0x80),
        (0x8eb8, 1),
        (0x8201, 0x80),
        (0x8203, 0),
        (0x8301, 0x40),
        (0x8320, 19),
        (0x8321, 72),
        (0x8322, 4),
        (0x8401, 0xff),
        (0x8420, 12),
    ] {
        let mut bytes = synthetic_rom();
        bytes[at] = value;
        assert!(inventory(&bytes).is_empty(), "accepted {at:x}={value:x}");
    }
}

#[test]
fn order_loops_need_a_yield_and_effect_indices_are_bounded() {
    let mut bytes = synthetic_rom();
    bytes[0x8323..0x8327].copy_from_slice(&[8, 0xfc, 0xff, 7]);
    assert_eq!(inventory(&bytes).len(), 1);
    bytes[0x8323..0x8327].copy_from_slice(&[5, 4, 0, 9]);
    assert!(inventory(&bytes).is_empty());
}

#[test]
fn banks_and_data_tables_can_relocate_without_title_or_media_identity() {
    let mut bytes = synthetic_rom();
    bytes.copy_within(0x8000..0xc000, 0xc000);
    bytes[0x8000] = 0;
    bytes.copy_within(0xc200..0xc204, 0xc240);
    bytes[0xc084] = 0x40;
    let songs = inventory(&bytes);
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].bank, 3);
    assert_eq!(songs[0].table_entry.effective_offset, 0xc240);
    validate_song(&bytes, &songs[0], &AtomicBool::new(false)).unwrap();
    let mut forged = songs[0].clone();
    forged.index = 1;
    assert!(validate_song(&bytes, &forged, &AtomicBool::new(false)).is_err());
}

#[test]
fn cancelled_and_exhausted_work_are_reported() {
    let bytes = synthetic_rom();
    for (cancelled, remaining, expected) in [
        (true, 2000, ScanStop::Cancelled),
        (false, 0, ScanStop::WorkLimit),
    ] {
        let cancel = AtomicBool::new(cancelled);
        let mut budget = Budget {
            cancel: &cancel,
            remaining,
        };
        assert_eq!(scan(&bytes, &mut Vec::new(), &mut budget, 1), Err(expected));
    }
}

#[test]
fn native_songs_and_fingerprints_share_the_candidate_limit() {
    let mut bytes = synthetic_rom();
    bytes[0x300..0x310].copy_from_slice(b"GHX Audio Engine");
    for limit in [1, 2] {
        let report = crate::scan(
            zeff_emu_common::system::System::Gb,
            &bytes,
            crate::ScanLimits {
                max_candidates: limit,
                ..Default::default()
            },
            &AtomicBool::new(false),
        );
        assert_eq!(report.driver_candidates.len(), 1);
        assert_eq!(
            report.song_count() + report.driver_candidates.len(),
            limit as usize
        );
        assert_eq!(report.gb_sound_system_songs.len(), limit as usize - 1);
    }
}
