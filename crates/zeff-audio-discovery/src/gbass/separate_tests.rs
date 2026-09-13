use super::*;

fn budget(cancel: &AtomicBool) -> Budget<'_> {
    Budget {
        cancel,
        remaining: 1_000_000,
    }
}

#[test]
fn separate_pcm_configuration_preserves_its_native_tables() {
    let bytes = fixture_rom_separate();
    assert_eq!(
        zeff_firmware::sha256_hex(&bytes),
        separate_fixture::PROFILE.sha256
    );
    let cancel = AtomicBool::new(false);
    let mut songs = Vec::new();
    scan(&bytes, &mut songs, &mut budget(&cancel), 2).unwrap();
    assert_eq!(songs.len(), 2);
    for (index, song) in songs.iter().enumerate() {
        assert_eq!(song.index, index as u16);
        assert_eq!(song.root, RomSpan::new(0x800, 84));
        assert_eq!(song.instruments, 1);
        assert_eq!(song.samples, 1);
        assert_eq!(song.native.instrument_table, RomSpan::new(0x9a0, 4));
        assert_eq!(song.native.instrument_types, RomSpan::new(0x9a4, 1));
        assert_eq!(song.native.sample_table, RomSpan::new(0x980, 24));
        let prepared = prepare_rom(&bytes, song, &cancel).unwrap();
        for (offset, &original) in bytes.iter().enumerate() {
            if !(0x204..0x20c).contains(&offset) {
                assert_eq!(prepared.bytes[offset], original);
            }
        }
        for part in &song.mapped_spans {
            let range =
                part.effective_offset as usize..(part.effective_offset + part.byte_len) as usize;
            assert_eq!(prepared.bytes[range.clone()], bytes[range]);
        }
    }
}

#[test]
fn separate_pcm_layout_rejects_unqualified_psg_and_misbound_tables() {
    let bytes = fixture_rom_separate();
    let profile = &separate_fixture::PROFILE;
    for (offset, value) in [
        (0x80c, 1),
        (0x810, 0x0800_09a4),
        (0x814, 2),
        (0x818, 0x0200_09a0),
        (0x81c, 0x0900_09a4),
        (0x820, 2),
        (0x824, 0x0200_0980),
    ] {
        let mut changed = bytes.clone();
        changed[offset..offset + 4].copy_from_slice(&u32::to_le_bytes(value));
        assert!(inspect(&changed, profile, 0).is_none());
    }
    let wrong_layout = Profile {
        layout: separate::ConfigurationLayout::Unified,
        ..*profile
    };
    assert!(inspect(&bytes, &wrong_layout, 0).is_none());
    assert!(inspect(&bytes[..0x853], profile, 0).is_none());
    let song = inspect(&bytes, profile, 0).unwrap();
    let cancel = AtomicBool::new(false);
    let mut changed = song.clone();
    changed.root.byte_len = 80;
    assert!(prepare_rom(&bytes, &changed, &cancel).is_err());
    let mut changed = song;
    changed.native.instrument_table = changed.native.instrument_types;
    assert!(prepare_rom(&bytes, &changed, &cancel).is_err());
}

#[test]
fn separate_pcm_scan_keeps_budgets_and_source_identity_authoritative() {
    let bytes = fixture_rom_separate();
    let cancel = AtomicBool::new(false);
    let mut songs = Vec::new();
    assert_eq!(
        scan(&bytes, &mut songs, &mut budget(&cancel), 1),
        Err(ScanStop::CandidateLimit)
    );
    assert_eq!(songs.len(), 1);
    assert!(prepare_rom(&bytes, &songs[0], &cancel).is_ok());
    let mut exhausted = budget(&cancel);
    exhausted.remaining = 1;
    assert_eq!(
        scan(&bytes, &mut Vec::new(), &mut exhausted, 2),
        Err(ScanStop::WorkLimit)
    );
    assert_eq!(
        scan(
            &bytes,
            &mut Vec::new(),
            &mut budget(&AtomicBool::new(true)),
            2
        ),
        Err(ScanStop::Cancelled)
    );
    let mut changed = bytes.clone();
    changed[0x700] ^= 1;
    let mut rejected = Vec::new();
    scan(&changed, &mut rejected, &mut budget(&cancel), 2).unwrap();
    assert!(rejected.is_empty());
    assert!(prepare_rom(&changed, &songs[0], &cancel).is_err());
}
