use super::*;

fn fixture() -> (Vec<u8>, Profile) {
    let mut bytes = vec![0; 0x10000];
    bytes[0x8000..0x8003].copy_from_slice(&[0x21, 0x08, 0xde]);
    bytes[0x8050] = 0xc9;
    bytes[0x8100..0x8102].copy_from_slice(&0x4140_u16.to_le_bytes());
    bytes[0x8140..0x8146].copy_from_slice(&[0, 0, 4, 0, 1, 3]);
    for channel in 0..4 {
        let address: u16 = 0x4200 + channel * 0x10;
        let entry = 0x8146 + usize::from(channel) * 4;
        bytes[entry..entry + 2].copy_from_slice(&address.to_le_bytes());
        bytes[entry + 2] = 0xf4;
        bytes[entry + 3] = channel as u8;
        bytes[usize::from(address) + 0x4000..usize::from(address) + 0x4003]
            .copy_from_slice(&[0x81, 4, 0xf2]);
    }
    let profile = Profile {
        name: "synthetic-sega-psg",
        sha256: "unused in structural fixture",
        rom_len: bytes.len(),
        system: System::Sms,
        region: SegaPsgRegion::Export,
        audio_offset: 0x8000,
        driver_address: 0x4000,
        init_address: 0x4050,
        table: 0x4100,
        song_count: 1,
        frame_divider: 1,
        audio_byte_len: 0x8000,
        header_layout: HeaderLayout::FourByteChannels,
        rejected_selectors: &[],
        supplemental_selectors: &[],
        additional_sources: &[],
    };
    (bytes, profile)
}

#[test]
fn grouped_header_preserves_native_selector_and_bank_addresses() {
    let (bytes, profile) = fixture();
    let song = inspect(&bytes, &profile, 0).unwrap();
    assert_eq!((song.index, song.raw_index), (0, 0x81));
    assert_eq!(song.channels.len(), 4);
    assert_eq!(song.header.byte_len, 22);
    assert_eq!(song.table_entry.effective_offset, 0x8100);
    assert_eq!(song.table_entry.canonical_cpu_address, 0x4100);
    assert_eq!(song.channels[0].transpose, -12);
    assert_eq!(song.channels[3].initial_attenuation, 3);
    assert_eq!(song.mapped_spans[0].byte_len, 0x8000);
    assert_eq!(serde_json::to_value(&song).unwrap()["system"], "sms");
}

#[test]
fn adjacent_effect_pointers_cannot_extend_the_qualified_music_table() {
    let (mut bytes, mut profile) = fixture();
    bytes[0x8102..0x8104].copy_from_slice(&0x4180_u16.to_le_bytes());
    bytes[0x8180..0x818a].copy_from_slice(&[0, 0, 1, 1, 0x88, 0xc0, 0, 0x43, 0, 2]);
    bytes[0x8300..0x8304].copy_from_slice(&[0, 0x50, 3, 0xf2]);
    profile.song_count = 2;
    let effect_as_music = inspect(&bytes, &profile, 1).unwrap();
    profile.song_count = 1;
    assert!(inspect(&bytes, &profile, 1).is_none());
    let cancel = AtomicBool::new(false);
    assert!(prepare_profile(&bytes, &effect_as_music, &profile, &cancel).is_err());
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1000,
    };
    let mut songs = Vec::new();
    scan_profile(&bytes, &profile, &mut songs, &mut budget, 10).unwrap();
    assert_eq!(songs.len(), 1);
    assert_eq!((songs[0].index, songs[0].raw_index), (0, 0x81));
}

#[test]
fn headers_cannot_escape_the_bound_audio_banks() {
    let (mut bytes, profile) = fixture();
    assert!(inspect(&bytes, &profile, 1).is_none());
    bytes[0x8146..0x8148].copy_from_slice(&0xc000_u16.to_le_bytes());
    assert!(inspect(&bytes, &profile, 0).is_none());
    bytes[0x8146..0x8148].copy_from_slice(&0x3fff_u16.to_le_bytes());
    assert!(inspect(&bytes, &profile, 0).is_none());
    bytes[0x8146..0x8148].copy_from_slice(&0x4200_u16.to_le_bytes());
    bytes[0x8142] = 5;
    assert!(inspect(&bytes, &profile, 0).is_none());
    assert!(inspect(&bytes[..0x8144], &profile, 0).is_none());
}

#[test]
fn forged_or_cancelled_selections_are_rejected() {
    let (bytes, profile) = fixture();
    let cancel = AtomicBool::new(false);
    let song = inspect(&bytes, &profile, 0).unwrap();
    let mut forged = song.clone();
    forged.raw_index = 0x82;
    assert!(prepare_profile(&bytes, &forged, &profile, &cancel).is_err());
    forged = song.clone();
    forged.system = System::Gg;
    assert!(prepare_profile(&bytes, &forged, &profile, &cancel).is_err());
    forged = song.clone();
    forged.mapped_spans[0].byte_len -= 1;
    assert!(prepare_profile(&bytes, &forged, &profile, &cancel).is_err());
    cancel.store(true, Ordering::Relaxed);
    assert!(prepare_profile(&bytes, &song, &profile, &cancel).is_err());
}

#[test]
fn stop_only_placeholders_are_not_playable_music() {
    let (mut bytes, profile) = fixture();
    for channel in 0..4 {
        bytes[0x8200 + channel * 0x10] = 0xf2;
    }
    let song = inspect(&bytes, &profile, 0).unwrap();
    assert!(stop_only(&bytes, &song, &profile));
    assert!(prepare_profile(&bytes, &song, &profile, &AtomicBool::new(false)).is_err());
}

#[test]
fn bootstrap_preserves_audio_and_waits_before_the_selector_write() {
    let (bytes, profile) = fixture();
    let song = inspect(&bytes, &profile, 0).unwrap();
    let prepared = prepare_profile(&bytes, &song, &profile, &AtomicBool::new(false)).unwrap();
    assert_eq!(&prepared.bytes[0x8000..], &bytes[0x8000..]);
    assert_eq!(&prepared.bytes[..3], &[0xc3, 0, 1]);
    assert_eq!(prepared.ready_address, 0xc000);
    assert_eq!(prepared.ack_address, 0xc001);
    let wait = prepared.wait_start as usize;
    let end = prepared.wait_end as usize;
    assert_eq!(
        &prepared.bytes[wait..wait + 6],
        &[0x3a, 1, 0xc0, 0xfe, 1, 0xc2]
    );
    assert_eq!(
        &prepared.bytes[wait + 6..end],
        &prepared.wait_start.to_le_bytes()
    );
    assert_eq!(&prepared.bytes[end..end + 5], &[0x3e, 0x81, 0x32, 4, 0xde]);
    assert_eq!(
        &prepared.bytes[0x38..0x40],
        &[0xdb, 0xbf, 0xcd, 0, 0x40, 0xfb, 0xed, 0x4d]
    );
}

#[test]
fn game_gear_bootstrap_preserves_stereo_and_hardware_identity() {
    let (bytes, mut profile) = fixture();
    profile.system = System::Gg;
    profile.region = SegaPsgRegion::Japanese;
    let song = inspect(&bytes, &profile, 0).unwrap();
    let prepared = prepare_profile(&bytes, &song, &profile, &AtomicBool::new(false)).unwrap();
    assert_eq!(prepared.system, System::Gg);
    assert_eq!(prepared.region, SegaPsgRegion::Japanese);
    assert_eq!(prepared.timing, SegaPsgTiming::Ntsc);
    assert!(
        prepared.bytes[0x100..prepared.wait_start as usize]
            .windows(4)
            .any(|w| w == [0x3e, 0xff, 0xd3, 6])
    );
}

#[test]
fn unrecognized_roms_and_wrong_systems_do_not_gain_native_support() {
    let (bytes, _) = fixture();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000_000,
    };
    assert!(
        recognized(&bytes, System::Sms, &mut budget)
            .unwrap()
            .is_none()
    );
    assert!(
        recognized(&bytes, System::Sg, &mut budget)
            .unwrap()
            .is_none()
    );
    cancel.store(true, Ordering::Relaxed);
    assert!(matches!(
        recognized(&bytes, System::Sms, &mut budget),
        Err(ScanStop::Cancelled)
    ));
}

#[test]
fn accepted_synthetic_driver_has_the_same_native_preparation_contract() {
    let bytes = fixture_rom();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000_000,
    };
    let mut songs = Vec::new();
    scan(&bytes, System::Sms, &mut songs, &mut budget, 1).unwrap();
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].channels.len(), 3);
    assert!(prepare_rom(&bytes, &songs[0], &cancel).is_ok());
    let mut changed = bytes;
    changed[0x8000] ^= 1;
    assert!(prepare_rom(&changed, &songs[0], &cancel).is_err());
}

#[test]
fn every_admitted_profile_has_a_representable_separate_bootstrap() {
    for profile in all_profiles() {
        let bytes = vec![0; profile.rom_len];
        let prepared = bootstrap::prepare(&bytes, profile, 0x81)
            .unwrap_or_else(|error| panic!("{}: {error}", profile.name));
        assert!(usize::from(prepared.wait_end) <= profile.audio_offset);
        assert!(profile.audio_offset >= 0x4000);
        assert_eq!(
            &prepared.bytes[profile.audio_offset..],
            &bytes[profile.audio_offset..]
        );
    }
}

#[test]
fn compact_variants_preserve_every_legacy_identity_and_recipe() {
    let mut source_names = Vec::new();
    let mut source_hashes = Vec::new();
    let mut source_count = 0;
    for profile in all_profiles() {
        for source in profile.known_sources() {
            source_count += 1;
            source_names.push(source.name);
            source_hashes.push(source.sha256);
            let recognized =
                profile_by_identity(profile.system.code(), source.rom_len as u64, source.sha256)
                    .expect("every known source remains qualified");
            assert_eq!(recognized.source_name, source.name);
            assert!(std::ptr::eq(recognized.profile, profile));
            assert_eq!(
                recognized.profile.recipe().header_layout,
                profile.header_layout
            );
            assert_eq!(recognized.profile.recipe().first_raw_selector, 0x81);
        }
    }
    source_names.sort_unstable();
    source_names.dedup();
    source_hashes.sort_unstable();
    source_hashes.dedup();
    assert_eq!(source_count, 36);
    assert_eq!(source_names.len(), source_count);
    assert_eq!(source_hashes.len(), source_count);
}

#[test]
fn alternating_frame_driver_keeps_its_native_update_cadence() {
    let (bytes, mut profile) = fixture();
    profile.frame_divider = 2;
    let song = inspect(&bytes, &profile, 0).unwrap();
    let prepared = prepare_profile(&bytes, &song, &profile, &AtomicBool::new(false)).unwrap();
    assert_eq!(song.frame_divider, 2);
    assert_eq!(
        &prepared.bytes[0x3a..0x44],
        &[0x3a, 2, 0xc0, 0xee, 1, 0x32, 2, 0xc0, 0x20, 3]
    );
    assert_eq!(&prepared.bytes[0x44..0x47], &[0xcd, 0, 0x40]);
}

#[test]
fn skipped_placeholders_do_not_consume_candidate_capacity() {
    let (mut bytes, mut profile) = fixture();
    profile.song_count = 3;
    for (entry, header, sequence) in [(0x8102, 0x4180_u16, 0x4300_u16), (0x8104, 0x41a0, 0x4320)] {
        bytes[entry..entry + 2].copy_from_slice(&header.to_le_bytes());
        let at = usize::from(header) + 0x4000;
        bytes[at..at + 6].copy_from_slice(&[0, 0, 1, 0, 1, 3]);
        bytes[at + 6..at + 8].copy_from_slice(&sequence.to_le_bytes());
    }
    bytes[0x8300] = 0xf2;
    bytes[0x8320] = 0x81;
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1000,
    };
    let mut songs = Vec::new();
    scan_profile(&bytes, &profile, &mut songs, &mut budget, 2).unwrap();
    assert_eq!(
        songs
            .iter()
            .map(|song| (song.index, song.raw_index))
            .collect::<Vec<_>>(),
        vec![(0, 0x81), (2, 0x83)]
    );
    songs.clear();
    assert_eq!(
        scan_profile(&bytes, &profile, &mut songs, &mut budget, 1),
        Err(ScanStop::CandidateLimit)
    );
    assert_eq!(songs.len(), 1);
}

#[test]
fn resident_bank_profiles_do_not_claim_an_unrelated_second_slot() {
    let (mut bytes, mut profile) = fixture();
    profile.audio_offset = 0x4000;
    profile.audio_byte_len = 0x4000;
    bytes.copy_within(0x8000..0xc000, 0x4000);
    let song = inspect(&bytes, &profile, 0).unwrap();
    assert_eq!(song.mapped_spans[0].effective_offset, 0x4000);
    assert_eq!(song.mapped_spans[0].byte_len, 0x4000);
    assert!(prepare_profile(&bytes, &song, &profile, &AtomicBool::new(false)).is_ok());
    bytes[0x4146..0x4148].copy_from_slice(&0x8000_u16.to_le_bytes());
    assert!(inspect(&bytes, &profile, 0).is_none());
}

#[test]
fn source_mapping_requires_the_exact_identity_and_full_bounded_span() {
    let mut media = MediaIdentity {
        system: "sms",
        byte_len: fixture::PROFILE.rom_len as u64,
        sha256: Some(fixture::PROFILE.sha256.into()),
    };
    let span = SourceSpan {
        effective_offset: 0x8000,
        byte_len: 0x8000,
        canonical_cpu_address: Some(0x4000),
    };
    assert!(source_span_matches(&media, span));
    for changed in [
        SourceSpan {
            byte_len: 0,
            ..span
        },
        SourceSpan {
            byte_len: 0x8001,
            ..span
        },
        SourceSpan {
            effective_offset: 0x8001,
            ..span
        },
        SourceSpan {
            canonical_cpu_address: Some(0x3fff),
            ..span
        },
        SourceSpan {
            canonical_cpu_address: None,
            ..span
        },
    ] {
        assert!(!source_span_matches(&media, changed));
    }
    media.system = "gg";
    assert!(!source_span_matches(&media, span));
    media.system = "sms";
    media.byte_len += 1;
    assert!(!source_span_matches(&media, span));
    media.byte_len -= 1;
    media.sha256 = None;
    assert!(!source_span_matches(&media, span));
}

#[test]
fn absent_or_conflicting_headers_do_not_silently_choose_hardware() {
    let (mut bytes, mut profile) = fixture();
    profile.system = System::Gg;
    assert!(
        warnings(&bytes, &profile)
            .iter()
            .any(|warning| warning.contains("no recognized"))
    );
    bytes[0x7ff0..0x7ff8].copy_from_slice(b"TMR SEGA");
    bytes[0x7fff] = 0x4e;
    assert!(
        warnings(&bytes, &profile)
            .iter()
            .any(|warning| warning.contains("identifies sms"))
    );
    bytes[0x7fff] = 0x7e;
    assert_eq!(warnings(&bytes, &profile).len(), 1);
}

#[test]
fn six_byte_fixture_keeps_all_channel_bytes_and_exact_identity() {
    let bytes = fixture_rom_six_byte();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000_000,
    };
    let mut songs = Vec::new();
    scan(&bytes, System::Gg, &mut songs, &mut budget, 1).unwrap();
    assert_eq!(songs.len(), 1);
    let song = &songs[0];
    assert_eq!(song.header.byte_len, 24);
    for (number, channel) in song.channels.iter().enumerate() {
        assert_eq!(channel.entry.byte_len, 6);
        assert_eq!(channel.entry.effective_offset as usize, 0x8146 + number * 6);
        assert_eq!(channel.cpu_address as usize, 0x4200 + number * 0x10);
        assert_eq!(channel.transpose, 12 + number as i8);
        assert_eq!(channel.initial_attenuation, 3 + number as u8);
        let at = channel.entry.effective_offset as usize;
        assert_eq!(
            &bytes[at + 4..at + 6],
            &[0x80 + number as u8, 0x90 + number as u8]
        );
    }
    let prepared = prepare_rom(&bytes, song, &cancel).unwrap();
    assert_eq!(&prepared.bytes[0x8000..], &bytes[0x8000..]);
    assert_eq!(prepared.system, System::Gg);
    let mut changed = bytes.clone();
    changed[0x814a] ^= 1;
    assert!(prepare_rom(&changed, song, &cancel).is_err());
    songs.clear();
    scan(&bytes, System::Sms, &mut songs, &mut budget, 1).unwrap();
    assert!(songs.is_empty());
}

#[test]
fn six_byte_headers_reject_non_psg_channels_and_full_entry_bank_escapes() {
    let mut bytes = fixture_rom_six_byte();
    let profile = fixture::SIX_BYTE_PROFILE;
    bytes[0x8142] = 1;
    assert!(inspect(&bytes, &profile, 0).is_none());
    bytes[0x8142] = 0;
    bytes[0x8143] = 5;
    assert!(inspect(&bytes, &profile, 0).is_none());
    bytes[0x8143] = 3;
    bytes.copy_within(0x8140..0x8156, 0xffea);
    bytes[0x8100..0x8102].copy_from_slice(&0xbfea_u16.to_le_bytes());
    assert!(inspect(&bytes, &profile, 0).is_none());
}

#[test]
fn unsupported_interior_selector_keeps_later_music_and_reports_partial() {
    let mut bytes = fixture_rom_six_byte();
    let mut profile = fixture::SIX_BYTE_PROFILE;
    profile.song_count = 3;
    for at in [0x8102, 0x8104] {
        bytes[at..at + 2].copy_from_slice(&0x4140_u16.to_le_bytes());
    }
    let rejected_handle = inspect(&bytes, &profile, 1).unwrap();
    profile.rejected_selectors = &[profiles::RejectedSelector {
        raw_index: 0x82,
        reason: "Synthetic unqualified envelope lookup.",
    }];
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 10,
    };
    let mut songs = Vec::new();
    assert_eq!(
        scan_profile(&bytes, &profile, &mut songs, &mut budget, 2),
        Err(ScanStop::ValidationLimit)
    );
    assert_eq!(budget.remaining, 7);
    assert_eq!(
        songs
            .iter()
            .map(|s| (s.index, s.raw_index))
            .collect::<Vec<_>>(),
        vec![(0, 0x81), (2, 0x83)]
    );
    assert!(songs.iter().all(|song| song.warnings.iter().any(|warning| {
        warning.contains("selector 0x82") && warning.contains("unqualified envelope")
    })));
    assert!(prepare_profile(&bytes, &songs[1], &profile, &cancel).is_ok());
    assert!(prepare_profile(&bytes, &rejected_handle, &profile, &cancel).is_err());
    assert!(inspect(&bytes, &profile, 1).is_none());
    songs.clear();
    assert_eq!(
        scan_profile(&bytes, &profile, &mut songs, &mut budget, 1),
        Err(ScanStop::CandidateLimit)
    );
    assert_eq!(songs.len(), 1);
    songs.clear();
    budget.remaining = 1;
    assert_eq!(
        scan_profile(&bytes, &profile, &mut songs, &mut budget, 2),
        Err(ScanStop::WorkLimit)
    );
    assert_eq!(songs.len(), 1);
    songs.clear();
    cancel.store(true, Ordering::Relaxed);
    assert_eq!(
        scan_profile(&bytes, &profile, &mut songs, &mut budget, 2),
        Err(ScanStop::Cancelled)
    );
    assert!(songs.is_empty());
}

#[test]
fn setup_only_stop_requires_every_channel_to_have_no_note() {
    let mut bytes = fixture_rom_six_byte();
    let profile = fixture::SIX_BYTE_PROFILE;
    for channel in 0..3 {
        let at = 0x8200 + channel * 0x10;
        bytes[at..at + 12].copy_from_slice(&[0xf0, 7, 2, 1, 4, 0xe0, 0xff, 0, 0, 0, 1, 0xf2]);
    }
    let song = inspect(&bytes, &profile, 0).unwrap();
    assert!(stop_only(&bytes, &song, &profile));
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 10,
    };
    let mut songs = Vec::new();
    scan_profile(&bytes, &profile, &mut songs, &mut budget, 1).unwrap();
    assert!(songs.is_empty());
    assert!(prepare_profile(&bytes, &song, &profile, &cancel).is_err());
    bytes[0x822b..0x822e].copy_from_slice(&[0x81, 2, 0xf2]);
    assert!(!stop_only(&bytes, &song, &profile));
    scan_profile(&bytes, &profile, &mut songs, &mut budget, 1).unwrap();
    assert_eq!(songs.len(), 1);
    assert!(prepare_profile(&bytes, &songs[0], &profile, &cancel).is_ok());
}

#[test]
fn setup_stop_filter_is_layout_specific_bounded_and_argument_aware() {
    let mut bytes = fixture_rom_six_byte();
    let mut profile = fixture::SIX_BYTE_PROFILE;
    bytes[0x8143] = 1;
    let song = inspect(&bytes, &profile, 0).unwrap();
    bytes[0x8200..0x8207].copy_from_slice(&[0xe0, 0xf2, 0, 0, 0, 0, 0x81]);
    assert!(!stop_only(&bytes, &song, &profile));
    bytes[0x8206] = 0xf2;
    assert!(stop_only(&bytes, &song, &profile));
    profile.header_layout = HeaderLayout::FourByteChannels;
    assert!(!stop_only(&bytes, &song, &profile));
    profile.header_layout = HeaderLayout::SixByteChannels;
    for command in 0..9 {
        let at = 0x8200 + command * 5;
        bytes[at..at + 5].copy_from_slice(&[0xf0, 0, 0, 0, 0]);
    }
    bytes[0x822d] = 0xf2;
    assert!(!stop_only(&bytes, &song, &profile));
    bytes[0x8228] = 0xf2;
    assert!(stop_only(&bytes, &song, &profile));
    bytes[0x8146..0x8148].copy_from_slice(&0xbffa_u16.to_le_bytes());
    bytes[0xfffa..].copy_from_slice(&[0xe0, 0, 0, 0, 0, 0]);
    bytes.push(0xf2);
    let end_song = inspect(&bytes, &profile, 0).unwrap();
    assert!(!stop_only(&bytes, &end_song, &profile));
}
