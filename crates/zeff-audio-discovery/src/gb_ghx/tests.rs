use super::*;

#[test]
fn native_songs_and_fingerprints_share_the_candidate_limit() {
    let mut bytes = synthetic_rom();
    bytes[0x300..0x310].copy_from_slice(b"GHX Audio Engine");
    let cancel = AtomicBool::new(false);
    for limit in [1, 2] {
        let report = crate::scan(
            zeff_emu_common::system::System::Gb,
            &bytes,
            crate::ScanLimits {
                max_candidates: limit,
                ..Default::default()
            },
            &cancel,
        );
        assert_eq!(report.driver_candidates.len(), 1);
        assert_eq!(
            report.song_count() + report.driver_candidates.len(),
            limit as usize
        );
        assert_eq!(report.gb_ghx_songs.len(), limit as usize - 1);
        assert_eq!(
            report.status,
            if limit == 1 {
                crate::ScanStatus::Incomplete(ScanStop::CandidateLimit)
            } else {
                crate::ScanStatus::Complete
            }
        );
    }
}

pub(super) static PROFILE: profiles::Profile = profiles::Profile {
    name: "ghx-synthetic-v1",
    hash: "78323f90ab3df1067201a905fd89fa33f5951f552180709ecbec2dd7a18df28a",
    table_operand: 0x41,
    direct_patterns: false,
    wave_table: false,
    aliases: &[&[0x41]],
};

pub fn synthetic_rom() -> Vec<u8> {
    let mut bytes = vec![0; 0x10000];
    bytes[0x143] = 0xc0;
    bytes[0x147] = 0x19;
    bytes[0x148] = 1;
    bytes[0x8000..0x8009].copy_from_slice(&[0xc3, 0x40, 0x40, 0xc3, 0x80, 0x40, 0xc3, 0x90, 0x40]);
    let selector = [
        0x21, 0x00, 0x41, 0x3e, 0x80, 0xe0, 0x26, 0x3e, 0x77, 0xe0, 0x24, 0x3e, 0x11, 0xe0, 0x25,
        0x3e, 0x80, 0xe0, 0x11, 0x3e, 0xf0, 0xe0, 0x12, 0x3e, 0x20, 0xe0, 0x13, 0x3e, 0x87, 0xe0,
        0x14, 0xc9,
    ];
    bytes[0x8040..0x8040 + selector.len()].copy_from_slice(&selector);
    bytes[0x8080..0x8088].copy_from_slice(&[0x21, 0x00, 0xc1, 0x34, 0x7e, 0xe0, 0x13, 0xc9]);
    bytes[0x8090..0x8097].copy_from_slice(&[0xaf, 0xe0, 0x26, 0xea, 0x00, 0xc1, 0xc9]);
    bytes[0x8100..0x8102].copy_from_slice(&[0x20, 0x41]);
    bytes[0x8120..0x812c]
        .copy_from_slice(&[b'G', b'H', b'X', 1, 2, 0, 0, 0x42, 0, 0x43, 0x40, 0x41]);
    bytes[0x8140..0x8146].copy_from_slice(&[0, 0x60, 0x41, 0, 0x60, 0x41]);
    for channel in 0..4 {
        bytes[0x8160 + channel * 2] = channel as u8;
        let pattern = 0x4220u16 + channel as u16 * 16;
        let instrument = 0x4320u16 + channel as u16 * 32;
        bytes[0x8200 + channel * 2..0x8202 + channel * 2].copy_from_slice(&pattern.to_le_bytes());
        bytes[0x8300 + channel * 2..0x8302 + channel * 2]
            .copy_from_slice(&instrument.to_le_bytes());
        bytes[usize::from(pattern) + 0x4000..usize::from(pattern) + 0x4003].copy_from_slice(&[
            0x40 | 25,
            channel as u8 + 1,
            0,
        ]);
        let at = usize::from(instrument) + 0x4000;
        bytes[at..at + 3].copy_from_slice(&[1, 1, 0xf0]);
    }
    bytes
}

#[cfg(test)]
fn inventory(bytes: &[u8]) -> Vec<GbGhxSong> {
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
fn synthetic_inventory_and_native_handoff_revalidate() {
    let bytes = synthetic_rom();
    let songs = inventory(&bytes);
    assert_eq!(songs.len(), 1);
    assert!(songs[0].tracks.iter().all(|track| track.note_count == 2));
    let prepared = prepare_rom(&bytes, &songs[0], &AtomicBool::new(false)).unwrap();
    assert_eq!(&prepared.bytes[0x100..0x103], &[0xc3, 0x50, 1]);
    assert_eq!(prepared.hardware, GbGhxHardware::CgbDouble);
    assert!(prepared.wait_start < prepared.wait_end);
    for at in [0x8000, 0x8100, 0x8120, 0x8140, 0x8160, 0x8220, 0x836e] {
        assert!(
            songs[0].mapped_spans.iter().any(|span| {
                at >= span.effective_offset && at < span.effective_offset + span.byte_len
            }),
            "unmapped {at:x}"
        );
    }
}

#[test]
fn code_data_hardware_and_illegal_closure_mutations_are_rejected() {
    for (at, value) in [
        (0x143, 0x80),
        (0x147, 1),
        (0x148, 3),
        (0x8001, 0xff),
        (0x8053, 0xff),
        (0x8124, 0),
        (0x8101, 0x80),
        (0x8127, 0x80),
        (0x8129, 0x80),
        (0x8142, 0x80),
        (0x8145, 0x80),
        (0x8201, 0x80),
        (0x8301, 0x80),
        (0x8360, 0xe1),
        (0x8324, 0x80),
        (0x8324, 0x81),
    ] {
        let mut bytes = synthetic_rom();
        bytes[at] = value;
        assert!(inventory(&bytes).is_empty(), "mutation {at:x}/{value:x}");
    }
    let mut bytes = synthetic_rom();
    bytes[0x8080] = 0xe9;
    assert!(inventory(&bytes).is_empty());
    bytes.truncate(0x9000);
    assert!(inventory(&bytes).is_empty());
}

#[test]
fn identities_reject_forgery_and_allow_relocated_banks_and_ram() {
    let mut bytes = synthetic_rom();
    let original = inventory(&bytes).remove(0);
    let cancel = AtomicBool::new(false);
    for field in 0..6 {
        let mut forged = original.clone();
        match field {
            0 => forged.index += 1,
            1 => forged.module += 1,
            2 => forged.subsong += 1,
            3 => forged.title.push('x'),
            4 => forged.mapped_spans.clear(),
            _ => forged.tracks[0].note_count += 1,
        }
        assert!(validate_song(&bytes, &forged, &cancel).is_err());
    }
    bytes[0x8095] = 0xc2;
    bytes[0x8082] = 0xc2;
    assert_eq!(inventory(&bytes).len(), 1);
    bytes.copy_within(0x8000..0xc000, 0xc000);
    assert_eq!(inventory(&bytes).len(), 2);
    bytes[0x4000] = 0x12;
    assert_eq!(inventory(&bytes).len(), 2);
}

#[test]
fn cancellation_and_limits_bound_code_and_sequence_walks() {
    let bytes = synthetic_rom();
    for (cancelled, work, maximum) in [(true, 100000, 100), (false, 20, 100), (false, 100000, 0)] {
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

#[test]
fn direct_playlists_and_instrument_back_edges_stay_inside_their_closure() {
    static DIRECT: profiles::Profile = profiles::Profile {
        name: "ghx-direct-fixture",
        hash: "",
        table_operand: 0x41,
        direct_patterns: true,
        wave_table: false,
        aliases: &[&[0x41]],
    };
    let mut bytes = synthetic_rom();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 2_000_000,
    };
    let mut driver = profiles::recognized(&bytes, &mut budget).unwrap().remove(0);
    driver.profile = &DIRECT;
    bytes[0x8160..0x816b].fill(0);
    for channel in 0..4 {
        let pointer = 0x4220u16 + channel as u16 * 16;
        bytes[0x8160 + channel * 3..0x8162 + channel * 3].copy_from_slice(&pointer.to_le_bytes());
    }
    bytes[0x8325] = 0x81;
    let song = sequence::song(&bytes, &driver, 0, 0, &mut budget).unwrap();
    assert!(song.tracks.iter().all(|track| track.note_count == 2));
    bytes[0x816a] = 0x80;
    assert!(sequence::song(&bytes, &driver, 0, 0, &mut budget).is_err());
}
