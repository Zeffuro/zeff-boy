use super::*;
use zeff_audio_discovery::{ScanLimits, catalog::SongId};
use zeff_emu_common::system::System;

fn options() -> RenderOptions {
    RenderOptions {
        max_seconds: 1,
        sample_rate: 44_100,
        ..Default::default()
    }
}

#[test]
fn gbass_started_hardware_runs_once_across_render_reset_and_seek() -> Result<()> {
    check_gbass_hardware_starts(zeff_audio_discovery::gbass::fixture_rom_started(), 1)
}

#[test]
fn gbass_partial_bank_preserves_wrapper_restart_across_render_reset_and_seek() -> Result<()> {
    check_gbass_hardware_starts(zeff_audio_discovery::gbass::fixture_rom_partial(), 2)
}

fn check_gbass_hardware_starts(bytes: Vec<u8>, playback_starts: u32) -> Result<()> {
    let cancel = AtomicBool::new(false);
    let report = zeff_audio_discovery::scan(System::Gba, &bytes, ScanLimits::default(), &cancel);
    assert_eq!(
        report.status == zeff_audio_discovery::ScanStatus::Complete,
        playback_starts == 1
    );
    assert!(report.song(SongId::Gbass(0)).is_some());
    let song = &report.gbass_songs[0];
    let prepared = zeff_audio_discovery::gbass::prepare_rom(&bytes, song, &cancel)?;
    let mut session = GbaSession::new_ready(
        prepared.bytes,
        prepared.wait_loop,
        options(),
        Vec::new(),
        &cancel,
    )?;
    session.finish_boot(&cancel)?;
    assert_eq!(session.emulator.cpu_peek32(0x0300_1408), 1);
    let mut expected = vec![0; 20_000];
    assert_eq!(session.read(&mut expected, &cancel)?, expected.len());
    assert!(expected.windows(2).any(|samples| samples[0] != samples[1]));
    assert_eq!(session.emulator.cpu_peek32(0x0300_1408), playback_starts);
    session.reset()?;
    let mut actual = vec![0; expected.len()];
    session.read(&mut actual, &cancel)?;
    assert_eq!(actual, expected);
    assert_eq!(session.emulator.cpu_peek32(0x0300_1408), playback_starts);
    session.reset()?;
    session.read(&mut vec![0; 2000], &cancel)?;
    let mut part = vec![0; 2000];
    session.read(&mut part, &cancel)?;
    assert_eq!(part, expected[2000..4000]);
    assert_eq!(session.emulator.cpu_peek32(0x0300_1408), playback_starts);
    Ok(())
}

#[test]
fn aas_stream_handoff_preserves_configuration_and_native_argument_abi() -> Result<()> {
    let bytes = zeff_audio_discovery::aas_stream::fixture_rom();
    let cancel = AtomicBool::new(false);
    let report = zeff_audio_discovery::scan(System::Gba, &bytes, ScanLimits::default(), &cancel);
    let mut rendered = Vec::new();
    for index in 0..2 {
        let song = &report.aas_stream_songs[index];
        assert!(report.song(SongId::AasStream(index)).is_some());
        let prepared = zeff_audio_discovery::aas_stream::prepare_rom(&bytes, song, &cancel)?;
        let mut session = GbaSession::new_ready(
            prepared.bytes,
            prepared.wait_loop,
            options(),
            Vec::new(),
            &cancel,
        )?;
        session.finish_boot(&cancel)?;
        assert_eq!(session.emulator.cpu_peek32(0x0300_1424), 1);
        assert_eq!(session.emulator.cpu_peek32(0x0300_1414), 0);
        let mut expected = vec![0; 20_000];
        assert_eq!(session.read(&mut expected, &cancel)?, expected.len());
        assert!(expected.iter().any(|&sample| sample != 0));
        for (offset, value) in [0, 64, index as u32 * 8, 8, 255, 1].into_iter().enumerate() {
            assert_eq!(
                session.emulator.cpu_peek32(0x0300_1400 + offset as u32 * 4),
                value
            );
        }
        assert!(session.emulator.cpu_peek32(0x0300_141c) > 0);
        assert!(session.emulator.cpu_peek32(0x0300_1420) > 0);
        assert_eq!(session.emulator.cpu_peek32(0x0300_1424), 1);
        session.reset()?;
        let mut actual = vec![0; expected.len()];
        session.read(&mut actual, &cancel)?;
        assert_eq!(actual, expected);
        session.reset()?;
        session.read(&mut vec![0; 2000], &cancel)?;
        let mut part = vec![0; 2000];
        session.read(&mut part, &cancel)?;
        assert_eq!(part, expected[2000..4000]);
        assert_eq!(session.emulator.cpu_peek32(0x0300_1424), 1);
        rendered.push(expected);
    }
    assert_ne!(rendered[0], rendered[1]);
    Ok(())
}

#[test]
fn aas_pcm_preserves_native_sample_arguments_and_reset() -> Result<()> {
    let bytes = zeff_audio_discovery::aas_pcm::fixture_rom();
    let cancel = AtomicBool::new(false);
    let report = zeff_audio_discovery::scan(System::Gba, &bytes, ScanLimits::default(), &cancel);
    assert_eq!(report.aas_pcm_songs.len(), 2);
    let mut rendered = Vec::new();
    for (index, song) in report.aas_pcm_songs.iter().enumerate() {
        assert!(report.song(SongId::AasPcm(index)).is_some());
        let prepared = zeff_audio_discovery::aas_pcm::prepare_rom(&bytes, song, &cancel)?;
        let mut session = GbaSession::new_ready(
            prepared.bytes,
            prepared.wait_loop,
            options(),
            Vec::new(),
            &cancel,
        )?;
        session.finish_boot(&cancel)?;
        assert_eq!(session.emulator.cpu_peek32(0x0300_1424), 1);
        assert_eq!(session.emulator.cpu_peek32(0x0300_1458), 0);
        let mut expected = vec![0; 20_000];
        assert_eq!(session.read(&mut expected, &cancel)?, expected.len());
        assert!(expected.iter().any(|&sample| sample != 0));
        let arguments = if index == 0 {
            [3, 64, 10_400, 16, 24, 16, 1]
        } else {
            [0, 64, 10_400, 40, 48, 0, 1]
        };
        for (offset, value) in arguments.into_iter().enumerate() {
            assert_eq!(
                session.emulator.cpu_peek32(0x0300_1440 + offset as u32 * 4),
                value
            );
        }
        assert_eq!(
            session.emulator.cpu_peek32(0x0300_1418),
            if index == 0 { 0x87bf } else { 0x875f }
        );
        assert!(session.emulator.cpu_peek32(0x0300_141c) > 0);
        assert!(session.emulator.cpu_peek32(0x0300_1420) > 0);
        assert_eq!(session.emulator.cpu_peek32(0x0300_1424), 1);
        session.reset()?;
        let mut actual = vec![0; expected.len()];
        session.read(&mut actual, &cancel)?;
        assert_eq!(actual, expected);
        session.reset()?;
        session.read(&mut vec![0; 2000], &cancel)?;
        let mut part = vec![0; 2000];
        session.read(&mut part, &cancel)?;
        assert_eq!(part, expected[2000..4000]);
        assert_eq!(session.emulator.cpu_peek32(0x0300_1458), 1);
        assert_eq!(session.emulator.cpu_peek32(0x0300_1424), 1);
        rendered.push(expected);
    }
    assert_ne!(rendered[0], rendered[1]);
    Ok(())
}
