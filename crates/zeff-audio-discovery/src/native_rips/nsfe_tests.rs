use super::*;
use crate::{ScanLimits, formats::SongFormat};
use zeff_emu_common::system::System;

fn chunks(bytes: &[u8]) -> Vec<([u8; 4], &[u8])> {
    assert_eq!(&bytes[..4], b"NSFE");
    let mut remaining = &bytes[4..];
    let mut result = Vec::new();
    while !remaining.is_empty() {
        let len = u32::from_le_bytes(remaining[..4].try_into().unwrap()) as usize;
        let tag = remaining[4..8].try_into().unwrap();
        result.push((tag, &remaining[8..8 + len]));
        remaining = &remaining[8 + len..];
    }
    result
}

#[test]
fn nsfe_preserves_qualified_nsf_code_selector_mapping_and_timing() {
    let bytes = crate::nes_native::fixture_rom();
    let cancel = AtomicBool::new(false);
    let report = crate::scan(System::Nes, &bytes, ScanLimits::default(), &cancel);
    assert!(!report.nes_native_songs.is_empty());
    for song in &report.nes_native_songs {
        let selected = SongRef::NesNative(song);
        assert!(selected.supports(SongFormat::Nsf));
        assert!(selected.supports(SongFormat::Nsfe));
        let nsf = encode(&bytes, selected, &cancel).unwrap();
        let nsfe = encode_as(&bytes, selected, NativeRipFormat::Nsfe, &cancel).unwrap();
        let parts = chunks(&nsfe.bytes);
        assert_eq!(
            parts.iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
            [*b"INFO", *b"RATE", *b"DATA", *b"tlbl", *b"NEND"]
        );
        assert_eq!(parts[0].1, &[0, 0x80, 0, 0x80, 0x30, 0xed, 0, 0, 1, 0]);
        assert_eq!(parts[1].1, &[0xff, 0x40, 0x1d, 0x4e]);
        assert_eq!(parts[2].1, &nsf.bytes[0x80..]);
        assert_eq!(parts[2].1[13], song.raw_index);
        assert_eq!(parts[3].1, format!("{}\0", song.title).as_bytes());
        assert!(parts[4].1.is_empty());
        assert_eq!(nsfe.metadata.format, NativeRipFormat::Nsfe);
        assert_eq!(nsfe.metadata.source_sha256, nsf.metadata.source_sha256);
        assert_eq!(nsfe.metadata.play_rate_numerator, 1_000_000);
        assert_eq!(nsfe.metadata.play_rate_denominator, 16639);
        assert_eq!(
            nsfe.metadata.output_sha256,
            zeff_firmware::sha256_hex(&nsfe.bytes)
        );
        assert_eq!(nsfe.metadata.output_byte_len, nsfe.bytes.len());
        assert_eq!(
            nsfe.bytes,
            encode_as(&bytes, selected, NativeRipFormat::Nsfe, &cancel)
                .unwrap()
                .bytes
        );
        assert!(
            encode_as(
                &bytes,
                selected,
                NativeRipFormat::Nsfe,
                &AtomicBool::new(true)
            )
            .is_err()
        );
        let mut changed = song.clone();
        changed.native.timing = crate::nes_native::NesNativeTiming::Pal;
        assert!(!SongRef::NesNative(&changed).supports(SongFormat::Nsfe));
        assert!(
            encode_as(
                &bytes,
                SongRef::NesNative(&changed),
                NativeRipFormat::Nsfe,
                &cancel
            )
            .is_err()
        );
        let mut changed = bytes.clone();
        changed[16 + 0x6c4c] ^= 1;
        assert!(encode_as(&changed, selected, NativeRipFormat::Nsfe, &cancel).is_err());
        assert!(encode_as(&bytes, selected, NativeRipFormat::Gbs, &cancel).is_err());
    }
}

#[test]
fn nsfe_does_not_admit_other_nes_drivers_or_structural_imports() {
    let cancel = AtomicBool::new(false);
    let bytes = crate::nes_native::fixture_rom_nintendo();
    let report = crate::scan(System::Nes, &bytes, ScanLimits::default(), &cancel);
    assert!(!report.nes_native_songs.is_empty());
    for song in &report.nes_native_songs {
        let selected = SongRef::NesNative(song);
        assert!(!selected.supports(SongFormat::Nsfe));
        assert!(encode_as(&bytes, selected, NativeRipFormat::Nsfe, &cancel).is_err());
    }
    let bytes = crate::test_support::rips::fixture(crate::rips::RipFormat::Nsf);
    let rip = crate::rips::inspect(
        &bytes,
        crate::rips::RipFormat::Nsf,
        ScanLimits::default(),
        &cancel,
    )
    .unwrap()
    .unwrap();
    assert!(!SongRef::Rip(&rip).supports(SongFormat::Nsfe));
}
