use super::*;
use crate::{ScanLimits, catalog::SongRef, native_rips, rips};

#[test]
fn selected_nsf_and_nsfe_preserve_mapping_and_qualified_bytes() {
    let cancel = AtomicBool::new(false);
    let expected = [
        [
            "c964c038a63ea177f7ef17e15d671aba651568043618fd34f08c8b9ea932a246",
            "329ddf3bb7771e923e6a47a5fd75109999b12b0d6d2ab51206d173189b1552de",
        ],
        [
            "b8bba1e8b11ffdc3ee7e656ffc4f3ee90c6e2e54e62658ad5cabf5d8639bb97e",
            "a3f125f944b3f5a70ec6b648b80cf2fc62221fdc639c8c9f88d48a2265bf5623",
        ],
    ];
    for (layout, hashes) in expected.iter().enumerate() {
        let bytes = crate::nes_native::fixture_rom_ggsound(layout != 0);
        let report = crate::scan(
            zeff_emu_common::system::System::Nes,
            &bytes,
            Default::default(),
            &cancel,
        );
        assert_eq!(report.nes_native_songs.len(), 2);
        for (index, song) in report.nes_native_songs.iter().enumerate() {
            let selection = SongRef::NesNative(song);
            let nsf = native_rips::encode(&bytes, selection, &cancel).unwrap();
            assert_eq!(zeff_firmware::sha256_hex(&nsf.bytes), hashes[index]);
            for span in &song.mapped_spans {
                let source = span.effective_offset as usize;
                let output = span.canonical_cpu_address as usize - 0x8000 + 0x80;
                let size = span.byte_len as usize;
                assert_eq!(
                    &nsf.bytes[output..output + size],
                    &bytes[source..source + size]
                );
            }
            for (format, parser) in [
                (NativeRipFormat::Nsf, rips::RipFormat::Nsf),
                (NativeRipFormat::Nsfe, rips::RipFormat::Nsfe),
            ] {
                assert!(native_rips::supports_format(selection, format));
                let rip = native_rips::encode_as(&bytes, selection, format, &cancel).unwrap();
                let parsed = rips::inspect(&rip.bytes, parser, ScanLimits::default(), &cancel)
                    .unwrap()
                    .unwrap();
                assert_eq!((parsed.song_count, parsed.first_song), (Some(1), Some(1)));
                assert_eq!(parsed.init.unwrap().cpu_address, 0xf800);
                assert_eq!(
                    u32::from(parsed.play.unwrap().cpu_address),
                    song.native.tick.canonical_cpu_address
                );
                assert_eq!(rip.metadata.raw_selector, index as u16);
                assert!(
                    native_rips::encode_as(&bytes, selection, format, &AtomicBool::new(true))
                        .is_err()
                );
            }
        }
    }
}

#[test]
fn rip_admission_rechecks_descriptor_and_full_source_identity() {
    let bytes = crate::nes_native::fixture_rom_ggsound(false);
    let cancel = AtomicBool::new(false);
    let report = crate::scan(
        zeff_emu_common::system::System::Nes,
        &bytes,
        Default::default(),
        &cancel,
    );
    let song = &report.nes_native_songs[0];
    let mut stale = bytes.clone();
    stale[0x8010] = 1;
    assert!(native_rips::encode(&stale, SongRef::NesNative(song), &cancel).is_err());
    for field in 0..4 {
        let mut forged = song.clone();
        match field {
            0 => forged.raw_index = 255,
            1 => forged.native.mapper = 3,
            2 => forged.native.timing = NesNativeTiming::Pal,
            _ => forged.header.canonical_cpu_address += 1,
        }
        assert!(native_rips::encode(&bytes, SongRef::NesNative(&forged), &cancel).is_err());
    }
}

#[test]
fn relocated_substream_is_exported_without_the_unused_original_bytes() {
    let mut bytes = crate::nes_native::fixture_rom_ggsound(false);
    let sequence = [0x72, 0, 0x61, 36, 38, 0x75];
    bytes[16 + 98..16 + 104].copy_from_slice(&sequence);
    bytes[16 + 41..16 + 43].copy_from_slice(&0x8062_u16.to_le_bytes());
    let cancel = AtomicBool::new(false);
    let report = crate::scan(
        zeff_emu_common::system::System::Nes,
        &bytes,
        Default::default(),
        &cancel,
    );
    assert_eq!(report.nes_native_songs.len(), 2);
    let song = &report.nes_native_songs[0];
    assert_eq!(
        song.mapped_spans
            .iter()
            .filter(|span| span.canonical_cpu_address < 0xc000)
            .map(|span| span.byte_len)
            .sum::<u32>(),
        84
    );
    let nsf = native_rips::encode(&bytes, SongRef::NesNative(song), &cancel).unwrap();
    assert_eq!(&nsf.bytes[0x80 + 98..0x80 + 104], &sequence);
    assert_eq!(&nsf.bytes[0x80 + 52..0x80 + 57], &[0; 5]);
}
