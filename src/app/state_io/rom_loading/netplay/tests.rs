use crate::app::tas_control::tests::harness::app_with_worker;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_thread::EmuThread;
use std::io::Write as _;
use std::path::Path;

#[test]
fn native_zip_netplay_reloads_battery_data_flushed_by_stopping() {
    let directory = crate::test_support::test_directory("netplay-zip-battery-flush").unwrap();
    let source = directory.path().join("battery.zip");
    let rom = source.join("folder/battery.nes");
    let bytes = crate::test_support::build_nes_battery_test_rom();
    archive(&source, "folder/battery.nes", &bytes);
    let save = crate::save_paths::sram_path_for_rom(&rom);
    std::fs::write(
        &save,
        crate::test_support::nes_battery_test_bytes(&bytes, 0x11),
    )
    .unwrap();
    let config = BackendLoadConfig {
        sample_rate: Some(48_000),
        ..Default::default()
    };
    let mut backend = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &source,
        &rom,
        Some(bytes.clone()),
        config.clone(),
    )
    .unwrap()
    .backend;
    let hash = backend.rom_hash();
    let updated = crate::test_support::nes_battery_test_bytes(&bytes, 0xa7);
    let crate::emu_backend::EmuBackend::Nes(nes) = &mut backend else {
        unreachable!()
    };
    nes.emu.load_persistent_data(&updated).unwrap();
    let mut app = app_with_worker(
        EmuThread::spawn(backend, false),
        11,
        ActiveSystem::Nes,
        rom.clone(),
    );
    app.rom_info.source_path = Some(source.clone());
    app.rom_info.rom_hash = Some(hash);
    app.prepare_netplay_game(zeff_netplay::rollback::InputDelay::default())
        .unwrap();
    assert_eq!(std::fs::read(&save).unwrap(), updated);
    let expected =
        load_backend_from_rom_source(ActiveSystem::Nes, &source, &rom, Some(bytes), config)
            .unwrap()
            .backend
            .encode_state_bytes()
            .unwrap();
    let worker = app.emu_thread.as_ref().unwrap();
    worker.send(crate::emu_thread::EmuCommand::CaptureStateBytes);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match worker.poll_response() {
            crate::emu_thread::EmuResponsePoll::Response(response) => {
                if let crate::emu_thread::EmuResponse::StateCaptured(actual) = *response {
                    assert_eq!(actual, expected);
                    break;
                }
            }
            crate::emu_thread::EmuResponsePoll::Disconnected => {
                panic!("prepared worker disconnected")
            }
            crate::emu_thread::EmuResponsePoll::Empty => {}
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    app.stop_emu_thread();
}

fn archive(path: &Path, member: &str, bytes: &[u8]) {
    let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    writer
        .start_file(member, zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(bytes).unwrap();
    writer.finish().unwrap();
}

#[test]
fn native_netplay_reopens_selected_zip_and_rejects_replacement_before_stopping() {
    for (system, extension) in [
        (ActiveSystem::Nes, "nes"),
        (ActiveSystem::MasterSystem, "sms"),
        (ActiveSystem::Sg1000, "sg"),
        (ActiveSystem::Pce, "pce"),
    ] {
        let directory = crate::test_support::test_directory("app-netplay-selected-zip").unwrap();
        let bytes = match system {
            ActiveSystem::Nes => crate::test_support::build_nes_test_rom(),
            ActiveSystem::Pce => crate::emu_backend::pce::netplay_fixture_hucard(),
            _ => {
                let mut bytes = vec![0; 32768];
                bytes[..3].copy_from_slice(&[0xc3, 0, 0]);
                bytes
            }
        };
        let source = directory.path().join("chosen.zip");
        let member = format!("folder/selected.{extension}");
        let rom = source.join(&member);
        archive(&source, &member, &bytes);
        let backend = load_backend_from_rom_source(
            system,
            &source,
            &rom,
            Some(bytes.clone()),
            BackendLoadConfig {
                sample_rate: Some(48_000),
                pce_netplay: system == ActiveSystem::Pce,
                ..Default::default()
            },
        )
        .unwrap()
        .backend;
        let hash = backend.rom_hash();
        let mut app = app_with_worker(EmuThread::spawn(backend, false), 11, system, rom.clone());
        app.rom_info.source_path = Some(source.clone());
        app.rom_info.rom_hash = Some(hash);
        let mut replaced = bytes.clone();
        *replaced.last_mut().unwrap() ^= 1;
        archive(&source, &member, &replaced);
        assert!(
            app.prepare_netplay_game(zeff_netplay::rollback::InputDelay::default())
                .unwrap_err()
                .to_string()
                .contains("changed since loading")
        );
        assert!(app.emu_thread.is_some(), "{system:?}");
        archive(&source, &format!("other.{extension}"), &bytes);
        assert!(
            app.prepare_netplay_game(zeff_netplay::rollback::InputDelay::default())
                .is_err()
        );
        assert!(app.emu_thread.is_some());
        archive(&source, &member, &bytes);
        app.prepare_netplay_game(zeff_netplay::rollback::InputDelay::default())
            .unwrap();
        assert_eq!(app.rom_info.source_path.as_ref(), Some(&source));
        assert_eq!(app.rom_info.rom_path.as_ref(), Some(&rom));
        assert_eq!(app.rom_info.rom_hash, Some(hash));
        app.stop_emu_thread();
    }
}
