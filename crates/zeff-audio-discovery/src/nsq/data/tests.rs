use std::sync::atomic::AtomicBool;

use super::*;

const MULTIPLIER: u32 = 7;

fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn hash(name: &str) -> u32 {
    name.bytes().fold(0u32, |value, byte| {
        value
            .wrapping_mul(MULTIPLIER)
            .wrapping_add(byte.to_ascii_lowercase() as u32)
    })
}

fn budget(cancel: &AtomicBool) -> Budget<'_> {
    Budget {
        cancel,
        remaining: 100_000,
    }
}

fn table_fixture(duplicate_selector: bool) -> (Vec<u8>, RomSpan) {
    let mut bytes = vec![0; 0x400];
    let name = b"audio\\sequencer\\song.nsq\0";
    let name_offset = 0x80;
    bytes[name_offset..name_offset + name.len()].copy_from_slice(name);
    put(&mut bytes, 0, 1);
    put(&mut bytes, 4, MULTIPLIER);
    put(&mut bytes, 8, hash("audio\\sequencer\\song.nsq"));
    put(&mut bytes, 12, 0);
    put(&mut bytes, 16, 24);
    put(&mut bytes, 20, 0x200);
    bytes[0x200..0x204].copy_from_slice(b"NSQ\0");
    put(&mut bytes, 0x40, 100);
    put(&mut bytes, 0x44, 0x0800_0000 + name_offset as u32);
    put(&mut bytes, 0x48, if duplicate_selector { 100 } else { 101 });
    put(&mut bytes, 0x4c, 0x0800_0000 + name_offset as u32);
    put(&mut bytes, 0x50, SENTINEL);
    (bytes, RomSpan::new(0, 24))
}

fn song_fixture() -> (Vec<u8>, NsqNativeProfile) {
    const DIRECTORY: usize = 0x80;
    const INSTRUMENT_PATH: usize = 0xa0;
    const SONG_PATH: usize = 0xc0;
    const RELEASE_PREFIX: usize = 0x100;
    const TABLE: usize = 0x200;
    const BANK: usize = 0x500;
    const RAW: usize = 0x1d00;
    const NSQ: usize = 0x1e00;
    let directory = b"audio\\patches\0";
    let instrument_path = b"audio\\instruments.npf\0";
    let song_path = b"audio\\sequencer\\song.nsq\0";
    let mut bytes = vec![0; 0x2200];
    bytes[RELEASE_PREFIX..RELEASE_PREFIX + 19].copy_from_slice(b"audio\\patches\\midi\0");
    bytes[DIRECTORY..DIRECTORY + directory.len()].copy_from_slice(directory);
    bytes[INSTRUMENT_PATH..INSTRUMENT_PATH + instrument_path.len()]
        .copy_from_slice(instrument_path);
    bytes[SONG_PATH..SONG_PATH + song_path.len()].copy_from_slice(song_path);
    put(&mut bytes, 0, 3);
    put(&mut bytes, 4, MULTIPLIER);
    for (index, (name, length, offset)) in [
        ("audio\\instruments.npf", NPF_BYTES as u32, BANK as u32),
        ("audio\\patches\\midi0.raw", 4, RAW as u32),
        ("audio\\sequencer\\song.nsq", 40, NSQ as u32),
    ]
    .into_iter()
    .enumerate()
    {
        let record = 8 + index * 16;
        put(&mut bytes, record, hash(name));
        put(&mut bytes, record + 8, length);
        put(&mut bytes, record + 12, offset);
    }
    put(&mut bytes, TABLE, 100);
    put(&mut bytes, TABLE + 4, 0x0800_0000 + SONG_PATH as u32);
    put(&mut bytes, TABLE + 8, SENTINEL);
    for index in 0..128 {
        put(&mut bytes, BANK + index * 48 + 4, u32::MAX);
    }
    put(&mut bytes, BANK, u32::MAX);
    put(&mut bytes, BANK + 4, 0);
    put(&mut bytes, BANK + 8, u32::MAX);
    put(&mut bytes, BANK + 12, 4096);
    bytes[NSQ..NSQ + 4].copy_from_slice(b"NSQ\0");
    bytes[NSQ + 10] = 9;
    bytes[NSQ + 12] = 60;
    bytes[NSQ + 24..NSQ + 26].copy_from_slice(&24u16.to_le_bytes());
    bytes[NSQ + 26] = 47;
    let native = NsqNativeProfile {
        load_bank: RomSpan::new(0x300, 32),
        load_songs: RomSpan::new(0x340, 32),
        play: RomSpan::new(0x380, 32),
        vblank: RomSpan::new(0x3c0, 32),
        mix: RomSpan::new(0x400, 32),
        song_table: RomSpan::new(TABLE, 16),
        filesystem: RomSpan::new(0, 56),
        bank_directory: RomSpan::new(DIRECTORY, directory.len()),
        instrument_path: RomSpan::new(INSTRUMENT_PATH, instrument_path.len()),
        release_prefix: RomSpan::new(RELEASE_PREFIX, 19),
        setup_spans: vec![RomSpan::new(0x300, 32)],
        bank_arguments: vec![[
            0x0800_0000 + DIRECTORY as u32,
            0x0800_0000 + INSTRUMENT_PATH as u32,
        ]],
    };
    (bytes, native)
}

#[test]
fn scan_tables_keeps_selector_order_and_shared_assets() {
    let (bytes, filesystem) = table_fixture(false);
    let cancel = AtomicBool::new(false);
    assert_eq!(
        scan_tables(&bytes, filesystem, &mut budget(&cancel)).unwrap(),
        vec![RomSpan::new(0x40, 24)]
    );
}

#[test]
fn scan_tables_rejects_duplicate_raw_selectors() {
    let (bytes, filesystem) = table_fixture(true);
    let cancel = AtomicBool::new(false);
    assert!(
        scan_tables(&bytes, filesystem, &mut budget(&cancel))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn parse_song_keeps_table_identity_and_closes_used_assets() {
    let (bytes, native) = song_fixture();
    let cancel = AtomicBool::new(false);
    let song = parse_song(&bytes, &native, 0, &mut budget(&cancel)).unwrap();
    assert_eq!(song.root, native.song_table);
    assert_eq!(song.header, RomSpan::new(0x200, 8));
    assert_eq!(song.sequence, RomSpan::new(0x1e00, 40));
    assert_eq!(
        (song.index, song.slot, song.notes, song.duration_frames),
        (100, 0, 1, 25)
    );
    assert_eq!((song.instruments, song.samples), (1, 1));
    assert!(song.mapped_spans.contains(&RomSpan::new(0x1d00, 4)));
    assert!(song.mapped_spans.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn sentinel_patches_do_not_resolve_sample_zero() {
    let mut bytes = vec![0; NPF_ENTRY_BYTES];
    put(&mut bytes, 4, u32::MAX);
    assert_eq!(patch(&bytes, 0).unwrap(), (None, None));
    put(&mut bytes, 4, (-2i32) as u32);
    put(&mut bytes, 44, 2);
    assert_eq!(patch(&bytes, 0).unwrap(), (None, None));
}

#[test]
fn unsupported_events_and_compressed_assets_are_rejected() {
    let (mut bytes, native) = song_fixture();
    let cancel = AtomicBool::new(false);
    bytes[0x1e0a] = 7;
    assert!(parse_song(&bytes, &native, 0, &mut budget(&cancel)).is_err());
    let (mut bytes, native) = song_fixture();
    put(&mut bytes, 28, 1);
    assert!(parse_song(&bytes, &native, 0, &mut budget(&cancel)).is_err());
}

#[test]
fn unused_pcm_patches_are_required_by_bank_startup() {
    let (mut bytes, native) = song_fixture();
    let cancel = AtomicBool::new(false);
    put(&mut bytes, 0x500 + 5 * 48 + 4, 2);
    put(&mut bytes, 0x500 + 5 * 48 + 8, u32::MAX);
    assert!(parse_song(&bytes, &native, 0, &mut budget(&cancel)).is_err());

    put(&mut bytes, 0, 4);
    put(&mut bytes, 56, hash("audio\\patches\\midi2.raw"));
    put(&mut bytes, 64, 4);
    put(&mut bytes, 68, 0x2100);
    let mut native = native;
    native.filesystem = RomSpan::new(0, 72);
    let song = parse_song(&bytes, &native, 0, &mut budget(&cancel)).unwrap();
    assert_eq!(song.instruments, 1);
    assert_eq!(song.samples, 2);
    assert!(song.mapped_spans.contains(&RomSpan::new(0x2100, 4)));
}

#[test]
fn release_samples_use_the_bound_native_prefix() {
    let (mut bytes, mut native) = song_fixture();
    let cancel = AtomicBool::new(false);
    bytes[0x80..0x8d].copy_from_slice(b"other\\patches");
    put(&mut bytes, 24, hash("other\\patches\\midi0.raw"));
    put(&mut bytes, 0x500 + 8, 1);
    put(&mut bytes, 0, 4);
    put(&mut bytes, 56, hash("audio\\patches\\midi1.raw"));
    put(&mut bytes, 64, 4);
    put(&mut bytes, 68, 0x2100);
    native.filesystem = RomSpan::new(0, 72);
    let song = parse_song(&bytes, &native, 0, &mut budget(&cancel)).unwrap();
    assert_eq!(song.samples, 2);
    assert!(song.mapped_spans.contains(&native.release_prefix));
    put(&mut bytes, 56, hash("other\\patches\\midi1.raw"));
    assert!(parse_song(&bytes, &native, 0, &mut budget(&cancel)).is_err());
}

#[test]
fn sample_ids_fit_the_native_128_entry_state_array() {
    let (mut bytes, native) = song_fixture();
    let cancel = AtomicBool::new(false);
    for id in [127, 128] {
        put(&mut bytes, 0x500 + 4, id);
        put(
            &mut bytes,
            24,
            hash(&format!("audio\\patches\\midi{id}.raw")),
        );
        assert_eq!(
            parse_song(&bytes, &native, 0, &mut budget(&cancel)).is_ok(),
            id == 127
        );
    }
    put(&mut bytes, 0x500 + 4, 0);
    put(&mut bytes, 0x500 + 8, 128);
    assert!(patch(&bytes, 0x500).is_err());
}
