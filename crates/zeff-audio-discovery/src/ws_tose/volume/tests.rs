#[cfg(test)]
use std::sync::atomic::AtomicBool;

#[cfg(test)]
use crate::{Budget, ScanStop};

use super::super::profiles::Profile;
use super::VolumeProfile;

pub(super) const PROFILE: VolumeProfile = VolumeProfile {
    driver: Profile {
        name: "ws-tose-volume-synthetic",
        len: 0x400000,
        fixed: 0x3e5e00,
        end: 0x3e6200,
        hash: "",
        segment: 0xe000,
        init: 0x6000,
        selector: 0x6080,
        tick: 0x6140,
        status: 0,
        slots: 0x2000,
        wave: 0x5e20,
        envelope: 0x5e40,
        frequency: 0x5f50,
        counts: &[],
    },
    table: 0x6200,
    row_count: 10,
    callers: [(0x6100, 2), (0x6120, 2)],
    control_start: 0x6000,
    control_end: 0x6140,
    volume_setup: 0x6090,
    color_setup: 0x6095,
    interrupt: 0x6020,
};

pub fn synthetic_volume_rom() -> Vec<u8> {
    let mut bytes = vec![0; PROFILE.driver.len];
    let driver = PROFILE.driver;
    let fixed = driver.fixed;
    bytes[driver.offset(driver.wave)..driver.offset(driver.wave) + 16].fill(0xf0);
    bytes[driver.offset(driver.envelope)..driver.offset(driver.envelope) + 240].fill(0xf0);
    bytes[driver.offset(driver.frequency)..driver.offset(driver.frequency) + 48].fill(1);

    let mut init = vec![
        0x33, 0xc0, 0x8e, 0xd8, 0xbb, 0, 0x20, 0xb9, 8, 0, 0xb8, 0xff, 0xff,
    ];
    let clear_slots = init.len();
    init.extend([0x89, 0x07, 0x83, 0xc3, 0x2a, 0xe2, 0]);
    init[clear_slots + 6] = (clear_slots as isize - init.len() as isize) as u8;
    init.extend([
        0x33, 0xc0, 0x8e, 0xc0, 0xbf, 0, 0x22, 0xb9, 16, 0, 0xb0, 0xf0, 0xf3, 0xaa, 0xb0, 0x88,
        0xe6, 0x8f, 0xb0, 8, 0xe6, 0x91, 0xb0, 0, 0xe6, 0x90, 0xcb,
    ]);
    write(&mut bytes, fixed + 0x200, &init);

    for (entry, count) in [(0x6080, 4), (0x6085, 3), (0x608a, 2), (0x608f, 1)] {
        let after = entry + 5;
        write(
            &mut bytes,
            driver.offset(entry),
            &[0xb9, count, 0, 0xeb, (0x60a0_i32 - i32::from(after)) as u8],
        );
    }
    let mut selector = vec![
        0x33, 0xdb, 0x8e, 0xdb, 0xa3, 0xf0, 0x1d, 0x89, 0x0e, 0xf2, 0x1d, 0xc6, 0x06, 0xf4, 0x1d,
        3, 0xbf, 0, 0x20, 0xb8, 2, 0,
    ];
    let activate = selector.len();
    selector.extend([0x89, 0x05, 0x83, 0xc7, 0x2a, 0xe2, 0, 0xcb]);
    selector[activate + 6] = (activate as isize - (activate + 7) as isize) as u8;
    write(&mut bytes, driver.offset(0x60a0), &selector);

    let mut tick = vec![0x33, 0xc0, 0x8e, 0xd8, 0x80, 0x3e, 0xf4, 0x1d, 0];
    let muted = tick.len();
    tick.extend([
        0x74, 0, 0xc6, 0x06, 0x40, 0x22, 0xff, 0xb8, 0, 4, 0xe7, 0x80, 0xb0, 1, 0xe6, 0x90, 0xfe,
        0x0e, 0xf4, 0x1d,
    ]);
    let active = tick.len();
    tick.extend([0x75, 0]);
    let clear_slots = tick.len();
    tick.extend([
        0xbf, 0, 0x20, 0xb9, 8, 0, 0xb8, 0xff, 0xff, 0x89, 0x05, 0x83, 0xc7, 0x2a, 0xe2, 0, 0xb0,
        0, 0xe6, 0x90, 0xcb,
    ]);
    let quiet = tick.len();
    tick.push(0xcb);
    tick[muted + 1] = (quiet as isize - (muted + 2) as isize) as u8;
    tick[active + 1] = (quiet as isize - (active + 2) as isize) as u8;
    tick[clear_slots + 15] = (clear_slots as isize + 9 - (clear_slots + 16) as isize) as u8;
    write(&mut bytes, driver.offset(driver.tick), &tick);

    write(
        &mut bytes,
        control(0x6000),
        &[
            0x50, 0x1e, 0x33, 0xc0, 0x8e, 0xd8, 0xa0, 0x40, 0x22, 0xe6, 0x88, 0x1f, 0x58, 0xc3,
        ],
    );
    let mut interrupt = vec![
        0x50, 0x53, 0x51, 0x52, 0x56, 0x57, 0x1e, 0x06, 0x33, 0xc0, 0x8e, 0xd8, 0x9a, 0x40, 0x61,
        0, 0xe0, 0xe8, 0, 0, 0xb0, 0x80, 0xe6, 0xb6, 0x07, 0x1f, 0x5f, 0x5e, 0x5a, 0x59, 0x5b,
        0x58, 0xcf,
    ];
    interrupt[18..20].copy_from_slice(&(0x6000_i16 - 0x6034_i16).to_le_bytes());
    write(&mut bytes, control(PROFILE.interrupt), &interrupt);
    write(
        &mut bytes,
        control(PROFILE.volume_setup),
        &[0xc6, 0x06, 0x4a, 0, 0x3f],
    );
    write(
        &mut bytes,
        control(PROFILE.color_setup),
        &[0xb0, 0xea, 0xe6, 0x60],
    );

    for (at, first, entry) in [
        (0x6100, 0, 0x608f),
        (0x610b, 1, 0x608a),
        (0x6120, 3, 0x6085),
        (0x612b, 6, 0x6080),
    ] {
        write(
            &mut bytes,
            control(at),
            &[
                0xb8,
                first,
                0,
                0x9a,
                entry as u8,
                (entry >> 8) as u8,
                0,
                0xe0,
                0xcb,
                0x90,
                0x90,
            ],
        );
    }

    let mut row = 0_usize;
    for count in 1..=4 {
        for slot in 0..count {
            let table = driver.offset(PROFILE.table) + row * 6;
            bytes[table..table + 2].copy_from_slice(&((slot * 0x2a) as u16).to_le_bytes());
            bytes[table + 2..table + 4].copy_from_slice(&((row % 4) as u16).to_le_bytes());
            bytes[table + 4..table + 6]
                .copy_from_slice(&(0x6300_u16 + (row * 8) as u16).to_le_bytes());
            let sequence = driver.offset(0x6300 + (row * 8) as u16);
            bytes[sequence..sequence + 2].copy_from_slice(&8_u16.to_le_bytes());
            bytes[sequence + 2..sequence + 4].copy_from_slice(&1_u16.to_le_bytes());
            bytes[sequence + 4..sequence + 6].copy_from_slice(&1_u16.to_le_bytes());
            bytes[sequence + 6..sequence + 8].copy_from_slice(&0x00ff_u16.to_le_bytes());
            row += 1;
        }
    }
    let footer = bytes.len() - 9;
    bytes[footer] = 1;
    bytes
}

pub(super) fn recognized(bytes: &[u8]) -> bool {
    let synthetic = synthetic_volume_rom();
    bytes.len() == PROFILE.driver.len
        && bytes.get(PROFILE.driver.fixed..PROFILE.driver.end)
            == synthetic.get(PROFILE.driver.fixed..PROFILE.driver.end)
        && bytes.get(control(PROFILE.control_start)..control(PROFILE.control_end))
            == synthetic.get(control(PROFILE.control_start)..control(PROFILE.control_end))
        && bytes.get(control(PROFILE.volume_setup)..control(PROFILE.volume_setup) + 5)
            == synthetic.get(control(PROFILE.volume_setup)..control(PROFILE.volume_setup) + 5)
        && bytes.get(control(PROFILE.color_setup)..control(PROFILE.color_setup) + 4)
            == synthetic.get(control(PROFILE.color_setup)..control(PROFILE.color_setup) + 4)
}

fn control(address: u16) -> usize {
    PROFILE.control_span(address, 0).effective_offset as usize
}

fn write(bytes: &mut [u8], at: usize, source: &[u8]) {
    bytes[at..at + source.len()].copy_from_slice(source);
}

#[cfg(test)]
fn inventory(bytes: &[u8]) -> (bool, Vec<super::WsToseSong>) {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    let mut songs = Vec::new();
    (
        super::scan(bytes, &mut songs, &mut budget, 100).unwrap(),
        songs,
    )
}

#[test]
fn recognizes_all_group_shapes_and_preserves_mapped_source() {
    let bytes = synthetic_volume_rom();
    let (recognized, songs) = inventory(&bytes);
    assert!(recognized);
    assert_eq!(songs.len(), 4);
    for (song, count) in songs.iter().zip(1..=4) {
        assert_eq!(song.profile, PROFILE.driver.name);
        assert_eq!(song.index, (count - 1) as u16);
        assert_eq!(song.tracks.len(), count);
        assert!(song.tracks.iter().all(|track| track.note_count == 1));
    }
    assert_eq!(songs[0].table_entry.effective_offset, 0x3e6200);
    assert_eq!(songs[0].table_entry.canonical_cpu_address, 0xe6200);
    assert!(songs[3].mapped_spans.iter().any(|span| {
        span.effective_offset == 0x3e6224
            && span.byte_len == 24
            && span.canonical_cpu_address == 0xe6224
    }));
    assert!(songs[0].mapped_spans.iter().any(|span| {
        span.effective_offset == 0x3f6000
            && span.byte_len >= 0x140
            && span.canonical_cpu_address == 0xf6000
    }));
    let cancel = AtomicBool::new(false);
    let prepared = super::prepare_rom(&bytes, &songs[0], &cancel).unwrap();
    for span in &songs[0].mapped_spans {
        let start = span.effective_offset as usize;
        let end = start + span.byte_len as usize;
        assert_eq!(&prepared.bytes[start..end], &bytes[start..end]);
    }
}

#[test]
fn rejects_malformed_source_rows_and_sequences() {
    let mut code = synthetic_volume_rom();
    code[PROFILE.driver.offset(PROFILE.driver.init)] ^= 1;
    assert!(!inventory(&code).0);
    let mut mixer = synthetic_volume_rom();
    mixer[control(0x6000)] ^= 1;
    assert!(!inventory(&mixer).0);
    let mut caller = synthetic_volume_rom();
    caller[control(0x6100)] ^= 1;
    assert!(!inventory(&caller).0);
    let mut color = synthetic_volume_rom();
    color[control(PROFILE.color_setup)] ^= 1;
    assert!(!inventory(&color).0);
    let mut data = synthetic_volume_rom();
    data[PROFILE.driver.offset(PROFILE.table) + 4..PROFILE.driver.offset(PROFILE.table) + 6]
        .copy_from_slice(&0x6200_u16.to_le_bytes());
    assert_eq!(inventory(&data).1.len(), 3);
    let mut slots = synthetic_volume_rom();
    slots[PROFILE.driver.offset(PROFILE.table)] = 1;
    assert_eq!(inventory(&slots).1.len(), 3);
    let mut sequence = synthetic_volume_rom();
    let at = PROFILE.driver.offset(0x6300);
    sequence[at + 4..at + 8].copy_from_slice(&[0xfd, 0, 0xb0, 0xf0]);
    assert_eq!(inventory(&sequence).1.len(), 3);
}

#[test]
fn accepts_the_volume_wrapped_sweep_index_only() {
    const SWEEP_TRACK: u16 = 0x7000;
    let mut bytes = synthetic_volume_rom();
    let table = PROFILE.driver.offset(PROFILE.table);
    bytes[table + 4..table + 6].copy_from_slice(&SWEEP_TRACK.to_le_bytes());
    let track = PROFILE.driver.offset(SWEEP_TRACK);
    bytes[track..track + 2].copy_from_slice(&8_u16.to_le_bytes());
    bytes[track + 2..track + 4].copy_from_slice(&1_u16.to_le_bytes());
    bytes[track + 4..track + 6].copy_from_slice(&0x80a1_u16.to_le_bytes());
    bytes[track + 6..track + 8].copy_from_slice(&1_u16.to_le_bytes());
    bytes[track + 8..track + 10].copy_from_slice(&0x00ff_u16.to_le_bytes());

    let (_, songs) = inventory(&bytes);
    assert_eq!(songs.len(), 4);
    assert_eq!(songs[0].tracks[0].note_count, 1);

    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    let mut reader = super::super::sequence::Reader::legacy(
        &bytes,
        PROFILE.driver.fixed / 0x10000,
        PROFILE.driver,
        &mut budget,
    );
    assert!(reader.track(SWEEP_TRACK, 0).is_err());
}

#[test]
fn refuses_stop_only_and_forged_inventory() {
    let mut bytes = synthetic_volume_rom();
    let at = PROFILE.driver.offset(0x6300);
    bytes[at + 4..at + 6].copy_from_slice(&0x00ff_u16.to_le_bytes());
    assert_eq!(inventory(&bytes).1.len(), 3);
    let bytes = synthetic_volume_rom();
    let mut songs = inventory(&bytes).1;
    songs[0].index = 1;
    let cancel = AtomicBool::new(false);
    assert!(super::prepare_rom(&bytes, &songs[0], &cancel).is_err());
}

#[test]
fn cancellation_work_and_candidate_limits_are_typed() {
    let bytes = synthetic_volume_rom();
    let cancel = AtomicBool::new(true);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    assert_eq!(
        super::scan(&bytes, &mut Vec::new(), &mut budget, 1),
        Err(ScanStop::Cancelled)
    );
    cancel.store(false, std::sync::atomic::Ordering::Relaxed);
    budget.remaining = 1;
    assert_eq!(
        super::scan(&bytes, &mut Vec::new(), &mut budget, 1),
        Err(ScanStop::WorkLimit)
    );
    budget.remaining = 100_000;
    assert_eq!(
        super::scan(&bytes, &mut Vec::new(), &mut budget, 0),
        Err(ScanStop::CandidateLimit)
    );
}
