use super::*;

#[test]
fn supplemental_fixture_has_exact_identity_and_preserves_original_data() {
    let bytes = fixture_rom_supplemental();
    assert_eq!(
        const_hex::encode(zeff_firmware::sha256_bytes(&bytes)),
        fixture::SUPPLEMENTAL_PROFILE.sha256
    );
    let song = inspect(&bytes, &fixture::SUPPLEMENTAL_PROFILE, 0).unwrap();
    let prepared = prepare_rom(&bytes, &song, &AtomicBool::new(false)).unwrap();
    assert_eq!(song.mapped_spans.len(), 3);
    assert!(
        song.warnings
            .iter()
            .any(|warning| warning.contains("low-ROM"))
    );
    for span in &song.mapped_spans {
        let start = span.effective_offset as usize;
        let end = start + span.byte_len as usize;
        assert_eq!(bytes[start..end], prepared.bytes[start..end]);
    }
}

#[test]
fn supplemental_source_mapping_rejects_holes_aliases_and_unqualified_media() {
    let profile = all_profiles()
        .find(|profile| !profile.supplemental_selectors.is_empty())
        .unwrap();
    let identity = || MediaIdentity {
        system: profile.system.code(),
        byte_len: profile.rom_len as u64,
        sha256: Some(profile.sha256.into()),
    };
    let media = identity();
    let span = SourceSpan {
        effective_offset: 0x311,
        byte_len: 3,
        canonical_cpu_address: Some(0x311),
    };
    assert!(source_span_matches(&media, span));
    assert!(source_span_matches(
        &media,
        SourceSpan {
            effective_offset: 0x312,
            byte_len: 1,
            canonical_cpu_address: Some(0x312)
        }
    ));
    for changed in [
        SourceSpan {
            byte_len: 0,
            ..span
        },
        SourceSpan {
            byte_len: 4,
            ..span
        },
        SourceSpan {
            effective_offset: 0x310,
            byte_len: 1,
            canonical_cpu_address: Some(0x310),
        },
        SourceSpan {
            effective_offset: 0x314,
            byte_len: 1,
            canonical_cpu_address: Some(0x314),
        },
        SourceSpan {
            effective_offset: 0x312,
            ..span
        },
        SourceSpan {
            canonical_cpu_address: Some(0x8311),
            ..span
        },
        SourceSpan {
            canonical_cpu_address: None,
            ..span
        },
        SourceSpan {
            effective_offset: 0,
            byte_len: 1,
            canonical_cpu_address: Some(0),
        },
    ] {
        assert!(!source_span_matches(&media, changed), "{changed:?}");
    }
    for changed in [
        MediaIdentity {
            system: "sms",
            ..identity()
        },
        MediaIdentity {
            byte_len: media.byte_len + 1,
            ..identity()
        },
        MediaIdentity {
            sha256: Some(fixture::PROFILE.sha256.into()),
            ..identity()
        },
        MediaIdentity {
            sha256: None,
            ..identity()
        },
    ] {
        assert!(!source_span_matches(&changed, span));
    }
}

#[test]
fn supplemental_mappings_are_selector_specific_and_cannot_be_forged() {
    let bytes = fixture_rom_supplemental();
    let profile = &fixture::SUPPLEMENTAL_PROFILE;
    assert!(profile.supplemental_spans(0x82).is_empty());
    let original = inspect(&bytes, profile, 0).unwrap();
    for altered in 0..3 {
        let mut song = original.clone();
        match altered {
            0 => {
                song.mapped_spans.remove(0);
            }
            1 => {
                song.mapped_spans[0].canonical_cpu_address += 0x8000;
            }
            _ => {
                song.mapped_spans[0].byte_len += 1;
            }
        }
        assert!(prepare_profile(&bytes, &song, profile, &AtomicBool::new(false)).is_err());
    }
}
