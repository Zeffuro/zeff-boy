use super::*;

mod rom_breakpoints;

fn fixture(header: u8) -> EmuBackend {
    let mut rom = vec![0; 0x900004];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA SSF        ");
    rom[0x1f0] = header;
    rom[0x800040..0x800042].copy_from_slice(&[0x12, 0x34]);
    rom[0x880040..0x880042].copy_from_slice(&[0x56, 0x78]);
    let program: [u16; 34] = [
        0x13fc, 0x80, 0xc0, 0x11, 0x13fc, 4, 0xc0, 0x11, 0x13fc, 0x90, 0xc0, 0x11, 0x13fc, 16,
        0xa1, 0x30f3, 0x3039, 8, 0x40, 0x33c0, 0xff, 0x40, 0x13fc, 17, 0xa1, 0x30f3, 0x3039, 8,
        0x40, 0x33c0, 0xff, 0x42, 0x60fe, 0x4e71,
    ];
    for (index, word) in program.iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let path = std::path::Path::new("large-mapper-rom-cheats.md");
    load_backend_from_rom_source(
        ActiveSystem::Md,
        path,
        path,
        Some(rom),
        BackendLoadConfig {
            md_load_save_ram: false,
            md_host_persistence: false,
            ..Default::default()
        },
    )
    .unwrap()
    .backend
}

fn word(backend: &EmuBackend, address: u32) -> u16 {
    u16::from_be_bytes([
        machine(backend).peek(address).unwrap(),
        machine(backend).peek(address + 1).unwrap(),
    ])
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_large_mapper_rom_cheats_reach_normal_worker_banks_restore_and_reset_atomically() {
    let imported = parse_cht_file_for_system(
        "cheat0_code = \"ROM:00800040:A55A+ROM:00880040:C77C\"\ncheat0_enable = true\n",
        ActiveSystem::Md,
    );
    assert_eq!(imported.len(), 1);
    let roundtrip = parse_cht_file_for_system(&export_cht_file(&imported), ActiveSystem::Md);
    assert_eq!(roundtrip[0].patches, imported[0].patches);
    let patches = collect_enabled_patches(&roundtrip, &[]);
    for header in *b"UJE" {
        let mut backend = fixture(header);
        let caps = backend.capabilities();
        assert!(caps.cheat_features.supports_rom_patches);
        assert!(caps.supports_memory_watchpoints && caps.supports_debugger);
        let hash = backend.rom_hash();
        let source = machine(&backend).rom_bytes().to_vec();
        let mut current = Vec::new();
        assert!(update(&mut backend, &mut current, patches.clone()).is_none());
        let mut expected = machine(&backend).clone();
        for _ in 0..6 {
            expected.step_instruction().unwrap();
        }
        let mut actions = DebugUiActions::none();
        actions.add_watchpoint = Some((0xff0040, 0xff0043, WatchType::Write));
        let mut fault = WorkerRuntimeFault::default();
        let result = deliver(&mut backend, 1, actions, false, false, &mut fault);
        compare(&backend, &mut expected, &result, 0);
        assert!(backend.is_suspended());
        assert_eq!(word(&backend, 0x080040), 0xa55a);
        assert_eq!(word(&backend, 0xff0040), 0xa55a);
        assert_eq!(machine(&backend).registers().pc, 0x22c);
        let state = machine(&backend).encode_state().unwrap();
        let frontend = machine(&backend).encode_frontend_state().unwrap();
        assert_eq!(u32::from_le_bytes(state[8..12].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(frontend[8..12].try_into().unwrap()), 1);
        let before = machine(&backend).clone();
        let previous = current.clone();
        for text in [
            "ROM:00800040:FFFF+ROM:00900004:00+FF0050:FF",
            "000004:0040+000006:0000",
        ] {
            assert!(matches!(
                update(&mut backend, &mut current, codes(text)),
                Some(EmuResponse::CheatsUpdateFailed(_))
            ));
            assert_eq!(current, previous);
            assert_eq!(machine(&backend), &before);
            assert_eq!(machine(&backend).peek(0xff0050), Some(0));
        }
        for _ in 0..3 {
            expected.step_instruction().unwrap();
        }
        let result = deliver(
            &mut backend,
            1,
            DebugUiActions::none(),
            false,
            true,
            &mut fault,
        );
        compare(&backend, &mut expected, &result, 0);
        assert_eq!(word(&backend, 0x080040), 0xc77c);
        assert_eq!(word(&backend, 0xff0042), 0xc77c);
        assert_eq!(machine(&backend).registers().pc, 0x240);

        assert!(update(&mut backend, &mut current, Vec::new()).is_none());
        backend.load_state_from_bytes(state.clone()).unwrap();
        expected.clear_rom_patches();
        expected.decode_state(&state).unwrap();
        assert_eq!(word(&backend, 0x080040), 0x1234);
        assert_eq!(word(&backend, 0xff0040), 0xa55a);
        for _ in 0..3 {
            expected.step_instruction().unwrap();
        }
        let result = deliver(
            &mut backend,
            1,
            DebugUiActions::none(),
            false,
            false,
            &mut fault,
        );
        compare(&backend, &mut expected, &result, 0);
        assert_eq!(word(&backend, 0xff0042), 0x5678);
        assert!(update(&mut backend, &mut current, patches.clone()).is_none());
        backend.reset();
        expected.install_rom_patches(&patches).unwrap();
        expected.reset();
        for _ in 0..6 {
            expected.step_instruction().unwrap();
        }
        let result = deliver(
            &mut backend,
            1,
            DebugUiActions::none(),
            false,
            false,
            &mut fault,
        );
        compare(&backend, &mut expected, &result, 0);
        assert_eq!(word(&backend, 0xff0040), 0xa55a);
        let mut actions = DebugUiActions::none();
        actions
            .remove_watchpoints
            .push((0xff0040, 0xff0043, WatchType::Write));
        expected.step_field().unwrap();
        let result = deliver(&mut backend, 1, actions, false, true, &mut fault);
        assert!(result.audio_samples.iter().any(|sample| *sample != 0.0));
        compare(&backend, &mut expected, &result, 1);
        assert_eq!(word(&backend, 0xff0042), 0xc77c);
        assert_eq!(backend.rom_hash(), hash);
        assert_eq!(machine(&backend).rom_bytes(), source);
    }
}
