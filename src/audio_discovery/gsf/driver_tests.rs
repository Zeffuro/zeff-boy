use super::*;

pub(super) fn fixture_driver(bytes: &[u8], song_number: u32) -> Result<Driver> {
    Ok(Driver {
        profile: "synthetic_bootstrap_fixture",
        callbacks: Callbacks {
            init: ROM_BASE + 0x101,
            init_r0: None,
            select: ROM_BASE + 0x121,
            main: ROM_BASE + 0x141,
            prime_main_after_init: false,
            main_in_vblank: false,
            vsync: Some(ROM_BASE + 0x161),
            dma1: None,
            dma2: None,
        },
        rom_len: bytes.len(),
        table_offset: 0x200,
        song_number,
        aliases: vec![song_number],
        copy_source: ROM_BASE + 0x180,
        copy_destination: 0x0300_2000,
        copy_len: 4,
        witnesses: vec![
            witness(bytes, "synthetic_reset", 0, 4)?,
            witness(bytes, "synthetic_driver", 0x100, 0x100)?,
        ],
    })
}

#[test]
fn bootstrap_preserves_cartridge_and_only_song_word_varies() -> Result<()> {
    let rom = (0..0x405).map(|index| index as u8).collect::<Vec<_>>();
    let cancel = AtomicBool::new(false);
    let first = build(&rom, &fixture_driver(&rom, 7)?, &cancel)?;
    let second = build(&rom, &fixture_driver(&rom, 257)?, &cancel)?;
    assert_eq!(&first.patched_rom[4..rom.len()], &rom[4..]);
    assert_eq!(&first.patched_rom[rom.len()..0x408], &[0, 0, 0]);
    let selector = first.song_number_offset as usize;
    assert_eq!(selector, second.song_number_offset as usize);
    assert_eq!(word(&first.patched_rom, selector), Some(7));
    assert_eq!(word(&second.patched_rom, selector), Some(257));
    assert_eq!(
        &first.patched_rom[..selector],
        &second.patched_rom[..selector]
    );
    assert_eq!(
        &first.patched_rom[selector + 4..],
        &second.patched_rom[selector + 4..]
    );
    assert_eq!(
        (word(&first.patched_rom, 0).unwrap() & 0xFFFFFF) * 4 + 8,
        0x408
    );
    assert_eq!(first.entry_address, ROM_BASE);
    Ok(())
}

#[test]
fn mutation_cancellation_and_cartridge_limit_block_bootstrap() -> Result<()> {
    let mut rom = vec![0; 0x1000];
    let driver = fixture_driver(&rom, 7)?;
    let cancel = AtomicBool::new(false);
    rom[0x1F0] = 1;
    assert!(build(&rom, &driver, &cancel).is_err());
    rom[0x1F0] = 0;
    cancel.store(true, Ordering::Relaxed);
    assert!(build(&rom, &driver, &cancel).is_err());
    cancel.store(false, Ordering::Relaxed);
    rom.resize(MAX_ROM_BYTES, 0);
    assert!(build(&rom, &driver, &cancel).is_err());
    assert!(build(&rom, &fixture_driver(&rom, 7)?, &cancel).is_err());
    Ok(())
}

#[test]
fn callback_decoding_checks_thumb_encoding_and_signed_range() -> Result<()> {
    let mut bytes = vec![0; 0x80];
    bytes[0x20..0x24].copy_from_slice(&[0xFF, 0xF7, 0xEE, 0xFF]);
    assert_eq!(bl_target(&bytes, 0x20)?, 0);
    bytes[0x20] = 0;
    assert!(bl_target(&bytes, 0x20).is_err());
    bytes[0x20..0x24].copy_from_slice(&[0, 0xF0, 0x3E, 0xF8]);
    assert!(bl_target(&bytes, 0x20).is_err());
    bytes[0x22..0x24].copy_from_slice(&[3, 0x48]);
    bytes[0x30..0x34].copy_from_slice(&SOUND_INFO_PTR.to_le_bytes());
    assert_eq!(literal(&bytes, 0x22)?, (0x30, SOUND_INFO_PTR));
    assert!(!iwram(0x0300_7EF0, 0x20));
    assert!(!iwram(0x0200_2000, 4));
    assert!(ram(0x0200_2000, 4));
    Ok(())
}

#[test]
fn known_mixer_profiles_and_callback_patterns_reject_mutation() {
    for profile in &PROFILES {
        let mut bytes = profile.init.bytes.to_vec();
        assert!(matches(&bytes, 0, profile.init));
        bytes[0] ^= 1;
        assert!(!matches(&bytes, 0, profile.init));
        assert_ne!(
            zeff_firmware::sha256_hex(&vec![0; profile.copy_len]),
            profile.copy_sha256
        );
    }
}

fn put_bl(bytes: &mut [u8], at: usize, target: usize) {
    let bits = ((target as i32 - at as i32 - 4) as u32) & 0x7F_FFFF;
    let first = 0xF000 | ((bits >> 12) & 0x7FF) as u16;
    let second = 0xF800 | ((bits >> 1) & 0x7FF) as u16;
    bytes[at..at + 2].copy_from_slice(&first.to_le_bytes());
    bytes[at + 2..at + 4].copy_from_slice(&second.to_le_bytes());
}

#[test]
fn synthetic_driver_requires_consistent_callbacks_state_and_table() -> Result<()> {
    use crate::audio_discovery::{
        self, ScanLimits, SongTableReference,
        tables::{SettingsFields, SongTableBoundary, SongTableEntry},
        test_support,
    };
    use zeff_emu_common::system::System;

    let cancel = AtomicBool::new(false);
    let mut bytes = test_support::gba_fixture();
    let mut song = audio_discovery::scan(System::Gba, &bytes, ScanLimits::default(), &cancel)
        .candidates
        .remove(0);
    bytes.resize(0x5000, 0);
    let put = test_support::put_word;
    let main = 0x1000;
    let init = 0x2400;
    let wrapper = init + 0x78;
    let selector = wrapper + 12;
    let vsync = 0x2000;
    bytes[main..main + patterns::MAIN.bytes.len()].copy_from_slice(patterns::MAIN.bytes);
    bytes[init..init + patterns::INIT_BASE.bytes.len()].copy_from_slice(patterns::INIT_BASE.bytes);
    bytes[vsync..vsync + patterns::VSYNC_BASE.bytes.len()]
        .copy_from_slice(patterns::VSYNC_BASE.bytes);
    bytes[selector..selector + 28].copy_from_slice(SELECTORS[0]);
    bytes[selector + 32..selector + 36].copy_from_slice(&[1, 0xBC, 0, 0x47]);
    bytes[wrapper..wrapper + 2].copy_from_slice(&[0, 0xB5]);
    bytes[wrapper + 6..wrapper + 12].copy_from_slice(&[1, 0xBC, 0, 0x47, 0, 0]);
    put_bl(&mut bytes, wrapper + 2, main);
    for at in [main + 0x3C, main + 0x44] {
        put_bl(&mut bytes, at, 0x3180);
    }
    bytes[0x3180..0x3182].copy_from_slice(&[0x18, 0x47]);
    for (relative, value) in [
        (0x6C, SOUND_INFO_PTR),
        (0x70, SOUND_MAGIC),
        (0x74, 0x0300_2001),
        (0x78, 0x0400_0006),
        (0x7C, 0x350),
        (0x80, 0x630),
    ] {
        put(&mut bytes, main + relative, value);
    }
    for (relative, value) in [
        (2, ROM_BASE + main as u32 + 0x85),
        (10, 0x0300_2000),
        (12, 0x0400_00E0),
        (18, 0x0300_5000),
        (24, 0x0300_6000),
        (30, 0x0093_C600),
        (36, 1),
        (46, ROM_BASE + 0x3F00),
        (0x42, 0x0300_4E00),
    ] {
        let at = literal(&bytes, init + relative)?.0;
        put(&mut bytes, at, value);
    }
    let prefixes: [(usize, &[u8]); 5] = [
        (0x0E, &[0x0B, 0xDF, 0x70, 0x47]),
        (0x14, PROFILES[0].sound_init_prefix),
        (
            0x1A,
            &[
                0x70, 0xB5, 0x81, 0xB0, 0x05, 0x1C, 0x30, 0x49, 0x8F, 0x20, 0x08, 0x80, 0x2F, 0x4B,
                0x00, 0x22, 0x1A, 0x80, 0x2F, 0x48, 0x08, 0x21, 0x01, 0x70,
            ],
        ),
        (
            0x20,
            &[
                0x30, 0xB5, 0x03, 0x1C, 0x21, 0x48, 0x05, 0x68, 0x29, 0x68, 0x21, 0x48, 0x81, 0x42,
                0x3A, 0xD1, 0x48, 0x1C, 0x28, 0x60, 0xFF, 0x24, 0x1C, 0x40,
            ],
        ),
        (
            0x3A,
            &[
                0xF0, 0xB5, 0x07, 0x1C, 0x0E, 0x1C, 0x12, 0x06, 0x14, 0x0E, 0x00, 0x2C, 0x2A, 0xD0,
                0x10, 0x2C, 0x00, 0xD9, 0x10, 0x24, 0x15, 0x48, 0x05, 0x68,
            ],
        ),
    ];
    for (index, (relative, prefix)) in prefixes.into_iter().enumerate() {
        let at = 0x3000 + index * 0x40;
        bytes[at..at + prefix.len()].copy_from_slice(prefix);
        put_bl(&mut bytes, init + relative, at);
    }
    put_bl(&mut bytes, selector + 28, 0x3140);
    bytes[0x3140..0x3142].copy_from_slice(&[0xF0, 0xB5]);
    bytes[0x30B8..0x30BA].copy_from_slice(&[0x41, 0x4C]);
    put(&mut bytes, 0x31C0, 0x0300_4F00);
    bytes[vsync] = 0x3F;
    bytes[vsync + 4] = 0x3F;
    for (relative, value) in [
        (0, SOUND_INFO_PTR),
        (4, SOUND_MAGIC),
        (0x1A, 0x0400_00BC),
        (0x22, 0x8440_0004),
    ] {
        let at = literal(&bytes, vsync + relative)?.0;
        put(&mut bytes, at, value);
    }
    put(&mut bytes, selector + 36, ROM_BASE + 0x3F00);
    put(&mut bytes, selector + 40, ROM_BASE + 0x4000);
    put(&mut bytes, 0x3F00, 0x0300_4000);
    put(&mut bytes, 0x3F04, 0x0300_4100);
    bytes[0x3F08] = 2;
    put(&mut bytes, 0x4008, song.header.canonical_cpu_address);
    let entry = crate::audio_discovery::test_support::rom_span(0x4008, 8);
    song.evidence.song_table_verified = true;
    song.table_entries = vec![SongTableReference {
        table_offset: 0x4000,
        index: 1,
        entry,
        player: 0,
    }];
    let tables = vec![SongTableInventory {
        selector: crate::audio_discovery::test_support::rom_span(selector, 44),
        settings: crate::audio_discovery::test_support::rom_span(init + 0x68, 12),
        settings_fields: SettingsFields {
            sound_mode: crate::audio_discovery::test_support::rom_span(init + 0x68, 4),
            player_count: crate::audio_discovery::test_support::rom_span(init + 0x6C, 4),
            player_table_pointer: crate::audio_discovery::test_support::rom_span(init + 0x70, 4),
        },
        dialect: SongDialect::Mp2k,
        table: crate::audio_discovery::test_support::rom_span(0x4000, 16),
        entries: vec![SongTableEntry {
            index: 1,
            entry,
            header_address: song.header.canonical_cpu_address,
            track_count: 2,
            player: 0,
            kind: SongTableEntryKind::Song,
        }],
        boundary: SongTableBoundary::MediaEnd {
            effective_offset: bytes.len() as u32,
        },
    }];
    let profile = Profile {
        name: "synthetic_mp2k_driver",
        regions: Box::leak(
            vec![fingerprints::Region {
                kind: "driver_code",
                offset: main - 16,
                byte_len: 0x3200 - main + 16,
                sha256: Box::leak(
                    zeff_firmware::sha256_hex(&bytes[main - 16..0x3200]).into_boxed_str(),
                ),
            }]
            .into_boxed_slice(),
        ),
        copy_sha256: Box::leak(
            zeff_firmware::sha256_hex(&bytes[main + 0x84..main + 0x84 + 896]).into_boxed_str(),
        ),
        ..PROFILES[0]
    };
    let profiles = [profile];
    let driver = inspect_profiles(&bytes, &song, &tables, &profiles, &cancel)?;
    assert_eq!(driver.song_number, 1);
    assert!(build(&bytes, &driver, &cancel).is_ok());
    assert!(inspect(&bytes, &song, &tables, &cancel).is_err());
    let mut stack_overlap = bytes.clone();
    put(&mut stack_overlap, 0x3F00, 0x0300_7EC0);
    assert!(inspect_profiles(&stack_overlap, &song, &tables, &profiles, &cancel).is_err());
    for at in [
        selector,
        selector + 28,
        init + 0x54,
        main + 0x84,
        0x3F00,
        0x4008,
        vsync + 2,
        0x3058,
        0x3098,
        0x30D8,
        0x3118,
        0x3158,
    ] {
        let mut mutated = bytes.clone();
        mutated[at] ^= 0xFF;
        assert!(
            build(&mutated, &driver, &cancel).is_err(),
            "changed witness at {at:X}"
        );
        assert!(inspect_profiles(&mutated, &song, &tables, &profiles, &cancel).is_err());
    }
    Ok(())
}
