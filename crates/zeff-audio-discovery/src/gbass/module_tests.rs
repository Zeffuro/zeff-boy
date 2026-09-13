use super::*;

fn scan_fixture(capacity: usize) -> (Vec<GbassSong>, Result<(), ScanStop>) {
    let bytes = fixture_rom_module();
    let cancel = AtomicBool::new(false);
    let mut songs = Vec::new();
    let result = scan(
        &bytes,
        &mut songs,
        &mut Budget {
            cancel: &cancel,
            remaining: 1_000_000,
        },
        capacity,
    );
    (songs, result)
}

#[test]
fn copied_modules_keep_source_provenance_and_distinct_native_identity() {
    let bytes = fixture_rom_module();
    assert_eq!(
        zeff_firmware::sha256_hex(&bytes),
        module_fixture::PROFILES[0].sha256
    );
    let (songs, result) = scan_fixture(4);
    assert_eq!(result, Ok(()));
    assert_eq!(
        songs.iter().map(|song| song.index).collect::<Vec<_>>(),
        [0, 1, 0, 1]
    );
    for (position, song) in songs.iter().enumerate() {
        let module = song.native.module.unwrap();
        let offset = if position < 2 { 0x1000 } else { 0x5000 };
        assert_eq!(usize::from(module.index), position / 2);
        assert!(song.native.bank.is_none());
        assert_eq!(module.source, RomSpan::new(offset, 0x4000));
        assert_eq!(song.root, RomSpan::new(offset + 0x800, 80));
        assert_eq!(module.configuration_address, 0x0200_0800);
        assert_eq!(song.native.song_table, RomSpan::new(offset + 0x900, 24));
        assert_eq!(
            song.tracks[0].sequence,
            RomSpan::new(offset + 0xa00 + position % 2 * 4, 1)
        );
        assert_eq!(module.runtime_address(song.native.play), Some(0x0200_0380));
        assert!(song.mapped_spans.contains(&module.loader));
        assert!(
            song.mapped_spans
                .iter()
                .all(|span| span.canonical_cpu_address >= 0x0800_0000)
        );
        let prepared = prepare_rom(&bytes, song, &AtomicBool::new(false)).unwrap();
        for (at, (&original, &actual)) in bytes.iter().zip(&prepared.bytes).enumerate() {
            if !(0x204..0x20c).contains(&at) && !(offset + 0x204..offset + 0x20c).contains(&at) {
                assert_eq!(original, actual);
            }
        }
        assert!(
            prepared.bytes[bytes.len()..]
                .windows(4)
                .any(|part| part == 0x0200_0381_u32.to_le_bytes())
        );
    }
    let old = inspect(&fixture_rom(), &fixture::PROFILE, 0).unwrap();
    assert!(
        serde_json::to_value(old.native)
            .unwrap()
            .get("module")
            .is_none()
    );
}

#[test]
fn copied_module_limits_and_cancellation_preserve_usable_selections() {
    for capacity in 0..=4 {
        let (songs, result) = scan_fixture(capacity);
        assert_eq!(songs.len(), capacity);
        assert_eq!(
            result,
            if capacity < 4 {
                Err(ScanStop::CandidateLimit)
            } else {
                Ok(())
            }
        );
    }
    let bytes = fixture_rom_module();
    let cancel = AtomicBool::new(false);
    let mut songs = Vec::new();
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 257 + 2 * 19,
    };
    assert_eq!(
        scan(&bytes, &mut songs, &mut budget, 4),
        Err(ScanStop::WorkLimit)
    );
    assert_eq!(songs.len(), 2);
    assert!(prepare_rom(&bytes, &songs[0], &AtomicBool::new(true)).is_err());
}

#[test]
fn copied_module_handles_and_loaded_pointer_bounds_are_revalidated() {
    let bytes = fixture_rom_module();
    let song = inspect(&bytes, &module_fixture::PROFILES[1], 0).unwrap();
    for mutate in [
        |song: &mut GbassSong| song.native.module.as_mut().unwrap().index = 0,
        |song: &mut GbassSong| song.native.module.as_mut().unwrap().source.byte_len -= 4,
        |song: &mut GbassSong| song.native.module.as_mut().unwrap().configuration_address += 4,
        |song: &mut GbassSong| song.native.module.as_mut().unwrap().loader.effective_offset += 4,
        |song: &mut GbassSong| song.native.module.as_mut().unwrap().loader_handoff.byte_len += 4,
        |song: &mut GbassSong| song.native.module.as_mut().unwrap().load_address = 0x0300_0000,
        |song: &mut GbassSong| song.native.module = None,
    ] {
        let mut changed = song.clone();
        mutate(&mut changed);
        assert!(prepare_rom(&bytes, &changed, &AtomicBool::new(false)).is_err());
    }
    for address in [0x0200_3ff4_u32, 0x0204_0000, 0x0100_0900, 0x0800_5900] {
        let mut changed = bytes.clone();
        changed[0x5808..0x580c].copy_from_slice(&address.to_le_bytes());
        assert!(inspect(&changed, &module_fixture::PROFILES[1], 0).is_none());
        assert!(prepare_rom(&changed, &song, &AtomicBool::new(false)).is_err());
    }
}

#[test]
fn copied_module_loader_and_handoff_must_be_separate_from_mapped_data() {
    let bytes = fixture_rom_module();
    for mutate in [
        |profile: &mut Profile| {
            profile
                .module
                .as_mut()
                .unwrap()
                .source
                .canonical_cpu_address += 4
        },
        |profile: &mut Profile| profile.module.as_mut().unwrap().source.byte_len = 0x40004,
        |profile: &mut Profile| profile.module.as_mut().unwrap().loader = RomSpan::new(0x1300, 4),
        |profile: &mut Profile| {
            profile.module.as_mut().unwrap().loader_handoff = RomSpan::new(0x700, 8)
        },
        |profile: &mut Profile| {
            profile.module.as_mut().unwrap().loader_handoff = RomSpan::new(0x1300, 8)
        },
        |profile: &mut Profile| profile.handoff = (0x1a00, 8),
        |profile: &mut Profile| profile.play = (0x6000, 8),
    ] {
        let mut profile = module_fixture::PROFILES[0];
        mutate(&mut profile);
        assert!(inspect(&bytes, &profile, 0).is_none());
    }
}
