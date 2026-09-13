use std::sync::atomic::AtomicBool;

use super::*;
use crate::test_support::gax::{fixture, half, integer};

fn scanned(bytes: &[u8]) -> Vec<GaxSong> {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    let mut songs = Vec::new();
    scan(bytes, &mut songs, &mut budget, 16).unwrap();
    songs
}

#[test]
fn validates_full_graph_and_preserves_source_spans() {
    let bytes = fixture();
    let songs = scanned(&bytes);
    assert_eq!(songs.len(), 1);
    let song = &songs[0];
    assert_eq!(song.header, RomSpan::new(0x800, 200));
    assert_eq!((&*song.title, &*song.artist), ("Fixture", "Tests"));
    assert_eq!(
        (
            song.channels.len(),
            song.patterns.len(),
            song.instruments.len(),
            song.samples.len()
        ),
        (2, 1, 1, 1)
    );
    assert_eq!(song.patterns[0].source, RomSpan::new(0x500, 10));
    assert_eq!(song.samples[0].data, RomSpan::new(0x1000, 16));
    assert_eq!(song.instruments[0].sample_settings[0].loop_end, 16);
    assert!(song.xm_exportable, "{:?}", song.warnings);
    for span in &song.mapped_spans {
        assert!(
            bytes
                .get(
                    span.effective_offset as usize
                        ..(span.effective_offset + span.byte_len) as usize
                )
                .is_some()
        );
    }
}

#[test]
fn projection_rejects_public_song_data_with_an_out_of_range_sample_slot() {
    let bytes = fixture();
    let mut song = scanned(&bytes).remove(0);
    let setting = song.instruments[0].sample_settings[0];
    song.instruments[0].sample_settings.resize(5, setting);
    song.instruments[0].rows[0].sample_slot = 5;
    assert!(project(&bytes, &song, &AtomicBool::new(false)).is_err());
}

#[test]
fn equal_sample_loop_endpoints_preserve_pcm_without_enabling_xm_loops() {
    for endpoint in [0, 8, 16] {
        for bidirectional in [0, 1] {
            let mut bytes = fixture();
            integer(&mut bytes, 0x320, endpoint);
            integer(&mut bytes, 0x324, endpoint);
            bytes[0x31B] = bidirectional;
            let song = scanned(&bytes).remove(0);
            let setting = &song.instruments[0].sample_settings[0];
            assert_eq!((setting.loop_start, setting.loop_end), (endpoint, endpoint));
            assert_eq!(setting.bidirectional, bidirectional != 0);
            assert!(song.xm_exportable, "{:?}", song.warnings);
            let cancel = AtomicBool::new(false);
            let module = project(&bytes, &song, &cancel).unwrap();
            let sample = &module.instruments[0].samples[0];
            assert_eq!(sample.loop_range, None);
            assert!(!sample.ping_pong);
            let expected: Vec<_> = bytes[0x1000..0x1010]
                .iter()
                .map(|&byte| (i16::from(byte) - 128) * 256)
                .collect();
            assert_eq!(sample.pcm, expected);
            let xm = crate::tracker::xm::encode(&module, &cancel).unwrap();
            let mut at = 60 + word(&xm, 60).unwrap() as usize;
            for _ in &module.patterns {
                at += word(&xm, at).unwrap() as usize + usize::from(u16_at(&xm, at + 7).unwrap());
            }
            let sample_header = at + word(&xm, at).unwrap() as usize;
            assert_eq!(word(&xm, sample_header), Some(16));
            assert_eq!(word(&xm, sample_header + 4), Some(0));
            assert_eq!(word(&xm, sample_header + 8), Some(0));
            assert_eq!(xm[sample_header + 14] & 3, 0);
        }
    }
}

#[test]
fn invalid_sample_loop_bounds_remain_rejected_by_scan_and_public_projection() {
    for (start, end) in [(16, 8), (17, 17), (0, 17), (u32::MAX, u32::MAX)] {
        let mut bytes = fixture();
        integer(&mut bytes, 0x320, start);
        integer(&mut bytes, 0x324, end);
        assert!(scanned(&bytes).is_empty());
        let original = fixture();
        let mut song = scanned(&original).remove(0);
        let setting = &mut song.instruments[0].sample_settings[0];
        setting.loop_start = start;
        setting.loop_end = end;
        assert!(project(&original, &song, &AtomicBool::new(false)).is_err());
    }
}

#[test]
fn mixing_amplification_is_retained_without_enabling_unsupported_xm_levels() {
    for volume in [0, 256, 257, 512, 724, u16::MAX] {
        let mut bytes = fixture();
        half(&mut bytes, 0x808, volume);
        let songs = scanned(&bytes);
        assert_eq!(songs.len(), 1);
        let song = &songs[0];
        assert_eq!(song.volume, volume);
        assert_eq!(song.xm_exportable, volume <= 256);
        assert_eq!(
            song.warnings.iter().any(|warning| warning.offset == 0x808
                && warning.reason.contains("mixing amplification")),
            volume > 256
        );
        #[cfg(not(target_arch = "wasm32"))]
        assert_eq!(
            project(&bytes, song, &AtomicBool::new(false)).is_ok(),
            volume <= 256
        );
    }
}

#[test]
fn version_marker_and_partial_graph_cannot_create_a_candidate() {
    let original = fixture();
    let mut bytes = original.clone();
    bytes[0x40 + 17] = b'2';
    assert!(scanned(&bytes).is_empty());
    bytes = original.clone();
    bytes[0x40] = b'X';
    assert!(scanned(&bytes).is_empty());
    bytes = original.clone();
    integer(&mut bytes, 0x234, u32::MAX);
    assert!(scanned(&bytes).is_empty());
    bytes = original.clone();
    integer(&mut bytes, 0x324, 17);
    assert!(scanned(&bytes).is_empty());
    bytes = original.clone();
    integer(&mut bytes, 0x828, 0x0800_0200);
    assert!(scanned(&bytes).is_empty());
    bytes = original.clone();
    bytes[0x501..0x503].copy_from_slice(&[0xFF, 0]);
    assert!(scanned(&bytes).is_empty());
    bytes = original;
    bytes[0x501..0x503].copy_from_slice(&[0xFF, 5]);
    assert!(scanned(&bytes).is_empty());
}

#[test]
fn inventory_keeps_runtime_effects_but_refuses_xm() {
    let mut bytes = fixture();
    bytes[0x504] = 7;
    bytes[0x505] = 0x34;
    let songs = scanned(&bytes);
    assert_eq!(songs.len(), 1);
    assert!(!songs[0].xm_exportable);
    assert!(
        songs[0]
            .warnings
            .iter()
            .any(|w| w.reason.contains("pattern effect"))
    );
    #[cfg(not(target_arch = "wasm32"))]
    assert!(project(&bytes, &songs[0], &AtomicBool::new(false)).is_err());
    bytes = fixture();
    bytes[0x311] = 2;
    bytes[0x488..0x490].copy_from_slice(&[2, 0, 1, 0, 0, 0, 0, 0]);
    let songs = scanned(&bytes);
    assert_eq!(songs.len(), 1);
    assert!(!songs[0].xm_exportable);
}

#[test]
fn supports_all_compressed_effect_markers_and_unterminated_title() {
    for command in 0xFA..=0xFE {
        let mut bytes = fixture();
        bytes[0x503] = command;
        assert_eq!(scanned(&bytes)[0].patterns[0].rows[1].effect, 12);
    }
    let mut bytes = fixture();
    let table = (word(&bytes, 0x820).unwrap() - 0x0800_0000) as usize;
    bytes[table - 1] = b'!';
    assert_eq!(scanned(&bytes)[0].artist, "Tests!");
}

#[test]
fn respects_cancellation_work_and_candidate_limits_with_partial_results() {
    let mut bytes = fixture();
    bytes.copy_within(0x800..0x800 + HEADER_LEN, 0xA00);
    let cancelled = AtomicBool::new(true);
    let mut songs = Vec::new();
    assert_eq!(
        scan(
            &bytes,
            &mut songs,
            &mut Budget {
                cancel: &cancelled,
                remaining: 100_000
            },
            16
        ),
        Err(ScanStop::Cancelled)
    );
    let cancel = AtomicBool::new(false);
    assert_eq!(
        scan(
            &bytes,
            &mut songs,
            &mut Budget {
                cancel: &cancel,
                remaining: 1
            },
            16
        ),
        Err(ScanStop::WorkLimit)
    );
    assert!(songs.is_empty());
    assert_eq!(
        scan(
            &bytes,
            &mut songs,
            &mut Budget {
                cancel: &cancel,
                remaining: 100_000
            },
            1
        ),
        Err(ScanStop::CandidateLimit)
    );
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].header.effective_offset, 0x800);
}

#[test]
fn retained_allocation_limit_counts_nested_graph_and_preserves_earlier_songs() {
    let mut bytes = fixture();
    bytes.copy_within(0x800..0x800 + HEADER_LEN, 0xA00);
    let cancel = AtomicBool::new(false);
    let scan_at_limit = |retained_limit| {
        let mut songs = Vec::new();
        let result = scan_with_inventory_limit(
            &bytes,
            &mut songs,
            &mut Budget {
                cancel: &cancel,
                remaining: 100_000,
            },
            2,
            retained_limit,
        );
        (result, songs)
    };

    let (result, complete) = scan_at_limit(usize::MAX);
    assert_eq!(result, Ok(()));
    assert_eq!(complete.len(), 2);
    let exact_limit = retained_owned_bytes(&complete);
    let song = &complete[0];
    let nested_bytes = song
        .channels
        .iter()
        .map(|channel| capacity_bytes(&channel.orders))
        .chain(
            song.patterns
                .iter()
                .map(|pattern| capacity_bytes(&pattern.rows)),
        )
        .chain(song.instruments.iter().flat_map(|instrument| {
            [
                capacity_bytes(&instrument.rows),
                capacity_bytes(&instrument.sample_settings),
                capacity_bytes(&instrument.envelope.points),
            ]
        }))
        .fold(0usize, usize::saturating_add);
    assert!(nested_bytes > 0);

    let (result, boundary) = scan_at_limit(exact_limit);
    assert_eq!(result, Ok(()));
    assert_eq!(boundary.len(), 2);
    assert_eq!(retained_owned_bytes(&boundary), exact_limit);

    let (result, partial) = scan_at_limit(exact_limit - 1);
    assert_eq!(result, Err(ScanStop::InventoryLimit));
    assert_eq!(partial.len(), 1);
    assert_eq!(partial[0].header.effective_offset, 0x800);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn projects_unsigned_pcm_exact_loop_pitch_and_channel_transpose() {
    let bytes = fixture();
    let songs = scanned(&bytes);
    let cancel = AtomicBool::new(false);
    let module = project(&bytes, &songs[0], &cancel).unwrap();
    assert_eq!(
        (module.channels, module.restart, module.speed, module.bpm),
        (2, 1, 6, 149)
    );
    assert_eq!(module.orders, [0, 1]);
    assert_eq!(module.patterns[0].cells[0].note, 49);
    assert_eq!(module.patterns[0].cells[1].note, 61);
    assert_eq!(module.patterns[0].cells[4].instrument, 1);
    assert_eq!(module.patterns[0].cells[4].note, 53);
    assert_eq!(module.patterns[0].cells[6].note, 97);
    let sample = &module.instruments[0].samples[0];
    assert_eq!(sample.loop_range, Some((2, 16)));
    assert_eq!((sample.relative_note, sample.finetune), (0, 0));
    assert_eq!(sample.pcm[0], -32768);
    assert_eq!(sample.pcm[6], 0);
    assert_eq!(sample.pcm[14], 32512);
    let xm = super::super::tracker::xm::encode(&module, &cancel).unwrap();
    assert!(xm.starts_with(b"Extended Module: "));
    assert!(project(&bytes, &songs[0], &AtomicBool::new(true)).is_err());
}

#[test]
fn retained_phase_pitch_changes_are_inventoried_without_false_retrigger_export() {
    let mut bytes = fixture();
    bytes[0x507] = 0;
    let songs = scanned(&bytes);
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].patterns[0].rows[2].instrument, Some(0));
    assert!(!songs[0].xm_exportable);
    assert!(
        songs[0]
            .warnings
            .iter()
            .any(|w| w.reason.contains("sample phase"))
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn preserves_fixed_pitch_negative_finetune_and_ping_pong_loop() {
    let mut bytes = fixture();
    bytes[0x480] = 49;
    bytes[0x481] = 1;
    bytes[0x31B] = 1;
    half(&mut bytes, 0x318, (-1i16) as u16);
    let songs = scanned(&bytes);
    let module = project(&bytes, &songs[0], &AtomicBool::new(false)).unwrap();
    assert_eq!(module.patterns[0].cells[0].note, 49);
    assert_eq!(module.patterns[0].cells[1].note, 49);
    let sample = &module.instruments[0].samples[0];
    assert_eq!((sample.relative_note, sample.finetune), (-1, -4));
    assert_eq!(sample.loop_range, Some((2, 16)));
    assert!(sample.ping_pong);
}
