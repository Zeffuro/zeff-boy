use super::*;
use crate::audio_discovery::{
    catalog::SongId, export::SongExportRequest, formats::SongFormat, render::RenderOptions,
};
use std::sync::atomic::Ordering;
use zeff_emu_common::system::System;
use zeff_ws_core::emulator::Emulator;

#[test]
fn wsr_bootstrap_preserves_selectors_through_init_and_reset() -> Result<()> {
    let mut image = wsr::synthetic_wsr();
    image[0xfd000..0xfd00c].copy_from_slice(&[
        0x31, 0xc0, 0x8e, 0xd8, 0xa1, 0xf0, 0x3f, 0xea, 0, 0xe0, 0, 0xf0,
    ]);
    image[0xffff0..0xffff5].copy_from_slice(&[0xea, 0, 0xd0, 0, 0xf0]);
    let mut emulator = Emulator::new(&image, 44_100)?;
    let mut first = Vec::new();
    for selector in (0..=255_u16).chain([256, u16::MAX, 0]) {
        emulator.reset();
        emulator.cpu_write8(0x3ff0, selector as u8);
        emulator.cpu_write8(0x3ff1, (selector >> 8) as u8);
        for address in 0x100..0x106 {
            emulator.cpu_write8(address, 0xff);
        }
        for _ in 0..200_000 {
            if emulator.cpu_cycles() >= 81_408 {
                break;
            }
            emulator.step_instruction();
            assert!(emulator.last_trap().is_none(), "selector {selector}");
        }
        assert!(emulator.cpu_cycles() >= 81_408);
        assert_eq!(emulator.cpu_peek16(0x100), 0x1234);
        let mut audio = Vec::new();
        emulator.drain_audio_samples_into(&mut audio);
        assert!(!audio.is_empty());
        if selector == 0 {
            assert_eq!(emulator.cpu_peek16(0x102), 32);
            assert!(emulator.cpu_peek16(0x104) > 0);
            assert!(audio.iter().any(|&sample| sample != 0.0));
            if first.is_empty() {
                first = audio;
            } else {
                assert_eq!(audio, first);
            }
        } else {
            assert_eq!(emulator.cpu_peek16(0x102), 0, "selector {selector}");
            assert_eq!(emulator.cpu_peek16(0x104), 0);
            assert!(audio.iter().all(|&sample| sample == 0.0));
            assert_eq!(emulator.io_peek8(0xb2), 0);
        }
    }
    Ok(())
}

#[test]
fn wsr_export_refuses_unqualified_or_forged_sources_without_files() -> Result<()> {
    let input = ScanInput {
        cdda: None,
        system: Some(System::Ws),
        standalone_audio: None,
        bytes: zeff_audio_discovery::ws_tose::synthetic_legacy_rom().into(),
        provenance: None,
        analysis_profile: "wsr-export-test",
        display_name: None,
    };
    let cancel = AtomicBool::new(false);
    let directory = tempfile::tempdir()?;
    for mutation in 0..5 {
        let mut manifest = input.analyze(Default::default(), &cancel);
        let song = &mut manifest.scan.ws_tose_songs[0];
        if mutation != 0 {
            song.profile = "ws-tose-fixed-v5";
            song.index = 32;
            song.wsr_exportable = true;
        }
        match mutation {
            0 | 1 => (),
            2 => manifest.scan.media.sha256 = Some("f".repeat(64)),
            3 => manifest.scan.media.system = "nes",
            4 => song.table_entry.effective_offset = u32::MAX,
            _ => unreachable!(),
        }
        let output = directory.path().join(format!("rejected-{mutation}.wsr"));
        let progress = AtomicU32::new(0);
        let result = SongExportRequest::prepare(
            &input,
            &manifest,
            SongId::WsTose(0),
            SongFormat::Wsr,
            RenderOptions::default(),
        )
        .and_then(|request| request.write_new(&output, &cancel, &progress));
        assert!(result.is_err(), "mutation {mutation}");
        assert!(!output.exists());
        assert_eq!(progress.load(Ordering::Relaxed), 0);
    }
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 0);
    Ok(())
}
