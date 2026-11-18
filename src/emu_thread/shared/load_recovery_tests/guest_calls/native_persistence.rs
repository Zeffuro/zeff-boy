use super::*;
use std::path::Path;

fn persistent_backend(path: &Path) -> (EmuBackend, Vec<u8>) {
    let source = guest_call_backend();
    let EmuBackend::Md(md) = source else {
        unreachable!()
    };
    let mut rom = md.core().rom_bytes().to_vec();
    rom[0x1b0..0x1bc]
        .copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20, 0x3f, 0xff]);
    for (address, words) in [
        (0x1000, &[0x13fc_u16, 0x00a5, 0x0020, 0x0001, 0x4e75][..]),
        (
            0x1200,
            &[
                0x13fc, 0x005a, 0x0020, 0x0001, 0x13fc, 0x006d, 0x00b0, 0x0000, 0x4e75,
            ][..],
        ),
    ] {
        for (index, word) in words.iter().enumerate() {
            let offset = address + index * 2;
            rom[offset..offset + 2].copy_from_slice(&word.to_be_bytes());
        }
    }
    let original = rom.clone();
    let mut backend = load_backend_from_rom_source(
        ActiveSystem::Md,
        path,
        path,
        Some(rom),
        BackendLoadConfig {
            sample_rate: Some(48_000),
            md_load_save_ram: false,
            md_host_persistence: true,
            ..Default::default()
        },
    )
    .unwrap()
    .backend;
    let EmuBackend::Md(md) = &mut backend else {
        unreachable!()
    };
    for _ in 0..6 {
        md.test_normal_core_mut()
            .unwrap()
            .step_instruction()
            .unwrap();
    }
    backend.debug_suspend();
    assert!(backend.save_ram_kind().is_persistent());
    (backend, original)
}

fn front(backend: &EmuBackend) -> Vec<u8> {
    let EmuBackend::Md(md) = backend else {
        unreachable!()
    };
    md.core().encode_frontend_state().unwrap()
}

fn cells(backend: &EmuBackend) -> Vec<u8> {
    let EmuBackend::Md(md) = backend else {
        unreachable!()
    };
    md.core().dump_save_ram().unwrap()
}

#[test]
#[ignore = "requires owned ZEFF_CONFIG_DIR and ZEFF_MUTE_AUDIO=1"]
fn md_guest_call_battery_publication_undo_and_failed_callee_are_atomic() {
    let config = std::path::PathBuf::from(
        std::env::var_os("ZEFF_CONFIG_DIR").expect("requires owned test configuration"),
    );
    assert!(config.is_absolute());
    assert_eq!(crate::settings::Settings::settings_dir(), config);
    assert_eq!(std::env::var("ZEFF_MUTE_AUDIO").as_deref(), Ok("1"));
    let directory = config.join(format!(
        "md-guest-call-persistence-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    assert!(directory.starts_with(&config));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("persistent-call.md");
    let save = crate::save_paths::sram_path_for_rom(&path);
    assert!(!save.exists());
    let (mut backend, original) = persistent_backend(&path);
    let before = front(&backend);
    let baseline = cells(&backend);
    assert_eq!(baseline, vec![0xff; 8192]);
    backend.flush_battery_sram().unwrap();
    assert_eq!(std::fs::read(&save).unwrap(), baseline);
    let request = |target, instruction_budget| GuestCallRequest {
        name: "Persistent call".into(),
        target,
        storage_offset: Some(u64::from(target)),
        explicit_overlay: false,
        exec_mode: crate::symbols::ExecMode::M68000,
        instruction_budget,
    };
    let (instructions, token) = backend.execute_guest_call(&request(0x1000, 2)).unwrap();
    assert_eq!(instructions, 2);
    assert!(backend.is_suspended());
    let changed = cells(&backend);
    assert_eq!(changed[0], 0xa5);
    assert_eq!(&changed[1..], &baseline[1..]);
    backend.flush_battery_sram().unwrap();
    assert_eq!(std::fs::read(&save).unwrap(), changed);
    backend.undo_guest_call(token).unwrap();
    assert!(backend.is_suspended());
    assert_eq!(front(&backend), before);
    assert_eq!(cells(&backend), baseline);
    backend.flush_battery_sram().unwrap();
    assert_eq!(std::fs::read(&save).unwrap(), baseline);
    let file_before = std::fs::read(&save).unwrap();
    let error = backend.execute_guest_call(&request(0x1200, 3)).unwrap_err();
    assert!(error.to_string().contains("machine fault"));
    assert!(error.to_string().contains("state restored"));
    assert_eq!(front(&backend), before);
    assert_eq!(cells(&backend), baseline);
    assert_eq!(std::fs::read(&save).unwrap(), file_before);
    backend.flush_battery_sram().unwrap();
    assert_eq!(std::fs::read(&save).unwrap(), file_before);
    let EmuBackend::Md(md) = &backend else {
        unreachable!()
    };
    assert_eq!(md.core().rom_bytes(), original);
    drop(backend);
    assert_eq!(
        directory.canonicalize().unwrap().parent(),
        Some(config.canonicalize().unwrap().as_path())
    );
    std::fs::remove_dir_all(&directory).unwrap();
}
