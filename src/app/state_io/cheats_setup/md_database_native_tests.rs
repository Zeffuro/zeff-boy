use super::*;
use crate::cheats::{export_cht_file, load_game_cheats};
use crate::debug::{LibretroAsyncResult, poll_libretro_results_for_test};

fn backend(marker: u8) -> (EmuBackend, u32) {
    let mut rom = vec![0; 0x400];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x1f0] = b'U';
    rom[0x200..0x204].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
    rom[0x3ff] = marker;
    let crc = crc32fast::hash(&rom);
    let path = Path::new("database-app.md");
    let backend = crate::emu_backend::load_backend_from_rom_source(
        ActiveSystem::Md,
        path,
        path,
        Some(rom),
        crate::emu_backend::BackendLoadConfig {
            md_load_save_ram: false,
            md_host_persistence: false,
            ..Default::default()
        },
    )
    .unwrap()
    .backend;
    (backend, crc)
}

#[test]
#[ignore = "requires owned ZEFF_CONFIG_DIR and ZEFF_MUTE_AUDIO=1"]
fn md_database_headless_app_setup_import_save_switch_and_crc_reload() {
    let config =
        std::env::var_os("ZEFF_CONFIG_DIR").expect("qualification requires owned config root");
    assert_eq!(std::env::var("ZEFF_MUTE_AUDIO").as_deref(), Ok("1"));
    let config = std::path::PathBuf::from(config);
    assert!(config.is_absolute());
    assert_eq!(crate::settings::Settings::settings_dir(), config);
    let (first, first_crc) = backend(1);
    let (second, second_crc) = backend(2);
    assert_ne!(first_crc, second_crc);
    let directories = [first_crc, second_crc]
        .map(|crc| config.join("cheats/md/libretro").join(format!("{crc:08X}")));
    for directory in &directories {
        assert!(
            !directory.exists(),
            "qualification CRC namespace must be unused"
        );
    }
    for crc in [first_crc, second_crc] {
        let (user, database) = load_game_cheats(ActiveSystem::Md, None, Some(crc));
        assert!(
            user.is_empty() && database.is_empty(),
            "qualification CRC namespace has legacy records"
        );
    }
    let mut settings = crate::settings::Settings::default();
    settings.ui.check_for_updates = false;
    settings.emulation.save_recovery_state = false;
    settings.emulation.resume_recovery_state = false;
    let mut app = crate::app::construct::create(None, settings, None, false);
    app.active_system = ActiveSystem::Md;
    app.setup_cheats_for_rom(ActiveSystem::Md, Path::new("First database.md"), &first);
    assert_eq!(app.debug_windows.cheat.rom_crc32, Some(first_crc));
    assert_eq!(
        app.debug_windows.cheat.libretro_platform,
        LibretroPlatform::Md
    );
    let (tx, rx) = std::sync::mpsc::channel();
    app.debug_windows.cheat.libretro_rx = Some(rx);
    app.debug_windows.cheat.libretro_busy = true;
    tx.send(LibretroAsyncResult::Downloaded {
        filename: "Synthetic database.cht".into(),
        result: Ok(concat!(
            "cheat0_desc = \"First word\"\ncheat0_code = \"AABT-AN3W\"\ncheat0_enable = false\n",
            "cheat1_desc = \"Paired words\"\ncheat1_code = \"RYLA-C60W+RYLT-C6TE\"\ncheat1_enable = false\n",
        ).into()),
    }).unwrap();
    drop(tx);
    poll_libretro_results_for_test(&mut app.debug_windows.cheat);
    assert_eq!(app.debug_windows.cheat.libretro_codes.len(), 2);
    assert!(
        app.debug_windows
            .cheat
            .libretro_codes
            .iter()
            .all(|code| !code.enabled)
    );
    app.debug_windows.cheat.libretro_codes[0].enabled = true;
    let expected = export_cht_file(&app.debug_windows.cheat.libretro_codes);
    app.save_current_cheats();
    assert_eq!(
        std::fs::read_to_string(directories[0].join("libretro.cht")).unwrap(),
        expected
    );
    let (stale, rx) = std::sync::mpsc::channel();
    app.debug_windows.cheat.libretro_rx = Some(rx);
    app.debug_windows.cheat.libretro_busy = true;
    app.debug_windows.cheat.libretro_file_list = Some(vec!["Previous.cht".into()]);
    app.debug_windows.cheat.libretro_results = vec!["Previous.cht".into()];
    app.debug_windows.cheat.libretro_status = Some("Previous status".into());
    stale
        .send(LibretroAsyncResult::Downloaded {
            filename: "Previous.cht".into(),
            result: Ok("cheat0_code = \"FF0100:22\"\n".into()),
        })
        .unwrap();
    app.setup_cheats_for_rom(ActiveSystem::Md, Path::new("Second database.md"), &second);
    let state = &app.debug_windows.cheat;
    assert_eq!(state.rom_crc32, Some(second_crc));
    assert_eq!(state.rom_title.as_deref(), Some("Second database"));
    assert!(state.user_codes.is_empty() && state.libretro_codes.is_empty());
    assert!(state.libretro_file_list.is_none() && state.libretro_results.is_empty());
    assert!(state.libretro_status.is_none() && state.libretro_rx.is_none() && !state.libretro_busy);
    assert!(state.cheats_dirty);
    assert!(
        stale
            .send(LibretroAsyncResult::FileList(Ok(vec![])))
            .is_err()
    );
    app.setup_cheats_for_rom(ActiveSystem::Md, Path::new("Renamed database.md"), &first);
    let state = &app.debug_windows.cheat;
    assert_eq!(state.rom_title.as_deref(), Some("Renamed database"));
    assert_eq!(state.rom_crc32, Some(first_crc));
    assert_eq!(export_cht_file(&state.libretro_codes), expected);
    assert!(state.libretro_codes[0].enabled && !state.libretro_codes[1].enabled);
    assert_eq!(
        export_cht_file(&load_game_cheats(ActiveSystem::Md, Some("Other name"), Some(first_crc)).1),
        expected
    );
    assert!(app.emu_thread.is_none() && app.gfx.is_none());
    app.debug_windows.cheat.rom_title = None;
    drop(app);
    std::fs::remove_dir_all(&directories[0]).unwrap();
    assert!(!directories[1].exists());
}
