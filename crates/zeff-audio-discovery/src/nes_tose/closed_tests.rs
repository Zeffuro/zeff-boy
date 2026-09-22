#[cfg(test)]
use super::{NesToseSong, closed::PrgMapping};
use super::{
    closed::{Input, Profile},
    closed_profiles::PROFILES,
};

pub fn synthetic_closed_rom(profile_index: usize) -> Vec<u8> {
    let profile = &PROFILES[profile_index];
    let mut bytes = vec![0; profile.byte_len];
    bytes[..16].copy_from_slice(&profile.header);
    put(profile, &mut bytes, 0xf700, profile.id.as_bytes());
    for group in profile.groups {
        for channel in 0..4 {
            let address = profile.table + u16::from(group.index) * 4 + channel * 4;
            let pointer = group.start + channel * 8;
            put(
                profile,
                &mut bytes,
                address,
                &[
                    84 + channel as u8 * 21,
                    channel as u8,
                    pointer as u8,
                    (pointer >> 8) as u8,
                ],
            );
        }
    }
    put(
        profile,
        &mut bytes,
        profile.init,
        &[0xa9, 0, 0x8d, 0x15, 0x40, 0x60],
    );
    let selector = match profile.input {
        Input::Accumulator => vec![0x8d, 0xb0, 6, 0x60],
        Input::Y => vec![0x8c, 0xb0, 6, 0x60],
        Input::Memory(address) => vec![
            0xad,
            address as u8,
            (address >> 8) as u8,
            0x8d,
            0xb0,
            6,
            0x60,
        ],
    };
    put(profile, &mut bytes, profile.selector, &selector);
    put(
        profile,
        &mut bytes,
        profile.tick,
        &[
            0xa9, 1, 0x8d, 0x15, 0x40, 0xa9, 0xbf, 0x8d, 0, 0x40, 0xa9, 0, 0x8d, 1, 0x40, 0xad,
            0xb0, 6, 0x09, 0x80, 0x8d, 2, 0x40, 0xa9, 8, 0x8d, 3, 0x40, 0xee, 0xb1, 6, 0x60,
        ],
    );
    bytes
}

fn put(profile: &Profile, bytes: &mut [u8], address: u16, data: &[u8]) {
    let at = profile.span(address, data.len() as u16).effective_offset as usize;
    bytes[at..at + data.len()].copy_from_slice(data);
}

pub(super) fn fixture_hash(profile: &Profile, hash: &str) -> bool {
    static HASHES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    let hashes = HASHES.get_or_init(|| {
        PROFILES
            .iter()
            .enumerate()
            .map(|(index, profile)| {
                let bytes = synthetic_closed_rom(index);
                zeff_firmware::sha256_hex(&bytes[16..16 + usize::from(profile.header[4]) * 0x4000])
            })
            .collect()
    });
    PROFILES
        .iter()
        .position(|item| item.id == profile.id)
        .is_some_and(|index| hashes[index] == hash)
}

#[cfg(test)]
fn inventory(bytes: &[u8]) -> Vec<NesToseSong> {
    use std::sync::atomic::AtomicBool;
    let cancel = AtomicBool::new(false);
    let mut budget = crate::Budget {
        cancel: &cancel,
        remaining: 2_000_000,
    };
    let mut songs = Vec::new();
    super::scan(bytes, &mut songs, &mut budget, 4096).unwrap();
    songs
}

#[test]
fn exact_profiles_preserve_groups_and_mapped_assets() {
    use std::sync::atomic::AtomicBool;
    for (index, profile) in PROFILES.iter().enumerate() {
        let bytes = synthetic_closed_rom(index);
        let songs = inventory(&bytes);
        assert_eq!(songs.len(), profile.groups.len());
        for song in &songs {
            assert_eq!(song.profile, profile.id);
            assert_eq!(song.tracks.len(), 4);
            let prepared = super::prepare_rom(&bytes, song, &AtomicBool::new(false)).unwrap();
            assert_eq!(prepared.mapper, profile.mapper);
            assert_eq!(prepared.timing, profile.timing);
            assert_eq!(
                prepared.bytes[9] & 1,
                u8::from(profile.timing == crate::nes_native::NesNativeTiming::Pal)
            );
            let bootstrap = if profile.mapping == PrgMapping::Mmc1Upper16K {
                0xb800_u16
            } else {
                0xf800_u16
            };
            assert!((bootstrap..bootstrap + 256).contains(&prepared.wait_start));
            let vectors = profile.span(0xfffc, 2).effective_offset as usize;
            assert_eq!(
                &prepared.bytes[vectors..vectors + 2],
                &bootstrap.to_le_bytes()
            );
            if profile.mapping == PrgMapping::Mmc1Upper16K {
                let reset = 16 + usize::from(profile.header[4]) * 0x4000 - 4;
                assert_eq!(&prepared.bytes[reset..reset + 2], &bootstrap.to_le_bytes());
            }
            for span in &song.mapped_spans {
                let range = span.effective_offset as usize
                    ..(span.effective_offset + span.byte_len) as usize;
                assert_eq!(&bytes[range.clone()], &prepared.bytes[range]);
            }
        }
        let mut changed_chr = bytes.clone();
        changed_chr[16 + usize::from(profile.header[4]) * 0x4000] ^= 1;
        assert_eq!(inventory(&changed_chr), songs);
    }
}

#[test]
fn source_changes_and_stale_metadata_reject() {
    use std::sync::atomic::AtomicBool;
    for (index, profile) in PROFILES.iter().enumerate() {
        let bytes = synthetic_closed_rom(index);
        let song = inventory(&bytes).remove(0);
        for address in [
            0x8000,
            profile.init,
            profile.selector,
            profile.tick,
            profile.table,
            profile.groups[0].start,
            0xfffa,
        ] {
            let mut changed = bytes.clone();
            changed[profile.span(address, 1).effective_offset as usize] ^= 1;
            assert!(inventory(&changed).is_empty(), "{} {address:x}", profile.id);
            assert!(super::validate_song(&changed, &song, &AtomicBool::new(false)).is_err());
        }
        for at in 0..16 {
            let mut changed = bytes.clone();
            changed[at] ^= 1;
            assert!(inventory(&changed).is_empty(), "{} header {at}", profile.id);
        }
        assert!(inventory(&bytes[..bytes.len() - 1]).is_empty());
        let mut stale = song.clone();
        stale.tracks[0].note_count += 1;
        assert!(super::validate_song(&bytes, &stale, &AtomicBool::new(false)).is_err());
        stale = song.clone();
        stale.mapped_spans[0].byte_len -= 1;
        assert!(super::validate_song(&bytes, &stale, &AtomicBool::new(false)).is_err());
        assert!(super::validate_song(&bytes, &song, &AtomicBool::new(true)).is_err());
    }
}

#[test]
fn closed_profiles_observe_candidate_and_work_limits() {
    use std::sync::atomic::AtomicBool;
    let bytes = synthetic_closed_rom(0);
    let cancel = AtomicBool::new(false);
    let mut budget = crate::Budget {
        cancel: &cancel,
        remaining: 2_000_000,
    };
    assert_eq!(
        super::closed::scan(&bytes, &mut Vec::new(), &mut budget, 0),
        Err(crate::ScanStop::CandidateLimit)
    );
    let mut budget = crate::Budget {
        cancel: &cancel,
        remaining: 2,
    };
    assert_eq!(
        super::closed::scan(&bytes, &mut Vec::new(), &mut budget, 4096),
        Err(crate::ScanStop::WorkLimit)
    );
}

#[test]
fn instruction_dummy_reads_remain_in_mapped_source() {
    for (profile_index, index, address) in [
        (1, 0, 0xa302),
        (1, 0, 0xa305),
        (5, 43, 0x875a),
        (13, 0, 0xd1f5),
    ] {
        let bytes = synthetic_closed_rom(profile_index);
        let songs = inventory(&bytes);
        let song = songs.iter().find(|song| song.index == index).unwrap();
        let offset = PROFILES[profile_index].span(address, 1).effective_offset;
        assert!(song.mapped_spans.iter().any(|span| {
            (span.effective_offset..span.effective_offset + span.byte_len).contains(&offset)
        }));
    }
}
