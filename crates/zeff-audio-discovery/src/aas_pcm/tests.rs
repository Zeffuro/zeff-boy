use super::*;

fn budget(cancel: &AtomicBool) -> Budget<'_> {
    Budget {
        cancel,
        remaining: 1_000_000,
    }
}

fn set_word(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn authored_pcm_cues_preserve_samples_guards_loops_and_selector_channels() {
    let bytes = fixture_rom();
    assert_eq!(zeff_firmware::sha256_hex(&bytes), fixture::PROFILE.sha256);
    let cancel = AtomicBool::new(false);
    let mut songs = Vec::new();
    scan(&bytes, &mut songs, &mut budget(&cancel), 2).unwrap();
    assert_eq!(songs.len(), 2);
    for (index, song) in songs.iter().enumerate() {
        let sample = 0x910 + index * 24;
        assert_eq!(song.index as usize, index);
        assert_eq!(song.root, RomSpan::new(0x800, 24));
        assert_eq!(song.header, RomSpan::new(0x800 + index * 12, 12));
        assert_eq!(song.sample_rate, 10_400);
        assert_eq!(song.channels, 1);
        assert_eq!(song.sample_data, RomSpan::new(sample, 8));
        assert_eq!(song.sample_leadin, RomSpan::new(sample - 16, 16));
        assert_eq!(song.sample_padding, RomSpan::new(sample + 8, 16));
        assert_eq!(song.loop_start, (index == 0).then_some(0));
        assert_eq!(song.native.playback_channel, if index == 0 { 3 } else { 0 });
        assert_eq!(song.native.volume, 64);
        assert!(
            song.mapped_spans
                .iter()
                .all(|&span| !intersects(span, song.native.handoff))
        );
    }
}

#[test]
fn pcm_parser_rejects_invalid_relative_extents_loops_and_guards() {
    let bytes = fixture_rom();
    assert!(inspect(&bytes, &fixture::PROFILE, 2).is_none());
    assert!(inspect(&bytes[..bytes.len() - 1], &fixture::PROFILE, 0).is_none());
    for (at, value) in [
        (0x800, 0),
        (0x800, 15),
        (0x800, u32::MAX),
        (0x804, 16),
        (0x804, u32::MAX),
        (0x808, 1),
        (0x808, 24),
        (0x808, u32::MAX),
    ] {
        let mut changed = bytes.clone();
        set_word(&mut changed, at, value);
        assert!(inspect(&changed, &fixture::PROFILE, 0).is_none());
    }
    let mut changed = bytes.clone();
    set_word(
        &mut changed,
        0x804,
        (bytes.len() - fixture::PROFILE.bank) as u32,
    );
    assert!(inspect(&changed, &fixture::PROFILE, 0).is_none());
}

#[test]
fn interior_loop_offsets_are_relative_to_the_logical_sample() {
    let mut bytes = fixture_rom();
    set_word(&mut bytes, 0x808, 19);
    let song = inspect(&bytes, &fixture::PROFILE, 0).unwrap();
    assert_eq!(song.loop_start, Some(3));
    assert_eq!(song.sample_data, RomSpan::new(0x910, 8));
    let prepared = bootstrap::build(&bytes, &song).unwrap();
    assert!(
        prepared.bytes[bytes.len()..]
            .as_chunks::<4>()
            .0
            .contains(&19_u32.to_le_bytes())
    );
}

#[test]
fn preparation_changes_only_the_startup_handoff_and_appended_bootstrap() {
    let bytes = fixture_rom();
    for index in 0..2 {
        let song = inspect(&bytes, &fixture::PROFILE, index).unwrap();
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
}

#[test]
fn exact_source_and_every_retained_pcm_contract_are_revalidated() {
    let bytes = fixture_rom();
    let song = inspect(&bytes, &fixture::PROFILE, 0).unwrap();
    let cancel = AtomicBool::new(false);
    for mutate in [
        |song: &mut AasPcmSong| song.index = 1,
        |song: &mut AasPcmSong| song.root.byte_len += 12,
        |song: &mut AasPcmSong| song.header.effective_offset += 12,
        |song: &mut AasPcmSong| song.title.clear(),
        |song: &mut AasPcmSong| song.channels = 2,
        |song: &mut AasPcmSong| song.sample_rate += 1,
        |song: &mut AasPcmSong| song.sample_data.effective_offset += 4,
        |song: &mut AasPcmSong| song.sample_leadin.byte_len = 0,
        |song: &mut AasPcmSong| song.sample_padding.byte_len = 0,
        |song: &mut AasPcmSong| song.loop_start = None,
        |song: &mut AasPcmSong| song.native.vblank_slot_address += 4,
        |song: &mut AasPcmSong| song.native.sample_bank_address += 4,
        |song: &mut AasPcmSong| song.native.play.canonical_cpu_address += 2,
        |song: &mut AasPcmSong| song.native.playback_channel = 0,
        |song: &mut AasPcmSong| song.native.volume -= 1,
        |song: &mut AasPcmSong| song.mapped_spans.clear(),
        |song: &mut AasPcmSong| song.warnings.clear(),
    ] {
        let mut changed = song.clone();
        mutate(&mut changed);
        assert!(prepare_rom(&bytes, &changed, &cancel).is_err());
    }
    let mut changed = bytes.clone();
    changed[0x100] ^= 1;
    assert!(prepare_rom(&changed, &song, &cancel).is_err());
    let mut songs = Vec::new();
    scan(&changed, &mut songs, &mut budget(&cancel), 2).unwrap();
    assert!(songs.is_empty());
    assert!(prepare_rom(&bytes, &song, &AtomicBool::new(true)).is_err());
}

#[test]
fn bounded_scan_retains_only_complete_cues_and_honors_cancellation() {
    let bytes = fixture_rom();
    let cancel = AtomicBool::new(false);
    let mut songs = Vec::new();
    assert_eq!(
        scan(&bytes, &mut songs, &mut budget(&cancel), 1),
        Err(ScanStop::CandidateLimit)
    );
    assert_eq!(songs.len(), 1);
    let mut limited = Budget {
        cancel: &cancel,
        remaining: 2,
    };
    assert_eq!(
        scan(&bytes, &mut Vec::new(), &mut limited, 2),
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
}

#[test]
fn cartridge_dispatch_retains_cues_with_source_bound_asset_graphs() {
    let bytes = fixture_rom();
    let cancel = AtomicBool::new(false);
    let report = crate::scan(
        zeff_emu_common::system::System::Gba,
        &bytes,
        crate::ScanLimits::default(),
        &cancel,
    );
    assert_eq!(report.aas_pcm_songs.len(), 2);
    assert!(report.aas_songs.is_empty());
    assert!(report.aas_stream_songs.is_empty());
    assert_eq!(report.song_count(), 2);
    assert_eq!(
        report
            .song(crate::catalog::SongId::AasPcm(1))
            .unwrap()
            .detector_id(),
        "gba-aas-pcm-driver"
    );
    assert_eq!(report.status, crate::ScanStatus::Complete);
    let graph = report.asset_relations(
        crate::catalog::SongId::AasPcm(1),
        crate::relations::GraphLimits::default(),
        &cancel,
    );
    assert_eq!(graph.status, crate::relations::GraphStatus::Complete);
    assert!(
        graph
            .nodes
            .iter()
            .any(|node| node.kind == crate::relations::AssetKind::SampleData)
    );
}
