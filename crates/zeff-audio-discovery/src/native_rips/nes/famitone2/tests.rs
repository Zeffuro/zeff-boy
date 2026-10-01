use super::*;
use crate::{ScanLimits, catalog::SongRef, native_rips, rips};

#[test]
fn selected_nsf_and_nsfe_preserve_mapping_and_qualified_bytes() {
    let cancel = AtomicBool::new(false);
    let expected = [
        [
            "6a151779dfd395000577dfaa7b5d91e303cc625c2644b264eb8d0744cd70ac8d",
            "23e0a18c1a6b4c2666c84936aeea671a4bd2c582e5e597c1cef192feb4fa3095",
        ],
        [
            "da9bf541322751054fb305ad1d9758325e25cdf04b4e2441d277a1b1b61c9b91",
            "6e1ec5fe0e13114a5ea66f04ec151eaea182cce403bb39a6ceaf4f5118c3c6a7",
        ],
    ];
    for (layout, hashes) in expected.iter().enumerate() {
        let bytes = crate::nes_native::fixture_rom_famitone2(layout != 0);
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
    let bytes = crate::nes_native::fixture_rom_famitone2(false);
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
fn four_channel_rips_match_the_external_player_proof() {
    let cancel = AtomicBool::new(false);
    let expected = [
        [
            "d247f10660c4ee15b61239fffcbeafe53e8cef3856989413d60d74cab495e52b",
            "4f88ff33f51c6eabb662691096e0fa892596be2c3b6c6ac759ea6f20a12eb8c4",
        ],
        [
            "3a30bc55cd342415d7e93f0e4d18d1c2ce626d0bd67ad3f5d3546ee8aefa0606",
            "7a69c259521d19ebf8f6496f42b83897e75bb695fdef2ab7689fa2877273188e",
        ],
    ];
    for (layout, hashes) in expected.iter().enumerate() {
        let bytes = crate::nes_native::fixture_rom_famitone2_four_channel(layout != 0);
        let report = crate::scan(
            zeff_emu_common::system::System::Nes,
            &bytes,
            Default::default(),
            &cancel,
        );
        assert_eq!(report.nes_native_songs.len(), 2);
        for (cue, song) in report.nes_native_songs.iter().enumerate() {
            let selection = SongRef::NesNative(song);
            let nsf = native_rips::encode(&bytes, selection, &cancel).unwrap();
            assert_eq!(zeff_firmware::sha256_hex(&nsf.bytes), hashes[cue]);
            assert!(native_rips::supports_format(
                selection,
                NativeRipFormat::Nsfe
            ));
        }
    }
}
