use super::*;

fn budget(cancel: &AtomicBool) -> Budget<'_> {
    Budget {
        cancel,
        remaining: 1_000_000,
    }
}

fn set_word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn authored_fixture_retains_original_selectors_and_bounded_data() {
    let bytes = fixture_rom();
    assert_eq!(zeff_firmware::sha256_hex(&bytes), fixture::PROFILE.sha256);
    let cancel = AtomicBool::new(false);
    let mut songs = Vec::new();
    scan(&bytes, &mut songs, &mut budget(&cancel), 2).unwrap();
    assert_eq!(songs.len(), 2);
    assert_eq!(songs[0].title, "Fixture One");
    assert_eq!(songs[1].title, "Fixture Two");
    assert_eq!(songs[1].index, 1);
    assert_eq!(songs[0].channels, 1);
    assert_eq!(songs[0].instruments, 1);
    assert_eq!(songs[0].samples, 1);
    assert_eq!(songs[0].tracks[0].sequence, RomSpan::new(0xa00, 1));
    assert_eq!(songs[1].tracks[0].sequence, RomSpan::new(0xa04, 1));
    assert_eq!(songs[0].tracks[0].initial_volume, 256);
    assert!(songs[0].mapped_spans.contains(&RomSpan::new(0xb00, 8)));
    for song in &songs {
        assert!(song.mapped_spans.iter().all(|&part| {
            !intersects(part, song.native.handoff)
                && part.byte_len != 0
                && part.effective_offset + part.byte_len <= bytes.len() as u32
                && part.canonical_cpu_address == 0x0800_0000 + part.effective_offset
        }));
    }
}

#[test]
fn preparation_preserves_original_source_outside_the_handoff() {
    let bytes = fixture_rom();
    let song = inspect(&bytes, &fixture::PROFILE, 1).unwrap();
    let prepared = prepare_rom(&bytes, &song, &AtomicBool::new(false)).unwrap();
    let hook = song.native.handoff;
    for (offset, (&original, &actual)) in bytes.iter().zip(&prepared.bytes).enumerate() {
        if !(hook.effective_offset as usize..(hook.effective_offset + hook.byte_len) as usize)
            .contains(&offset)
        {
            assert_eq!(original, actual);
        }
    }
    assert_eq!(prepared.wait_loop.byte_len, 12);
    assert!(prepared.wait_loop.effective_offset as usize >= bytes.len());
    assert!(prepared.bytes.len() <= bytes.len() + 512);
    for span in song.mapped_spans {
        let range =
            span.effective_offset as usize..(span.effective_offset + span.byte_len) as usize;
        assert_eq!(bytes[range.clone()], prepared.bytes[range]);
    }
}

#[test]
fn exact_source_and_complete_handle_are_revalidated() {
    let bytes = fixture_rom();
    let song = inspect(&bytes, &fixture::PROFILE, 0).unwrap();
    let cancel = AtomicBool::new(false);
    for mutate in [
        |song: &mut GbassSong| song.index = 1,
        |song: &mut GbassSong| song.title.push('!'),
        |song: &mut GbassSong| song.native.play.canonical_cpu_address += 2,
        |song: &mut GbassSong| song.tracks[0].initial_volume = 1,
        |song: &mut GbassSong| song.mapped_spans.clear(),
        |song: &mut GbassSong| song.native.state_address += 4,
        |song: &mut GbassSong| song.native.schedule = GbassSchedule::VblankIrq,
        |song: &mut GbassSong| song.native.hardware_started_before_handoff = true,
    ] {
        let mut changed = song.clone();
        mutate(&mut changed);
        assert!(prepare_rom(&bytes, &changed, &cancel).is_err());
    }
    let mut changed = bytes.clone();
    changed[0x300] ^= 1;
    assert!(prepare_rom(&changed, &song, &cancel).is_err());
    let mut songs = Vec::new();
    scan(&changed, &mut songs, &mut budget(&cancel), 2).unwrap();
    assert!(songs.is_empty());
    assert!(prepare_rom(&bytes, &song, &AtomicBool::new(true)).is_err());
}

#[test]
fn irq_fixture_uses_a_distinct_qualified_native_schedule() {
    let bytes = fixture_rom_irq();
    assert_eq!(
        zeff_firmware::sha256_hex(&bytes),
        fixture::IRQ_PROFILE.sha256
    );
    let cancel = AtomicBool::new(false);
    let mut songs = Vec::new();
    scan(&bytes, &mut songs, &mut budget(&cancel), 2).unwrap();
    assert_eq!(songs.len(), 2);
    assert_eq!(songs[0].native.schedule, GbassSchedule::VblankIrq);
    assert_eq!(songs[0].native.update_wrapper, RomSpan::new(0x3c0, 10));
    let prepared = prepare_rom(&bytes, &songs[0], &cancel).unwrap();
    assert_eq!(&prepared.bytes[0x3c0..0x3ca], &bytes[0x3c0..0x3ca]);
    let mut wrong = songs[0].clone();
    wrong.native.schedule = GbassSchedule::VblankThenMain;
    assert!(prepare_rom(&bytes, &wrong, &cancel).is_err());
}

#[test]
fn started_fixture_preserves_the_original_hardware_setup_contract() {
    let bytes = fixture_rom_started();
    assert_eq!(
        zeff_firmware::sha256_hex(&bytes),
        fixture::STARTED_PROFILE.sha256
    );
    let cancel = AtomicBool::new(false);
    let mut songs = Vec::new();
    scan(&bytes, &mut songs, &mut budget(&cancel), 2).unwrap();
    assert_eq!(songs.len(), 2);
    assert!(songs[0].native.hardware_started_before_handoff);
    assert_eq!(songs[0].native.handoff, RomSpan::new(0x204, 8));
    assert_eq!(songs[0].native.schedule, GbassSchedule::VblankIrq);
    prepare_rom(&bytes, &songs[0], &cancel).unwrap();
    let old = inspect(&fixture_rom_irq(), &fixture::IRQ_PROFILE, 0).unwrap();
    assert!(
        serde_json::to_value(&old.native).unwrap()["hardware_started_before_handoff"].is_null()
    );
    assert_eq!(
        serde_json::to_value(&songs[0].native).unwrap()["hardware_started_before_handoff"],
        true
    );
    songs[0].native.hardware_started_before_handoff = false;
    assert!(prepare_rom(&bytes, &songs[0], &cancel).is_err());
}

#[test]
fn sample_steps_are_qualified_per_profile_without_changing_pcm_bounds() {
    let mut bytes = fixture_rom_started();
    for step in [0x1_0000, 0x2_0000, 0x2_031a] {
        set_word(&mut bytes, 0x984, step);
        let song = inspect(&bytes, &fixture::STARTED_PROFILE, 0).unwrap();
        assert!(song.mapped_spans.contains(&RomSpan::new(0xb00, 8)));
        if step != 0x1_0000 {
            assert!(inspect(&bytes, &fixture::PROFILE, 0).is_none());
        }
    }
    for step in [0, 0x8000, 0x2_031b, u32::MAX] {
        set_word(&mut bytes, 0x984, step);
        assert!(inspect(&bytes, &fixture::STARTED_PROFILE, 0).is_none());
    }
}

#[test]
fn partial_fixture_keeps_qualified_selections_and_limit_precedence() {
    let bytes = fixture_rom_partial();
    assert_eq!(
        zeff_firmware::sha256_hex(&bytes),
        fixture::PARTIAL_PROFILE.sha256
    );
    let cancel = AtomicBool::new(false);
    for capacity in 0..=2 {
        let mut songs = Vec::new();
        let result = scan(&bytes, &mut songs, &mut budget(&cancel), capacity);
        assert_eq!(songs.len(), capacity);
        assert_eq!(
            result,
            Err(if capacity < 2 {
                ScanStop::CandidateLimit
            } else {
                ScanStop::ValidationLimit
            })
        );
        for (index, song) in songs.iter().enumerate() {
            assert_eq!(usize::from(song.index), index);
            assert_eq!(
                song.warnings.last().unwrap(),
                fixture::PARTIAL_PROFILE.partial_warning.unwrap()
            );
            prepare_rom(&bytes, song, &cancel).unwrap();
        }
    }
    let mut songs = Vec::new();
    let mut limited = Budget {
        cancel: &cancel,
        remaining: 1,
    };
    assert_eq!(
        scan(&bytes, &mut songs, &mut limited, 2),
        Err(ScanStop::WorkLimit)
    );
    assert!(songs.is_empty());
    assert_eq!(
        scan(&bytes, &mut songs, &mut budget(&AtomicBool::new(true)), 2),
        Err(ScanStop::Cancelled)
    );
    let mut forged = inspect(&bytes, &fixture::PARTIAL_PROFILE, 0).unwrap();
    forged.index = 2;
    assert!(prepare_rom(&bytes, &forged, &cancel).is_err());
}

#[test]
fn limits_preserve_completed_candidates_and_do_not_relabel_indices() {
    let bytes = fixture_rom();
    let cancel = AtomicBool::new(false);
    for capacity in 0..=2 {
        let mut songs = Vec::new();
        let result = scan(&bytes, &mut songs, &mut budget(&cancel), capacity);
        assert_eq!(songs.len(), capacity);
        assert_eq!(
            result,
            if capacity < 2 {
                Err(ScanStop::CandidateLimit)
            } else {
                Ok(())
            }
        );
        for (index, song) in songs.iter().enumerate() {
            assert_eq!(usize::from(song.index), index);
        }
    }
    let mut songs = Vec::new();
    let mut limited = Budget {
        cancel: &cancel,
        remaining: 1,
    };
    assert_eq!(
        scan(&bytes, &mut songs, &mut limited, 2),
        Err(ScanStop::WorkLimit)
    );
    assert!(songs.is_empty());
    assert_eq!(
        scan(&bytes, &mut songs, &mut budget(&AtomicBool::new(true)), 2),
        Err(ScanStop::Cancelled)
    );
}

#[test]
fn malformed_table_channels_titles_and_sequences_are_rejected() {
    let fixture = fixture_rom();
    for (offset, value) in [
        (0x804, 3),
        (0x900, 0),
        (0x900, 13),
        (0x904, 0x0a00_0940),
        (0x908, 0x0800_3fff),
        (0x940, 0x0800_0a08),
        (0x940, 0x0800_0200),
        (0x944, 0x0100_000c),
        (0x944, 0x0101_0000),
    ] {
        let mut bytes = fixture.clone();
        set_word(&mut bytes, offset, value);
        assert!(
            inspect(&bytes, &fixture::PROFILE, 0).is_none(),
            "{offset:x}: {value:x}"
        );
    }
    let mut bytes = fixture.clone();
    set_word(&mut bytes, 0x900, 2);
    assert!(inspect(&bytes, &fixture::PROFILE, 0).is_none());
    let mut bytes = fixture;
    bytes[0x920..0x960].fill(b'A');
    assert!(inspect(&bytes, &fixture::PROFILE, 0).is_none());
    assert!(inspect(&bytes, &fixture::PROFILE, 2).is_none());
}

#[test]
fn sample_fixed_point_bounds_and_instrument_pointers_are_checked() {
    let fixture = fixture_rom();
    for (offset, value) in [
        (0x980, 0x0800_0b01),
        (0x984, 0x8000),
        (0x988, 1),
        (0x98c, 0),
        (0x98c, 0x0008_0001),
        (0x990, 0x0009_0000),
        (0x994, 0x0009_0000),
        (0x9a0, 0x0080_09a8),
        (0x9a0, 0x0800_0a00),
    ] {
        let mut bytes = fixture.clone();
        set_word(&mut bytes, offset, value);
        assert!(
            inspect(&bytes, &fixture::PROFILE, 0).is_none(),
            "{offset:x}: {value:x}"
        );
    }
    let mut bytes = fixture;
    let mut profile = fixture::PROFILE;
    profile.sample_data = (0xb00, 9);
    set_word(&mut bytes, 0x994, 9 << 16);
    let song = inspect(&bytes, &profile, 0).unwrap();
    assert!(song.mapped_spans.contains(&RomSpan::new(0xb00, 9)));
}

#[test]
fn mapped_data_cannot_overlap_the_bootstrap_handoff() {
    let mut bytes = fixture_rom();
    let mut profile = fixture::PROFILE;
    profile.sequences = profile.handoff;
    set_word(&mut bytes, 0x940, 0x0800_0200);
    assert!(inspect(&bytes, &profile, 0).is_none());
    let mut profile = fixture::PROFILE;
    profile.sample_data = profile.handoff;
    set_word(&mut bytes, 0x940, 0x0800_0a00);
    set_word(&mut bytes, 0x980, 0x0800_0200);
    assert!(inspect(&bytes, &profile, 0).is_none());
}

#[test]
fn banked_fixture_retains_source_indices_and_configuration_bindings() {
    let bytes = fixture_rom_banked();
    assert_eq!(
        zeff_firmware::sha256_hex(&bytes),
        fixture::BANKED_PROFILES[0].sha256
    );
    let cancel = AtomicBool::new(false);
    let mut songs = Vec::new();
    scan(&bytes, &mut songs, &mut budget(&cancel), 4).unwrap();
    assert_eq!(songs.len(), 4);
    assert_eq!(
        songs.iter().map(|song| song.index).collect::<Vec<_>>(),
        [0, 1, 0, 1]
    );
    for (position, song) in songs.iter().enumerate() {
        let bank = song.native.bank.unwrap();
        assert_eq!(usize::from(bank.index), position / 2);
        assert_eq!(bank.table, RomSpan::new(0xc80, 8));
        assert_eq!(bank.configuration_address, 0x0300_1500);
        assert_eq!(
            song.root.effective_offset,
            if position < 2 { 0x800 } else { 0xc00 }
        );
        assert!(song.mapped_spans.contains(&bank.table));
        let prepared = prepare_rom(&bytes, song, &cancel).unwrap();
        for (offset, (&original, &actual)) in bytes.iter().zip(&prepared.bytes).enumerate() {
            if !(0x204..0x20c).contains(&offset) {
                assert_eq!(original, actual);
            }
        }
    }
    assert_eq!(songs[2].title, "Bank Two One");
    assert_eq!(songs[3].tracks[0].sequence, RomSpan::new(0xe04, 1));
    let old = inspect(&fixture_rom(), &fixture::PROFILE, 0).unwrap();
    assert!(
        serde_json::to_value(old.native)
            .unwrap()
            .get("bank")
            .is_none()
    );
}

#[test]
fn banked_limits_count_completed_songs_across_banks() {
    let bytes = fixture_rom_banked();
    let cancel = AtomicBool::new(false);
    for capacity in 0..=4 {
        let mut songs = Vec::new();
        let result = scan(&bytes, &mut songs, &mut budget(&cancel), capacity);
        assert_eq!(songs.len(), capacity);
        assert_eq!(
            result,
            if capacity < 4 {
                Err(ScanStop::CandidateLimit)
            } else {
                Ok(())
            }
        );
        for song in songs {
            prepare_rom(&bytes, &song, &cancel).unwrap();
        }
    }
    let mut songs = Vec::new();
    let mut limited = Budget {
        cancel: &cancel,
        remaining: 65 + 2 * 19,
    };
    assert_eq!(
        scan(&bytes, &mut songs, &mut limited, 4),
        Err(ScanStop::WorkLimit)
    );
    assert_eq!(songs.len(), 2);
    assert!(
        songs
            .iter()
            .all(|song| song.native.bank.unwrap().index == 0)
    );
    assert!(prepare_rom(&bytes, &songs[0], &AtomicBool::new(true)).is_err());
}

#[test]
fn banked_handles_and_original_bank_tables_are_revalidated() {
    let bytes = fixture_rom_banked();
    let song = inspect(&bytes, &fixture::BANKED_PROFILES[1], 0).unwrap();
    let cancel = AtomicBool::new(false);
    for mutate in [
        |song: &mut GbassSong| song.native.bank.as_mut().unwrap().index = 0,
        |song: &mut GbassSong| song.native.bank.as_mut().unwrap().configuration_address += 4,
        |song: &mut GbassSong| song.native.bank.as_mut().unwrap().table.byte_len = 4,
        |song: &mut GbassSong| song.root = RomSpan::new(0x800, 80),
        |song: &mut GbassSong| song.native.bank = None,
    ] {
        let mut changed = song.clone();
        mutate(&mut changed);
        assert!(prepare_rom(&bytes, &changed, &cancel).is_err());
    }
    let mut changed = bytes.clone();
    set_word(&mut changed, 0xc84, 0x0800_0800);
    assert!(inspect(&changed, &fixture::BANKED_PROFILES[1], 0).is_none());
    assert!(prepare_rom(&changed, &song, &cancel).is_err());
    let mut profile = fixture::BANKED_PROFILES[1];
    profile.bank.as_mut().unwrap().table.byte_len = 4;
    assert!(inspect(&bytes, &profile, 0).is_none());
}

#[test]
fn tagged_samples_require_a_qualified_flag_and_keep_canonical_bounds() {
    let mut bytes = fixture_rom();
    let mut profile = fixture::PROFILE;
    profile.sample_flags = &[0, 0x8000_0000];
    set_word(&mut bytes, 0x980, 0x8800_0b00);
    let song = inspect(&bytes, &profile, 0).unwrap();
    assert!(song.mapped_spans.contains(&RomSpan::new(0xb00, 8)));
    assert!(inspect(&bytes, &fixture::PROFILE, 0).is_none());
    for pointer in [0x4800_0b00, 0x9800_0b00, 0x8800_0b01, 0x8800_0200] {
        set_word(&mut bytes, 0x980, pointer);
        assert!(inspect(&bytes, &profile, 0).is_none());
    }
}
