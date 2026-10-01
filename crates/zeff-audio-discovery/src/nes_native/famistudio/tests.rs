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
    for (relocated, hash, native) in [
        (
            false,
            "ab2e26763247f308030d6b56fbe80caa188fbbba5121576b1f4b2380318f2a9d",
            "cd65d035cafa0b4d940e46d81b4a9c92682f940769f6df486d6820b5e0896598",
        ),
        (
            true,
            "a5fc1b681f87abfc9fc5c601b3130ef9e73491cd42678796f9c33b0e42d2a42b",
            "2efaf016bcfa3af039a4920c23864daf67ad3496118dc4396e57ec8f234f7bc5",
        ),
    ] {
        let bytes = rom(relocated);
        assert_eq!(zeff_firmware::sha256_hex(&bytes), hash);
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
                assert_eq!(zeff_firmware::sha256_hex(&prepared.bytes), native);
            }
            assert_eq!(&prepared.bytes[..0x7810], &bytes[..0x7810]);
            assert_eq!(
                report
                    .asset_relations(
                        crate::catalog::SongId::NesNative(usize::from(song.index)),
                        Default::default(),
                        &AtomicBool::new(false)
                    )
                    .status,
                crate::relations::GraphStatus::Complete
            );
            assert!(source_span_matches(
                &bytes,
                &report.media,
                song.table_entry.into()
            ));
            assert!(!source_span_matches(
                &bytes,
                &report.media,
                SourceSpan {
                    effective_offset: song.header.effective_offset + 55,
                    canonical_cpu_address: Some(song.header.canonical_cpu_address + 55),
                    byte_len: 3,
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
    for field in 0..4 {
        let mut forged = song.clone();
        match field {
            0 => forged.raw_index = 1,
            1 => forged.header.canonical_cpu_address += 1,
            2 => forged.native.mapper = 3,
            _ => forged.source_sha256 = Some("00".repeat(32)),
        }
        assert!(prepare(&bytes, &forged, &AtomicBool::new(false)).is_err());
    }
    assert!(prepare(&bytes, song, &AtomicBool::new(true)).is_err());
}

#[test]
fn engine_caller_and_bootstrap_changes_are_refused() {
    let bytes = rom(false);
    for offset in [
        6,
        16,
        16 + 0x45,
        16 + 0x42,
        16 + 0x52,
        0x800a,
        0x7810,
        0x66,
        0x66 + 0x10,
        0x66 + 0x300,
    ] {
        let mut changed = bytes.clone();
        changed[offset] ^= 0x40;
        assert!(songs(&changed).is_empty(), "offset {offset:x}");
    }
}
