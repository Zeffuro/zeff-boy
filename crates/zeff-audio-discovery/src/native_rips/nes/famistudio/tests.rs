use super::*;
use crate::{ScanLimits, catalog::SongRef, native_rips, rips};

#[test]
fn selected_nsf_and_nsfe_preserve_mapping_and_qualified_bytes() {
    let cancel = AtomicBool::new(false);
    let expected = [
        [
            "f8057a4d045ef5382217ea6d538b8bb385f8345d4023b3a0b72dc07b0c001e33",
            "edcc4503860be33b36d0225d3c91b721f4ccd5422107ce3b8c5baacfff912119",
        ],
        [
            "b0e42385ce547b3a5dc7c112f9bf58c5dd681fcdf3e61b037d8b5a826ecbbfa7",
            "3534bad0be8da762d32ad43d19245297ca493d469ce7c32f3af5d35ea2d4d226",
        ],
    ];
    for (layout, hashes) in expected.iter().enumerate() {
        let bytes = crate::nes_native::fixture_rom_famistudio(layout != 0);
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
    let bytes = crate::nes_native::fixture_rom_famistudio(false);
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
