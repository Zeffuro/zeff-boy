use super::*;
use crate::emu_backend::nes::{NesBackend, NesTasLoadProvenance};
use crate::emu_backend::{BackendLoadConfig, load_backend_from_rom_source};
use crate::test_support::{
    TestDirectory, build_nes_battery_test_rom, build_nes_test_rom, test_directory,
};

fn loaded(rom: &[u8], config: BackendLoadConfig) -> (TestDirectory, EmuBackend) {
    let directory = test_directory("netplay-identity").unwrap();
    let path = directory.path().join("game.nes");
    std::fs::write(&path, rom).unwrap();
    let backend = load_backend_from_rom_source(ActiveSystem::Nes, &path, &path, None, config)
        .unwrap()
        .backend;
    (directory, backend)
}

fn clean(rom: &[u8]) -> (TestDirectory, EmuBackend) {
    loaded(
        rom,
        BackendLoadConfig {
            nes_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )
}

fn changed_load(backend: EmuBackend, change: impl FnOnce(&mut NesTasLoadProvenance)) -> EmuBackend {
    let mut load = *backend.nes_tas_load_provenance().unwrap().load;
    change(&mut load);
    let path = backend.source_path().to_owned();
    let EmuBackend::Nes(backend) = backend else {
        unreachable!()
    };
    EmuBackend::Nes(Box::new(NesBackend::with_load_provenance(
        backend.emu,
        path.clone(),
        path,
        load,
    )))
}

#[test]
fn direct_identity_uses_owned_evidence_after_disk_contents_change() {
    let rom = build_nes_test_rom();
    let (directory, backend) = clean(&rom);
    let before = identity(&backend, [7; 32]).unwrap();
    assert_eq!(before.source, Sha256::digest(&rom).as_slice());
    assert_eq!(before.effective, before.source);
    assert_eq!(before.media_len, rom.len() as u64);
    assert_eq!(before.persistent, Sha256::digest([]).as_slice());
    std::fs::write(directory.path().join("game.nes"), b"changed").unwrap();
    assert_eq!(identity(&backend, [7; 32]).unwrap(), before);
    assert_ne!(identity(&backend, [8; 32]).unwrap().build, before.build);
}

#[test]
fn checkpoint_hash_includes_mapper_runtime_omitted_from_native_state() {
    let mut rom = build_nes_test_rom();
    rom[6] = 0x10;
    let (_directory, mut backend) = clean(&rom);
    let before = backend.encode_state_bytes().unwrap();
    let initial = checkpoint(&backend, 0, &[], [9; 32]).unwrap();
    let EmuBackend::Nes(nes) = &mut backend else {
        unreachable!()
    };
    nes.emu.bus_mut().cartridge.cpu_write(0x8000, 0x80);
    assert_eq!(backend.encode_state_bytes().unwrap(), before);
    let changed = checkpoint(&backend, 0, &[], [9; 32]).unwrap();
    assert_ne!(changed, initial);
}

#[test]
fn selected_input_delay_is_authenticated_before_any_frame_can_run() {
    use std::net::{TcpListener, TcpStream};
    use zeff_netplay::{lockstep::Player, wire};
    let (_directory, backend) = clean(&build_nes_test_rom());
    let mut configs = std::collections::BTreeSet::new();
    for frames in InputDelay::MIN..=InputDelay::MAX {
        let local =
            identity_with_delay(&backend, [7; 32], InputDelay::new(frames).unwrap()).unwrap();
        assert!(configs.insert(local.config));
        if frames == 2 {
            continue;
        }
        let remote = identity(&backend, [7; 32]).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (peer, _) = listener.accept().unwrap();
        let other = std::thread::spawn(move || wire::admit(peer, Player::Two, &remote, &[5; 32]));
        assert!(
            wire::admit(stream, Player::One, &local, &[5; 32])
                .err()
                .unwrap()
                .to_string()
                .contains("session identity mismatch")
        );
        assert!(
            other
                .join()
                .unwrap()
                .err()
                .unwrap()
                .to_string()
                .contains("session identity mismatch")
        );
    }
    assert_eq!(configs.len(), 9);
    assert_eq!(backend.frame_count(), 0);
}

#[test]
fn low_delay_contract_rejects_the_previous_two_frame_configuration_before_execution() {
    use std::net::{TcpListener, TcpStream};
    use zeff_netplay::{lockstep::Player, wire};
    let (_directory, backend) = clean(&build_nes_test_rom());
    let local = identity(&backend, [7; 32]).unwrap();
    let mut legacy = local.clone();
    let mut config = Sha256::new();
    config.update(b"ZeffNetplay-App-native-NES/two-standard-controllers/delay2to8-predict8/runtime-snapshot1/hash60/48000/stereo-f32LE/discard-and-restore/v4");
    config.update([0]);
    config.update(2u64.to_le_bytes());
    config.update(zeff_netplay::rollback::PREDICTION_WINDOW.to_le_bytes());
    config.update(TAS_DETERMINISM_ABI_ID.as_bytes());
    config.update(TAS_STATE_FORMAT_COMPATIBILITY_ID.as_bytes());
    config.update(
        backend
            .nes_tas_load_provenance()
            .unwrap()
            .load
            .sync_config_sha256,
    );
    legacy.config = config.finalize().into();
    assert_ne!(local.config, legacy.config);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (peer, _) = listener.accept().unwrap();
    let other = std::thread::spawn(move || wire::admit(peer, Player::Two, &legacy, &[5; 32]));
    assert!(wire::admit(stream, Player::One, &local, &[5; 32]).is_err());
    assert!(other.join().unwrap().is_err());
    assert_eq!(backend.frame_count(), 0);
}

#[test]
fn loaded_timing_is_resolved_and_explicitly_bound_to_session_configuration() {
    for (tag, timing) in [
        (0, TimingMode::Ntsc),
        (1, TimingMode::Pal),
        (2, TimingMode::Ntsc),
        (3, TimingMode::Dendy),
    ] {
        let mut rom = build_nes_test_rom();
        rom[7] = 0x08;
        rom[12] = tag;
        let (_directory, backend) = clean(&rom);
        assert_eq!(backend.nes().unwrap().emu.resolved_timing_mode(), timing);
        let admitted = identity(&backend, [7; 32]).unwrap();
        assert_eq!(
            admitted.config,
            session_config(
                timing,
                backend
                    .nes_tas_load_provenance()
                    .unwrap()
                    .load
                    .sync_config_sha256,
                InputDelay::default()
            )
            .unwrap()
        );
    }
    assert_ne!(
        session_config(TimingMode::Ntsc, [3; 32], InputDelay::default()).unwrap(),
        session_config(TimingMode::Pal, [3; 32], InputDelay::default()).unwrap()
    );
    for timing in [TimingMode::Ntsc, TimingMode::Pal] {
        assert_ne!(
            session_config(timing, [3; 32], InputDelay::default()).unwrap(),
            session_config(TimingMode::Dendy, [3; 32], InputDelay::default()).unwrap()
        );
    }
    assert!(session_config(TimingMode::MultiRegion, [3; 32], InputDelay::default()).is_err());
}

#[test]
fn peer_with_different_timing_configuration_is_refused_before_admission() {
    for (local, remote) in [
        (TimingMode::Ntsc, TimingMode::Pal),
        (TimingMode::Pal, TimingMode::Dendy),
        (TimingMode::Dendy, TimingMode::Ntsc),
    ] {
        assert_timing_refusal(local, remote);
    }
}

fn assert_timing_refusal(local_timing: TimingMode, remote_timing: TimingMode) {
    use std::net::{TcpListener, TcpStream};
    use zeff_netplay::{lockstep::Player, wire};

    let (_directory, backend) = clean(&build_nes_test_rom());
    let mut local = identity(&backend, [7; 32]).unwrap();
    let policy = backend
        .nes_tas_load_provenance()
        .unwrap()
        .load
        .sync_config_sha256;
    local.config = session_config(local_timing, policy, InputDelay::default()).unwrap();
    let mut remote = local.clone();
    remote.config = session_config(remote_timing, policy, InputDelay::default()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (peer_stream, _) = listener.accept().unwrap();
    let peer = std::thread::spawn(move || {
        wire::admit(peer_stream, Player::Two, &remote, &[5; 32])
            .err()
            .unwrap()
    });
    let error = wire::admit(stream, Player::One, &local, &[5; 32])
        .err()
        .unwrap();
    assert!(
        error.to_string().contains("session identity mismatch"),
        "{error:#}"
    );
    assert!(
        peer.join()
            .unwrap()
            .to_string()
            .contains("session identity mismatch")
    );
}

#[test]
fn unsupported_loader_facts_are_rejected() {
    let changes: &[fn(&mut NesTasLoadProvenance)] = &[
        |load| load.direct_nes_file = false,
        |load| load.any_mod_enabled = true,
        |load| load.any_mod_applied = true,
        |load| load.raw_source_media_sha256[0] ^= 1,
        |load| load.raw_source_media_len = 0,
        |load| load.persistent_load = NesPersistentLoadOutcome::Unknown,
        |load| load.initial_input.buttons = 1,
        |load| load.initial_input.dpad = 1,
        |load| load.initial_sample_rate = 44_100,
        |load| load.configured_sample_rate = Some(44_100),
    ];
    for change in changes {
        let (_directory, backend) = clean(&build_nes_test_rom());
        let backend = changed_load(backend, change);
        assert!(identity(&backend, [0; 32]).is_err());
    }
}

#[test]
fn preloaded_or_archive_source_is_not_admitted_as_direct_media() {
    let (_directory, backend) = clean(&build_nes_test_rom());
    let path = backend.source_path();
    let preloaded = load_backend_from_rom_source(
        ActiveSystem::Nes,
        path,
        path,
        Some(build_nes_test_rom()),
        BackendLoadConfig::default(),
    )
    .unwrap()
    .backend;
    assert!(identity(&preloaded, [0; 32]).is_err());
    let archive_path = path.with_extension("zip");
    let archived = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &archive_path,
        path,
        Some(build_nes_test_rom()),
        BackendLoadConfig::default(),
    )
    .unwrap()
    .backend;
    assert!(identity(&archived, [0; 32]).is_err());

    let unwitnessed = EmuBackend::Nes(Box::new(NesBackend::new(
        zeff_nes_core::emulator::Emulator::from_rom_data(&build_nes_test_rom()).unwrap(),
        path.to_owned(),
    )));
    assert!(identity(&unwitnessed, [0; 32]).is_err());
}

#[test]
fn runtime_mutations_and_nonstandard_hardware_are_rejected() {
    let (_directory, mut backend) = clean(&build_nes_test_rom());
    backend.set_sample_rate(44_100);
    assert!(identity(&backend, [0; 32]).is_err());

    let (_directory, mut backend) = clean(&build_nes_test_rom());
    backend.set_zapper_state(true, false, false, None);
    assert!(identity(&backend, [0; 32]).is_err());

    let (_directory, mut backend) = clean(&build_nes_test_rom());
    let EmuBackend::Nes(nes) = &mut backend else {
        unreachable!()
    };
    nes.emu.cpu_write8(0, 0x80);
    assert!(identity(&backend, [0; 32]).is_err());

    let (_directory, mut backend) = clean(&build_nes_test_rom());
    backend.step_frame();
    assert!(identity(&backend, [0; 32]).is_err());

    let (_directory, mut backend) = clean(&build_nes_test_rom());
    let EmuBackend::Nes(nes) = &mut backend else {
        unreachable!()
    };
    nes.emu.debug_suspend();
    assert!(identity(&backend, [0; 32]).is_err());

    let (_directory, mut backend) = clean(&build_nes_test_rom());
    backend.set_firmware_manifests(vec![
        zeff_emu_common::replay::ReplayFirmwareManifest::Skipped {
            firmware_id: "unsupported".to_owned(),
            compatibility_version: 1,
        },
    ]);
    assert!(identity(&backend, [0; 32]).is_err());

    let mut vs = build_nes_test_rom();
    vs[7] = 1;
    let (_directory, backend) = clean(&vs);
    assert!(identity(&backend, [0; 32]).is_err());
}

#[test]
fn battery_initial_digest_is_exact_and_later_mutations_are_rejected() {
    let rom = build_nes_battery_test_rom();
    let (_directory, backend) = clean(&rom);
    assert!(identity(&backend, [0; 32]).is_ok());
    let mut load = *backend.nes_tas_load_provenance().unwrap().load;
    load.persistent_load = NesPersistentLoadOutcome::Loaded;
    let path = backend.source_path().to_owned();
    let EmuBackend::Nes(backend) = backend else {
        unreachable!()
    };
    let mut emu = backend.emu;
    let mut persistent = emu.dump_persistent_data().unwrap();
    persistent[0] = 0x5a;
    emu.load_persistent_data(&persistent).unwrap();
    let mut backend = EmuBackend::Nes(Box::new(NesBackend::with_load_provenance(
        emu,
        path.clone(),
        path,
        load,
    )));
    let admitted = identity(&backend, [0; 32]).unwrap();
    assert_eq!(admitted.persistent, Sha256::digest(&persistent).as_slice());
    persistent[0] ^= 1;
    let EmuBackend::Nes(nes) = &mut backend else {
        unreachable!()
    };
    nes.emu.load_persistent_data(&persistent).unwrap();
    assert!(identity(&backend, [0; 32]).is_err());
}

#[test]
fn checkpoint_preserves_all_four_hash_domains_and_rejects_frame_drift() {
    let (_directory, backend) = clean(&build_nes_test_rom());
    let admitted = identity(&backend, [0; 32]).unwrap();
    let audio = [0.0, -0.0, f32::from_bits(0x7fc0_0123), 0.25];
    let Message::Checkpoint {
        logical,
        video,
        audio: digest,
        persistent,
        ..
    } = checkpoint(&backend, 0, &audio, admitted.config).unwrap()
    else {
        unreachable!()
    };
    let expected_audio: Vec<_> = audio
        .iter()
        .flat_map(|sample| sample.to_bits().to_le_bytes())
        .collect();
    assert_eq!(logical, admitted.initial);
    assert_eq!(video, Sha256::digest(backend.framebuffer()).as_slice());
    assert_eq!(digest, Sha256::digest(&expected_audio).as_slice());
    assert_eq!(persistent, admitted.persistent);
    assert!(checkpoint(&backend, 1, &[], admitted.config).is_err());
    assert_ne!(
        checkpoint(&backend, 0, &audio, [1; 32]).unwrap(),
        checkpoint(&backend, 0, &audio, admitted.config).unwrap()
    );
}

#[test]
fn runtime_audio_palette_and_debugger_controls_cannot_bypass_admission() {
    let changes: &[fn(&mut zeff_nes_core::emulator::Emulator)] = &[
        |emu| emu.set_apu_sample_generation_enabled(false),
        |emu| emu.set_apu_channel_mutes([true, false, false, false, false]),
        |emu| emu.set_sample_rate(44_100),
        |emu| emu.set_palette_mode(zeff_nes_core::hardware::ppu::NesPaletteMode::Ntsc),
        |emu| emu.add_breakpoint(0x8000),
        |emu| emu.add_one_shot_breakpoint(0x8000),
        |emu| emu.debug_step(),
        |emu| emu.add_breakpoint_after(0x8000, 10),
        |emu| emu.add_watchpoint(0x6000, zeff_nes_core::debug::WatchType::Write),
    ];
    for change in changes {
        let (_directory, mut backend) = clean(&build_nes_test_rom());
        assert!(identity(&backend, [0; 32]).is_ok());
        let EmuBackend::Nes(nes) = &mut backend else {
            unreachable!()
        };
        change(&mut nes.emu);
        assert!(identity(&backend, [0; 32]).is_err());
    }
}
