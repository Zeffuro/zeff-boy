use super::{WsToseHardware, legacy::FixedProfile, profiles::Profile};

pub(super) const PROFILE: FixedProfile = FixedProfile {
    driver: Profile {
        name: "ws-tose-fixed-synthetic",
        len: 0x200000,
        fixed: 0x44000,
        end: 0x4461e,
        hash: "",
        segment: 0x4000,
        init: 0x4400,
        selector: 0x4480,
        tick: 0x4500,
        status: 0,
        slots: 0x1000,
        wave: 0x4020,
        envelope: 0x4040,
        frequency: 0x4200,
        counts: &[],
    },
    table: 0x4600,
    count: 5,
    single: 0x4492,
    hardware: WsToseHardware::Mono,
    initial_word: None,
};

pub fn synthetic_legacy_rom() -> Vec<u8> {
    let mut bytes = vec![0; PROFILE.driver.len];
    bytes[0x44020..0x44030].fill(0xf0);
    bytes[0x44040..0x44130].fill(0xff);
    let init = [
        0x33, 0xc0, 0x8e, 0xc0, 0xbf, 0x80, 0, 0xb9, 16, 0, 0xb0, 0xf0, 0xf3, 0xaa, 0xb0, 2, 0xe6,
        0x8f, 0xb0, 8, 0xe6, 0x91, 0xcb,
    ];
    bytes[0x44400..0x44400 + init.len()].copy_from_slice(&init);
    bytes[0x44480] = 0xcb;
    bytes[0x44492] = 0xcb;
    let tick = [
        0xb8, 0, 4, 0xe7, 0x80, 0xb0, 0xff, 0xe6, 0x88, 0xb0, 0x41, 0xe6, 0x90, 0xcb,
    ];
    bytes[0x44500..0x44500 + tick.len()].copy_from_slice(&tick);
    for index in 0..5 {
        let row = 0x44600 + index * 6;
        let pointer = 0x4700_u16 + index as u16 * 32;
        bytes[row..row + 2].copy_from_slice(&(index as u16 * 0x2a).to_le_bytes());
        bytes[row + 2..row + 4].copy_from_slice(&(index as u16 % 4).to_le_bytes());
        bytes[row + 4..row + 6].copy_from_slice(&pointer.to_le_bytes());
        let at = 0x40000 + usize::from(pointer);
        bytes[at..at + 10].copy_from_slice(&[0, 0, 15, 0, 0xfd, 0, 0x30, 8, 0xb0, 0xf0]);
    }
    bytes[0x1ffff6..0x200000].copy_from_slice(&[1, 0, 0, 0, 2, 0, 4, 0, 0, 0]);
    bytes
}

pub(super) fn recognized(bytes: &[u8]) -> bool {
    let driver = PROFILE.driver;
    bytes.len() == driver.len
        && bytes.get(driver.fixed..driver.end)
            == synthetic_legacy_rom().get(driver.fixed..driver.end)
}

#[cfg(test)]
fn inventory(bytes: &[u8]) -> Vec<super::WsToseSong> {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut budget = crate::Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    let mut songs = Vec::new();
    super::scan(bytes, &mut songs, &mut budget, 100).unwrap();
    songs
}

#[test]
fn fixed_rows_select_four_tracks_and_single_track() {
    let bytes = synthetic_legacy_rom();
    let songs = inventory(&bytes);
    assert_eq!(
        songs
            .iter()
            .map(|song| (song.index, song.tracks.len()))
            .collect::<Vec<_>>(),
        [(0, 4), (4, 1)]
    );
    let cancel = std::sync::atomic::AtomicBool::new(false);
    for song in songs {
        let prepared = super::prepare_rom(&bytes, &song, &cancel).unwrap();
        assert_eq!(prepared.bootstrap, super::WsToseBootstrap::Ram);
        assert!((0x3c00..0x3d00).contains(&prepared.wait_start));
        assert_eq!(prepared.wait_end - prepared.wait_start, 7);
        for span in song.mapped_spans {
            let range =
                span.effective_offset as usize..(span.effective_offset + span.byte_len) as usize;
            assert_eq!(prepared.bytes[range.clone()], bytes[range]);
        }
    }
}

#[test]
fn fixed_sequences_reject_unsafe_repeat_pcm_and_immediate_loop() {
    for data in [[0xb0, 4], [0xfd, 4], [0xb0, 0xf0]] {
        let mut bytes = synthetic_legacy_rom();
        bytes[0x44706..0x44708].copy_from_slice(&data);
        assert!(inventory(&bytes).iter().all(|song| song.index != 0));
    }
    let mut bytes = synthetic_legacy_rom();
    bytes[0x44723] = 1;
    assert!(inventory(&bytes).iter().all(|song| song.index != 0));
}

#[test]
fn fixed_authentication_and_inventory_are_revalidated() {
    let bytes = synthetic_legacy_rom();
    let mut song = inventory(&bytes).remove(0);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    song.index = 1;
    assert!(super::prepare_rom(&bytes, &song, &cancel).is_err());
    for at in [0x44400, 0x44500, 0x44600] {
        let mut changed = bytes.clone();
        changed[at] ^= 1;
        assert!(inventory(&changed).is_empty());
    }
}
