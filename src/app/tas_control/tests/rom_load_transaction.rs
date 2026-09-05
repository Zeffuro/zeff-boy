use std::path::PathBuf;

use super::harness::app_with_worker;
use crate::emu_backend::{ActiveSystem, EmuBackend, PceBackend};
use crate::emu_thread::EmuThread;

#[test]
fn missing_direct_pce_cd_load_keeps_the_running_session() {
    let root = crate::test_support::test_directory("missing-direct-pce-cd-load").unwrap();
    let active_path = root.path().join("active.pce");
    let mut rom = vec![0xEA; 0x2000];
    rom[..4].copy_from_slice(&[0xD4, 0xEA, 0x80, 0xFD]);
    rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
    std::fs::write(&active_path, &rom).unwrap();
    let backend = EmuBackend::from_pce(PceBackend::new(rom, PathBuf::from("active.pce")).unwrap());
    let mut app = app_with_worker(
        EmuThread::spawn(backend, false),
        137,
        ActiveSystem::Pce,
        active_path.clone(),
    );
    let missing_path = root.path().join("missing.cue");

    app.load_rom(&missing_path);

    assert!(app.emu_thread.is_some());
    assert_eq!(app.active_system, ActiveSystem::Pce);
    assert_eq!(
        app.rom_info.source_path.as_deref(),
        Some(active_path.as_path())
    );
    assert_eq!(
        app.rom_info.rom_path.as_deref(),
        Some(active_path.as_path())
    );
    assert!(app.pending_rom_preparation.is_none());
}
