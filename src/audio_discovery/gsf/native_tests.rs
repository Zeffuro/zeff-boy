use std::io::{Cursor, Read};

use super::super::tests::decode;
use super::*;

#[test]
fn standalone_handshake_executes_and_preserves_ack_state() -> Result<()> {
    for offset in [0, 4] {
        let mut bytes = vec![0; 0x200];
        bytes[0xB2] = 0x96;
        bytes[..4].copy_from_slice(&0xEA00_003Eu32.to_le_bytes());
        for (index, word) in [
            0xE59F_0018,
            0xE590_1000 | offset,
            0xE351_0001,
            0x1AFF_FFFC,
            0xE580_1008,
            0xEAFF_FFFE,
            0,
            0,
            0x0300_7FE0u32,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[0x100 + index * 4..0x104 + index * 4].copy_from_slice(&word.to_le_bytes());
        }
        let wait = RomSpan {
            effective_offset: 0x104,
            byte_len: 12,
            canonical_cpu_address: 0x0800_0104,
        };
        let patched = autonomous(bytes.clone(), wait)?;
        let mut emulator = zeff_gba_core::emulator::Emulator::new(&patched, 44_100)?;
        emulator.step_frame();
        assert_eq!(emulator.cpu_peek32(0x0300_7FE0 + offset), 1);
        assert_eq!(emulator.cpu_peek32(0x0300_7FE8), 1);
        bytes[0x108] ^= 1;
        assert!(autonomous(bytes, wait).is_err());
        assert!(
            autonomous(
                patched,
                RomSpan {
                    effective_offset: 0x1000,
                    ..wait
                }
            )
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn native_pack_load_order_reconstructs_full_gsf_and_shares_source() -> Result<()> {
    let cancel = AtomicBool::new(false);
    let source = vec![0x55; 0x10000];
    let mut prior_base = None;
    for song in [1u8, 9] {
        let mut prepared = source.clone();
        prepared[..4].copy_from_slice(&[1, 2, 3, 4]);
        prepared[0x800] = song;
        prepared.extend_from_slice(&[song; 128]);
        let full = super::super::codec::encode(0x0800_0000, 0x0800_0000, &prepared, &[], &cancel)?;
        let zip = super::super::overlay::pack(
            &source,
            &prepared,
            "song.minigsf",
            &[],
            json!({}),
            &cancel,
        )?;
        let mut pack = zip::ZipArchive::new(Cursor::new(zip))?;
        let mut mini = Vec::new();
        pack.by_name("song.minigsf")?.read_to_end(&mut mini)?;
        let (_, address, patch, tags) = decode(&mini);
        let libraries: Vec<_> = tags
            .trim_start_matches("[TAG]")
            .lines()
            .filter_map(|line| {
                line.strip_prefix("_lib")
                    .and_then(|line| line.split_once('='))
            })
            .collect();
        assert_eq!(libraries.len(), 3);
        let mut reconstructed = Vec::new();
        for (index, (_, name)) in libraries.iter().enumerate() {
            let mut data = Vec::new();
            pack.by_name(name)?.read_to_end(&mut data)?;
            let (_, address, bytes, _) = decode(&data);
            if index == 0 {
                if let Some(prior) = &prior_base {
                    assert_eq!(prior, &data);
                }
                prior_base = Some(data);
                assert_eq!(bytes, source);
            }
            let offset = (address - 0x0800_0000) as usize;
            reconstructed.resize(reconstructed.len().max(offset + bytes.len()), 0);
            reconstructed[offset..offset + bytes.len()].copy_from_slice(&bytes);
            if index == 0 {
                let offset = (decode(&mini).1 - 0x0800_0000) as usize;
                reconstructed[offset..offset + patch.len()].copy_from_slice(&patch);
            }
        }
        assert_eq!(address, 0x0800_0000);
        assert_eq!(patch.len(), 4);
        assert_eq!(reconstructed, decode(&full).2);
        assert!(
            super::super::overlay::pack(
                &source,
                &prepared,
                "../song.minigsf",
                &[],
                json!({}),
                &cancel
            )
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn native_gsf_routes_validated_drivers_and_publishes_autonomous_audio() -> Result<()> {
    use zeff_audio_discovery as audio;
    let cancel = AtomicBool::new(false);
    let directory = crate::test_support::test_directory("native-gsf-drivers")?;
    let fixtures = [
        audio::aas::fixture_rom(false),
        audio::aas::fixture_rom(true),
        audio::descriptor_midi::fixture_rom(),
        audio::nsq::fixture_rom(),
        audio::radriver::fixture_rom(),
        audio::radriver::fixture_global_rom(),
        audio::gbass::fixture_rom(),
        audio::gbass::fixture_rom_irq(),
        audio::gbass::fixture_rom_started(),
        audio::gbass::fixture_rom_partial(),
        audio::aas_stream::fixture_rom(),
        audio::aas_pcm::fixture_rom(),
        audio::gbass::fixture_rom_banked(),
        audio::gbass::fixture_rom_module(),
        audio::gbass::fixture_rom_separate(),
    ];
    for (index, mut bytes) in fixtures.into_iter().enumerate() {
        bytes[0xB2] = 0x96;
        let input = ScanInput {
            system: Some(zeff_emu_common::system::System::Gba),
            standalone_audio: None,
            bytes: bytes.into(),
            cdda: None,
            provenance: None,
            analysis_profile: "native-gsf-test",
            display_name: Some("Fixture".to_owned()),
        };
        let manifest = input.analyze(Default::default(), &cancel);
        let ids: Vec<_> = manifest
            .scan
            .song_ids()
            .filter(|&id| {
                !matches!(id, SongId::Mp2k(_))
                    && manifest.scan.song(id).unwrap().supports(SongFormat::Gsf)
            })
            .collect();
        assert!(!ids.is_empty(), "fixture {index}");
        for (selector, id) in ids
            .into_iter()
            .take(if index >= 12 { 4 } else { 2 })
            .enumerate()
        {
            let path = directory.path().join(format!("{index}-{selector}.gsf"));
            let options = RenderOptions {
                max_seconds: 3,
                fade_seconds: 1,
                ..Default::default()
            };
            let request = crate::audio_discovery::export::SongExportRequest::prepare(
                &input,
                &manifest,
                id,
                SongFormat::Gsf,
                options,
            )?;
            request.write_new(&path, &cancel, &AtomicU32::new(0))?;
            let (_, _, bytes, tags) = decode(&std::fs::read(&path)?);
            assert!(tags.contains("length=2\nfade=1\n"));
            if index >= 6 {
                let mut emulator = zeff_gba_core::emulator::Emulator::new(&bytes, 44_100)?;
                let mut floats = Vec::new();
                let mut nonzero = 0;
                for _ in 0..30 {
                    emulator.step_frame();
                    emulator.drain_audio_samples_into(&mut floats);
                    assert!(floats.iter().all(|value| value.is_finite()));
                    nonzero += floats.iter().filter(|&&value| value != 0.0).count();
                    floats.clear();
                }
                assert!(nonzero > 0, "fixture {index} song {selector}");
                assert_eq!(emulator.cpu_peek32(audio::gba_bootstrap::ACK_ADDRESS), 1);
            }
            let before = std::fs::read(&path)?;
            assert!(
                crate::audio_discovery::export::SongExportRequest::prepare(
                    &input,
                    &manifest,
                    id,
                    SongFormat::Gsf,
                    options
                )?
                .write_new(&path, &cancel, &AtomicU32::new(0))
                .is_err()
            );
            assert_eq!(std::fs::read(&path)?, before);
        }
    }
    Ok(())
}
