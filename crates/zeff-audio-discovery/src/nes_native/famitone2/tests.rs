use super::*;

fn songs(bytes: &[u8]) -> Vec<NesNativeSong> {
    inspect(
        bytes,
        &mut Budget {
            cancel: &AtomicBool::new(false),
            remaining: 100_000,
        },
    )
    .unwrap()
}

#[test]
fn four_channel_sources_keep_a_separate_verified_profile() {
    for (relocated, hash) in [
        (
            false,
            "fb0475d3a0aa5042ebd62150109a97d71cb52488e56476c635c5cac7d80393ed",
        ),
        (
            true,
            "4b7cb3c6e32e1fd624f2698aa398bd18d9cf8c836c1e075df524a970bcdfe530",
        ),
    ] {
        let bytes = four_channel_rom(relocated);
        assert_eq!(zeff_firmware::sha256_hex(&bytes), hash);
        let found = songs(&bytes);
        assert_eq!(found.len(), 2);
        for mut song in found {
            assert_eq!(song.profile, FOUR_CHANNEL_PROFILE);
            let prepared = prepare(&bytes, &song, &AtomicBool::new(false)).unwrap();
            assert_eq!(&prepared.bytes[..0x7810], &bytes[..0x7810]);
            song.profile = PROFILE;
            assert!(prepare(&bytes, &song, &AtomicBool::new(false)).is_err());
        }
    }
}

#[test]
fn relocated_fixtures_preserve_the_qualified_bootstrap_and_source() {
    for (relocated, hash, native_hash) in [
        (
            false,
            "0466ef1f83eafde5731969eed5ee27f74d13471d739256d3cb365a959d6b1bf8",
            "524c75d4ff756328498a9fdbd812706415dfd73e7c348f73eaec5aa1df4e1f38",
        ),
        (
            true,
            "550c70df2535ffa625adcbddefd4d122b2c8e86594fff34a69d67eba9c2928de",
            "c87e4c44f5197fca8357e00b776938d4411223cff8e4d52994ed3bbb68ab99a0",
        ),
    ] {
        let bytes = rom(relocated);
        assert_eq!(zeff_firmware::sha256_hex(&bytes), hash);
        let songs = songs(&bytes);
        assert_eq!(songs.len(), 2);
        for song in songs {
            let prepared = prepare(&bytes, &song, &AtomicBool::new(false)).unwrap();
            assert_eq!(prepared.wait_start, 0xf834);
            assert_eq!(&prepared.bytes[..0x7810], &bytes[..0x7810]);
            assert_eq!(&prepared.bytes[0x786e..0x800a], &bytes[0x786e..0x800a]);
            assert_eq!(&prepared.bytes[0x8010..], &bytes[0x8010..]);
            if song.index == 0 {
                assert_eq!(zeff_firmware::sha256_hex(&prepared.bytes), native_hash);
            }
        }
    }
}

#[test]
fn stale_selection_and_forged_metadata_cannot_prepare() {
    let bytes = rom(false);
    let mut song = songs(&bytes).remove(0);
    let mut changed = bytes.clone();
    changed[0x8010] = 1;
    assert_eq!(songs(&changed).len(), 2);
    assert!(prepare(&changed, &song, &AtomicBool::new(false)).is_err());
    song.raw_index = 1;
    assert!(prepare(&bytes, &song, &AtomicBool::new(false)).is_err());
    assert!(prepare(&bytes, &songs(&bytes)[0], &AtomicBool::new(true)).is_err());
}

#[test]
fn executable_caller_and_mapping_changes_are_refused() {
    let bytes = rom(false);
    for offset in [
        6,
        7,
        16,
        16 + 0x45,
        16 + 0x42,
        16 + 0x52,
        0x800a,
        0x7810,
        0x66,
        0x66 + 0x10,
        0x66 + 0x7a,
        0x66 + 0x300,
    ] {
        let mut changed = bytes.clone();
        changed[offset] ^= 0x40;
        assert!(songs(&changed).is_empty(), "offset {offset:x}");
    }
}

#[test]
fn source_mapping_requires_fresh_identity_and_recognized_spans() {
    let bytes = rom(false);
    let report = crate::scan(
        zeff_emu_common::system::System::Nes,
        &bytes,
        Default::default(),
        &AtomicBool::new(false),
    );
    let song = &report.nes_native_songs[0];
    assert!(source_span_matches(
        &bytes,
        &report.media,
        song.table_entry.into()
    ));
    let mut media = report.media.clone();
    media.sha256 = Some("00".repeat(32));
    assert!(!source_span_matches(
        &bytes,
        &media,
        song.table_entry.into()
    ));
    assert!(!source_span_matches(
        &bytes,
        &report.media,
        SourceSpan {
            effective_offset: 0x7810,
            byte_len: 1,
            canonical_cpu_address: Some(0xf800)
        }
    ));
}

#[test]
fn retained_graph_is_complete_and_rejects_mismatched_source_identity() {
    let bytes = rom(false);
    let mut report = crate::scan(
        zeff_emu_common::system::System::Nes,
        &bytes,
        Default::default(),
        &AtomicBool::new(false),
    );
    for index in 0..2 {
        let graph = report.asset_relations(
            crate::catalog::SongId::NesNative(index),
            Default::default(),
            &AtomicBool::new(false),
        );
        assert_eq!(graph.status, crate::relations::GraphStatus::Complete);
        assert!(!graph.nodes.is_empty());
    }
    report.nes_native_songs[0].source_sha256 = Some("00".repeat(32));
    let graph = report.asset_relations(
        crate::catalog::SongId::NesNative(0),
        Default::default(),
        &AtomicBool::new(false),
    );
    assert_eq!(
        graph.status,
        crate::relations::GraphStatus::Incomplete(crate::relations::GraphStop::InvalidLocation)
    );
}
