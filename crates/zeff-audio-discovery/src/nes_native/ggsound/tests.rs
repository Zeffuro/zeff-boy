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
fn compiled_cues_preserve_the_verified_native_images_and_mapping() {
    for (relocated, source_hash, native_hash) in [
        (
            false,
            "3b64e371dfb0180c1dd23f73d89af865dbac18c166f5b7f2e46ee64984576fdb",
            "aebfcfed27e589573a2ed8d5c1ea43b97231a9bdd482663aab2d048673320718",
        ),
        (
            true,
            "e16572952a292fd6121f7caf86d1cc86b01a55e8ef9025aec14012a3eb33ebdf",
            "fef68b686dabdb132c4c5780d581262194f2ddbe310c5bbdb43a45f918069b75",
        ),
    ] {
        let bytes = rom(relocated);
        assert_eq!(zeff_firmware::sha256_hex(&bytes), source_hash);
        let report = crate::scan(
            zeff_emu_common::system::System::Nes,
            &bytes,
            Default::default(),
            &AtomicBool::new(false),
        );
        assert_eq!(report.nes_native_songs.len(), 2);
        for song in &report.nes_native_songs {
            assert_eq!(song.profile, PROFILE);
            let prepared = prepare(&bytes, song, &AtomicBool::new(false)).unwrap();
            if song.index == 0 {
                assert_eq!(zeff_firmware::sha256_hex(&prepared.bytes), native_hash);
            }
            assert_eq!(&prepared.bytes[..0x7810], &bytes[..0x7810]);
            assert_eq!(
                report
                    .asset_relations(
                        crate::catalog::SongId::NesNative(usize::from(song.index)),
                        Default::default(),
                        &AtomicBool::new(false),
                    )
                    .status,
                crate::relations::GraphStatus::Complete
            );
            assert!(source_span_matches(
                &bytes,
                &report.media,
                song.table_entry.into()
            ));
            assert!(source_span_matches(
                &bytes,
                &report.media,
                SourceSpan {
                    effective_offset: song.native.driver.effective_offset + 2472,
                    canonical_cpu_address: Some(song.native.driver.canonical_cpu_address + 2472),
                    byte_len: 1,
                }
            ));
            assert!(!source_span_matches(
                &bytes,
                &report.media,
                SourceSpan {
                    effective_offset: song.native.tables.effective_offset + 6,
                    canonical_cpu_address: Some(song.native.tables.canonical_cpu_address + 6),
                    byte_len: 2,
                }
            ));
        }
    }
}

#[test]
fn changed_sources_and_forged_selections_cannot_prepare() {
    let bytes = rom(false);
    let song = &songs(&bytes)[0];
    let mut changed = bytes.clone();
    changed[0x8010] = 1;
    assert_eq!(songs(&changed).len(), 2);
    assert!(prepare(&changed, song, &AtomicBool::new(false)).is_err());
    for field in 0..5 {
        let mut forged = song.clone();
        match field {
            0 => forged.raw_index = 1,
            1 => forged.raw_index = 255,
            2 => forged.header.canonical_cpu_address += 1,
            3 => forged.native.mapper = 3,
            _ => forged.source_sha256 = Some("00".repeat(32)),
        }
        assert!(prepare(&bytes, &forged, &AtomicBool::new(false)).is_err());
    }
    assert!(prepare(&bytes, song, &AtomicBool::new(true)).is_err());
}

#[test]
fn engine_helpers_and_prefetch_padding_are_authenticated() {
    let bytes = rom(false);
    for offset in [
        6,
        0x4010,
        0x405b,
        0x4077,
        0x4081,
        0x4094,
        0x4094 + 0x200,
        0x4a3c,
        0x800a,
        0x7810,
    ] {
        let mut changed = bytes.clone();
        changed[offset] ^= 0x40;
        assert!(songs(&changed).is_empty(), "offset {offset:x}");
    }
}
