use super::*;

fn fixture() -> PreparedGbBanked {
    use zeff_audio_discovery::gb_music::native;
    native::prepare_rom(
        &native::fixture_rom(),
        &native::fixture_song(),
        &AtomicBool::new(false),
    )
    .unwrap()
}

fn options() -> RenderOptions {
    RenderOptions {
        max_seconds: 1,
        sample_rate: 44_100,
        ..Default::default()
    }
}

fn render(session: &mut GbBankedSession, block: usize) -> Result<Vec<i16>> {
    let mut result = Vec::new();
    let mut chunk = vec![0; block];
    loop {
        let count = session.read(&mut chunk, &AtomicBool::new(false))?;
        if count == 0 {
            return Ok(result);
        }
        result.extend_from_slice(&chunk[..count]);
    }
}

#[test]
fn isolated_carillon_retains_source_and_replays_at_both_sample_rates() -> Result<()> {
    let bytes = zeff_audio_discovery::gb_carillon::synthetic_rom_alternate();
    let original = bytes.clone();
    let cancel = AtomicBool::new(false);
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Gb,
        &bytes,
        Default::default(),
        &cancel,
    );
    let song = &report.gb_carillon_songs[0];
    assert_eq!(song.profile, "carillon-cgb-v1-isolated");
    for sample_rate in [44_100, 48_000] {
        let prepared = zeff_audio_discovery::gb_carillon::prepare_rom(&bytes, song, &cancel)?;
        let mut session = GbBankedSession::new_carillon(
            prepared,
            RenderOptions {
                sample_rate,
                ..options()
            },
            song.warnings.clone(),
            &cancel,
        )?;
        let expected = render(&mut session, 2048)?;
        assert_eq!(expected.len(), sample_rate as usize * 2);
        assert!(expected.iter().any(|&sample| sample != 0));
        assert_eq!(session.emulator.cpu_peek8(0xff4d) & 0x80, 0x80);
        assert_eq!(session.emulator.cpu_peek8(0xc7d4), 0xff);
        session.reset()?;
        assert_eq!(render(&mut session, 258)?, expected);
    }
    assert_eq!(bytes, original);
    Ok(())
}

#[test]
fn cgb_native_handoff_chunking_reset_and_requested_duration_are_stable() -> Result<()> {
    let mut session =
        GbBankedSession::new(fixture(), options(), Vec::new(), &AtomicBool::new(false))?;
    assert!(!session.has_source_duration_limit());
    assert_eq!(session.duration_frames(), 44_100);
    assert_eq!(session.read(&mut [], &AtomicBool::new(false))?, 0);
    assert!(session.read(&mut [0; 64], &AtomicBool::new(true)).is_err());
    assert_eq!(session.position_frames(), 0);
    let expected = render(&mut session, 2048)?;
    assert_eq!(expected.len(), 88_200);
    assert!(expected.iter().any(|&sample| sample != 0));
    assert_eq!(session.emulator.hardware_mode(), HardwareMode::CGBNormal);
    assert_eq!(session.emulator.cpu_peek8(0xc103), 0x11);
    assert_eq!(session.emulator.cpu_peek8(0xc101), 1);
    assert!(session.emulator.cpu_peek8(0xc100) > 0);
    session.reset()?;
    assert_eq!(render(&mut session, 258)?, expected);
    session.set_track_mask(0)?;
    session.reset()?;
    assert!(render(&mut session, 512)?.iter().all(|&sample| sample == 0));
    assert!(session.set_track_mask(2).is_err());
    Ok(())
}

#[test]
fn native_handoff_needs_both_marker_and_wait_pc() -> Result<()> {
    let mut prepared = fixture();
    prepared.wait_start = 0xf0;
    prepared.wait_end = 0xf4;
    let ack = prepared.ack_address;
    let mut session =
        GbBankedSession::new(prepared, options(), Vec::new(), &AtomicBool::new(false))?;
    assert!(session.read(&mut [0; 64], &AtomicBool::new(false)).is_err());
    assert_eq!(session.emulator.cpu_peek8(ack), 0);
    assert_eq!(session.position_frames(), 0);
    Ok(())
}

#[test]
fn native_cgb_cartridge_and_handoff_contracts_are_required() {
    for (offset, value) in [(0x143, 0), (0x147, 0x13), (0x148, 5), (0x149, 0)] {
        let mut prepared = fixture();
        prepared.bytes[offset] = value;
        assert!(
            GbBankedSession::new(prepared, options(), Vec::new(), &AtomicBool::new(false)).is_err()
        );
    }
    let mut prepared = fixture();
    prepared.ack_address = prepared.ready_address;
    assert!(
        GbBankedSession::new(prepared, options(), Vec::new(), &AtomicBool::new(false)).is_err()
    );
}

#[test]
fn musyx_selectors_use_mbc5_handoff_and_stable_requested_duration() -> Result<()> {
    let bytes = zeff_audio_discovery::gb_musyx::synthetic_rom();
    let cancel = AtomicBool::new(false);
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Gb,
        &bytes,
        Default::default(),
        &cancel,
    );
    assert_eq!(report.gb_musyx_songs.len(), 2);
    for song in &report.gb_musyx_songs {
        let make = || zeff_audio_discovery::gb_musyx::prepare_rom(&bytes, song, &cancel);
        let mut session = GbBankedSession::new_musyx(make()?, options(), Vec::new(), &cancel)?;
        let expected = render(&mut session, 2048)?;
        assert_eq!(expected.len(), 88_200);
        assert!(expected.iter().any(|&value| value != 0));
        assert_eq!(session.emulator.hardware_mode(), HardwareMode::CGBNormal);
        assert_eq!(session.emulator.cpu_peek8(0xdf21), song.index as u8);
        assert!((58..=61).contains(&session.emulator.cpu_peek8(0xdf20)));
        session.reset()?;
        assert_eq!(render(&mut session, 258)?, expected);
        session.set_track_mask(0)?;
        session.reset()?;
        assert!(render(&mut session, 512)?.iter().all(|&value| value == 0));
        let mut invalid = make()?;
        invalid.bytes[0x147] = 0x10;
        assert!(GbBankedSession::new_musyx(invalid, options(), Vec::new(), &cancel).is_err());
        let mut invalid = make()?;
        invalid.wait_start = 0x1e0;
        invalid.wait_end = 0x1e4;
        let mut session = GbBankedSession::new_musyx(invalid, options(), Vec::new(), &cancel)?;
        assert!(session.read(&mut [0; 64], &cancel).is_err());
        assert_eq!(session.emulator.cpu_peek8(0xfffb), 0);
    }
    Ok(())
}

#[test]
fn tose_uses_dmg_bank_selection_and_a_reserved_handoff() -> Result<()> {
    let bytes = zeff_audio_discovery::gb_tose::synthetic_rom();
    let cancel = AtomicBool::new(false);
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Gb,
        &bytes,
        Default::default(),
        &cancel,
    );
    let song = &report.gb_tose_songs[0];
    let make = || zeff_audio_discovery::gb_tose::prepare_rom(&bytes, song, &cancel);
    let mut session = GbBankedSession::new_tose(make()?, options(), Vec::new(), &cancel)?;
    let expected = render(&mut session, 2048)?;
    assert_eq!(expected.len(), 88_200);
    assert!(expected.iter().any(|&value| value != 0));
    assert_eq!(session.emulator.hardware_mode(), HardwareMode::DMG);
    assert_eq!(session.emulator.cpu_peek8(0xdd9e), song.index as u8);
    assert!((58..=61).contains(&session.emulator.cpu_peek8(0xdd9c)));
    session.reset()?;
    assert_eq!(render(&mut session, 258)?, expected);
    session.set_track_mask(0)?;
    session.reset()?;
    assert!(render(&mut session, 512)?.iter().all(|&value| value == 0));
    assert!(session.set_track_mask(2).is_err());
    for (at, value) in [(0x143, 0x80), (0x147, 0x19), (0x148, 0)] {
        let mut invalid = make()?;
        invalid.bytes[at] = value;
        assert!(GbBankedSession::new_tose(invalid, options(), Vec::new(), &cancel).is_err());
    }
    let mut sgb = make()?;
    sgb.bytes[0x146] = 3;
    sgb.bytes[0x14b] = 0x33;
    let mut session = GbBankedSession::new_tose(sgb, options(), Vec::new(), &cancel)?;
    assert_eq!(session.emulator.hardware_mode(), HardwareMode::DMG);
    assert_eq!(render(&mut session, 2048)?, expected);
    let mut invalid = make()?;
    invalid.wait_start = 0x1e0;
    invalid.wait_end = 0x1e4;
    let mut session = GbBankedSession::new_tose(invalid, options(), Vec::new(), &cancel)?;
    assert!(session.read(&mut [0; 64], &cancel).is_err());
    assert_eq!(session.emulator.cpu_peek8(0xff80), 0);
    Ok(())
}

#[test]
fn quickthunder_switches_to_qualified_speed_before_audio_and_after_reset() -> Result<()> {
    let bytes = zeff_audio_discovery::gb_quickthunder::synthetic_rom();
    let cancel = AtomicBool::new(false);
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Gb,
        &bytes,
        Default::default(),
        &cancel,
    );
    let song = &report.gb_quickthunder_songs[0];
    let make = || zeff_audio_discovery::gb_quickthunder::prepare_rom(&bytes, song, &cancel);
    let mut session = GbBankedSession::new_quickthunder(make()?, options(), Vec::new(), &cancel)?;
    assert_eq!(session.emulator.hardware_mode(), HardwareMode::CGBNormal);
    let expected = render(&mut session, 2048)?;
    assert!(expected.iter().any(|&value| value != 0));
    assert_eq!(session.emulator.hardware_mode(), HardwareMode::CGBDouble);
    session.reset()?;
    assert_eq!(session.emulator.hardware_mode(), HardwareMode::CGBNormal);
    assert_eq!(render(&mut session, 258)?, expected);
    assert_eq!(session.emulator.hardware_mode(), HardwareMode::CGBDouble);

    let mut invalid = make()?;
    let stop = invalid.bytes[0x150..0x200]
        .windows(2)
        .position(|pair| pair == [0x10, 0])
        .expect("bootstrap contains the CGB speed switch");
    invalid.bytes[0x150 + stop] = 0;
    let ack = invalid.ack_address;
    let mut session = GbBankedSession::new_quickthunder(invalid, options(), Vec::new(), &cancel)?;
    assert!(session.read(&mut [0; 64], &cancel).is_err());
    assert_eq!(session.position_frames(), 0);
    assert_eq!(session.emulator.cpu_peek8(ack), 0);
    Ok(())
}

#[test]
fn isolated_quickthunder_replays_source_banks_at_both_sample_rates() -> Result<()> {
    let cancel = AtomicBool::new(false);
    for header in [0x97, 0x99] {
        let bytes = zeff_audio_discovery::gb_quickthunder::synthetic_rom_rocket(header);
        let report = zeff_audio_discovery::scan(
            zeff_emu_common::system::System::Gb,
            &bytes,
            Default::default(),
            &cancel,
        );
        let song = &report.gb_quickthunder_songs[0];
        for sample_rate in [44_100, 48_000] {
            let make = || zeff_audio_discovery::gb_quickthunder::prepare_rom(&bytes, song, &cancel);
            let mut session = GbBankedSession::new_quickthunder(
                make()?,
                RenderOptions {
                    sample_rate,
                    ..options()
                },
                song.warnings.clone(),
                &cancel,
            )?;
            let expected = render(&mut session, 2048)?;
            assert_eq!(expected.len(), sample_rate as usize * 2);
            assert!(expected.iter().any(|&value| value != 0));
            assert_eq!(session.emulator.hardware_mode(), HardwareMode::CGBDouble);
            session.reset()?;
            assert_eq!(render(&mut session, 258)?, expected);
            session.set_track_mask(0)?;
            session.reset()?;
            assert!(render(&mut session, 512)?.iter().all(|&value| value == 0));
            for (at, value) in [(0x143, 0x80), (0x147, header), (0x148, 1), (0x149, 2)] {
                let mut invalid = make()?;
                invalid.bytes[at] = value;
                assert!(
                    GbBankedSession::new_quickthunder(invalid, options(), Vec::new(), &cancel)
                        .is_err()
                );
            }
        }
    }
    Ok(())
}
#[test]
fn sampled_quickthunder_services_timer_vectors_and_restores_music_bank() -> Result<()> {
    let cancel = AtomicBool::new(false);
    let bytes = zeff_audio_discovery::gb_quickthunder::synthetic_rom_sampled();
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Gb,
        &bytes,
        Default::default(),
        &cancel,
    );
    assert_eq!(report.gb_quickthunder_songs.len(), 4);
    let song = &report.gb_quickthunder_songs[0];
    for sample_rate in [44_100, 48_000] {
        let prepared = zeff_audio_discovery::gb_quickthunder::prepare_rom(&bytes, song, &cancel)?;
        let mut session = GbBankedSession::new_quickthunder(
            prepared,
            RenderOptions {
                sample_rate,
                ..options()
            },
            song.warnings.clone(),
            &cancel,
        )?;
        let expected = render(&mut session, 2048)?;
        assert!(expected.iter().any(|&value| value != 0));
        assert_ne!(session.emulator.cpu_peek8(0xc164), 0);
        assert_eq!(session.emulator.cpu_peek8(0xff88), 62);
        assert_eq!(session.emulator.cpu_peek8(0xca06), 0xc3);
        assert_eq!(session.emulator.cpu_peek8(0xca07), 0x40);
        assert_eq!(session.emulator.cpu_peek8(0xca08), 0x17);
        for address in 0xff30..0xff40 {
            assert_eq!(session.emulator.cpu_peek8(address), 0x37);
        }
        session.reset()?;
        assert_eq!(render(&mut session, 258)?, expected);
    }
    Ok(())
}

#[test]
fn ghx_switches_to_qualified_speed_before_audio_and_after_reset() -> Result<()> {
    let bytes = zeff_audio_discovery::gb_ghx::synthetic_rom();
    let cancel = AtomicBool::new(false);
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Gb,
        &bytes,
        Default::default(),
        &cancel,
    );
    let song = &report.gb_ghx_songs[0];
    let make = || zeff_audio_discovery::gb_ghx::prepare_rom(&bytes, song, &cancel);
    let mut session = GbBankedSession::new_ghx(make()?, options(), Vec::new(), &cancel)?;
    assert_eq!(session.emulator.hardware_mode(), HardwareMode::CGBNormal);
    let expected = render(&mut session, 2048)?;
    assert!(expected.iter().any(|&value| value != 0));
    assert_eq!(session.emulator.hardware_mode(), HardwareMode::CGBDouble);
    session.reset()?;
    assert_eq!(session.emulator.hardware_mode(), HardwareMode::CGBNormal);
    assert_eq!(render(&mut session, 258)?, expected);
    assert_eq!(session.emulator.hardware_mode(), HardwareMode::CGBDouble);

    let mut invalid = make()?;
    let stop = invalid.bytes[0x150..0x200]
        .windows(2)
        .position(|pair| pair == [0x10, 0])
        .expect("bootstrap contains the CGB speed switch");
    invalid.bytes[0x150 + stop] = 0;
    let ack = invalid.ack_address;
    let mut session = GbBankedSession::new_ghx(invalid, options(), Vec::new(), &cancel)?;
    assert!(session.read(&mut [0; 64], &cancel).is_err());
    assert_eq!(session.position_frames(), 0);
    assert_eq!(session.emulator.cpu_peek8(ack), 0);
    Ok(())
}

#[test]
fn sound_system_rejects_a_hardware_contract_that_disagrees_with_startup() -> Result<()> {
    use zeff_audio_discovery::gb_sound_system::{GbSoundSystemHardware, prepare_rom};

    let bytes = zeff_audio_discovery::gb_sound_system::synthetic_rom();
    let cancel = AtomicBool::new(false);
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Gb,
        &bytes,
        Default::default(),
        &cancel,
    );
    let mut prepared = prepare_rom(&bytes, &report.gb_sound_system_songs[0], &cancel)?;
    prepared.hardware = match prepared.hardware {
        GbSoundSystemHardware::CgbNormal => GbSoundSystemHardware::CgbDouble,
        GbSoundSystemHardware::CgbDouble => GbSoundSystemHardware::CgbNormal,
    };
    let ack = prepared.ack_address;
    let mut session = GbBankedSession::new_sound_system(prepared, options(), Vec::new(), &cancel)?;
    assert!(session.read(&mut [0; 64], &cancel).is_err());
    assert_eq!(session.position_frames(), 0);
    assert_eq!(session.emulator.cpu_peek8(ack), 0);
    Ok(())
}
