use super::*;
use crate::emu_backend::{BackendLoadConfig, load_backend_from_rom_source};

fn fixture(header: u8, sf004: bool) -> EmuBackend {
    let mut rom = vec![0; if sf004 { 0x200000 } else { 0x100000 }];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x1f0] = header;
    let mut program = Vec::new();
    if sf004 {
        rom[0x180..0x18e].copy_from_slice(b"SF-004        ");
        rom[0x1a0..0x1a8].copy_from_slice(&[0, 0, 0, 0, 0, 3, 0xff, 0xff]);
        rom[0x1b0..0x1bc].fill(b' ');
        program.extend([0x13fc, 0x60, 0, 0xf01]);
    } else {
        rom[0x1b0..0x1bc].copy_from_slice(b"RA\xf8\x20\x00\x20\x00\x01\x00\x20\x3f\xff");
    }
    program.extend([0x3039, 0, 0x1000, 0x33c0, 0xff, 0x40, 0x60fe, 0x4e71]);
    for base in (0..rom.len()).step_by(0x40000) {
        for (index, word) in program.iter().enumerate() {
            rom[base + 0x200 + index * 2..base + 0x202 + index * 2]
                .copy_from_slice(&u16::to_be_bytes(*word));
        }
    }
    let path = std::path::Path::new("save-mapper-cheats.md");
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

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_rom_save_and_sf004_cheats_reach_normal_worker_watchpoints_state_and_reset() {
    for header in *b"UJE" {
        for sf004 in [false, true] {
            let mut backend = fixture(header, sf004);
            let caps = backend.capabilities();
            assert!(caps.cheat_features.supports_rom_patches);
            assert!(caps.save_ram_kind.is_persistent());
            assert_eq!(
                caps.save_ram_kind.size(),
                if sf004 { 0x8000 } else { 0x2000 }
            );
            assert!(caps.supports_instruction_trace && caps.supports_debugger);
            let source = machine(&backend).rom_bytes().to_vec();
            let hash = backend.rom_hash();
            let offset = if sf004 { "181000" } else { "001000" };
            let mut current = Vec::new();
            assert!(update(&mut backend, &mut current, codes(&format!("{offset}:A55A"))).is_none());
            let mut expected = machine(&backend).clone();
            for _ in 0..if sf004 { 3 } else { 2 } {
                expected.step_instruction().unwrap();
            }
            let mut actions = DebugUiActions::none();
            actions.add_watchpoint = Some((0xff0041, 0xff0041, WatchType::Write));
            actions.trace_enabled = Some(true);
            let mut fault = WorkerRuntimeFault::default();
            let result = deliver(&mut backend, 1, actions, false, false, &mut fault);
            compare(&backend, &mut expected, &result, 0);
            assert_eq!(machine(&backend).peek(0xff0040), Some(0xa5));
            assert_eq!(machine(&backend).peek(0xff0041), Some(0x5a));
            let hit = result.ui_data.cpu_debug.unwrap().hit_watchpoint.unwrap();
            assert_eq!((hit.address, hit.new_value), (0xff0041, Some(0x5a)));
            let state = machine(&backend).encode_state().unwrap();
            let old = current.clone();
            let before = machine(&backend).clone();
            assert!(matches!(
                update(
                    &mut backend,
                    &mut current,
                    codes(&format!("{offset}:FFFF+300000:FF+FF0050:FF"))
                ),
                Some(EmuResponse::CheatsUpdateFailed(_))
            ));
            assert_eq!(current, old);
            assert_eq!(machine(&backend), &before);
            assert!(update(&mut backend, &mut current, codes(&format!("{offset}:C77C"))).is_none());
            backend.load_state_from_bytes(state).unwrap();
            assert_eq!(machine(&backend).peek(0x1000), Some(0xc7));
            assert!(update(&mut backend, &mut current, Vec::new()).is_none());
            assert_eq!(machine(&backend).peek(0x1000), Some(0));
            zeff_emu_common::time::Reset::reset(&mut backend);
            assert_eq!(machine(&backend).registers().pc, 0x200);
            assert_eq!(machine(&backend).rom_bytes(), source);
            assert_eq!(backend.rom_hash(), hash);
        }
    }
}
