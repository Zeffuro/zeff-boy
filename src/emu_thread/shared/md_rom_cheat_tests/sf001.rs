use super::*;
use crate::emu_backend::{BackendLoadConfig, load_backend_from_rom_source};
use zeff_emu_common::time::Reset;

fn fixture(header: u8, checksum: u16) -> EmuBackend {
    let mut rom = vec![0; 0x400000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x180..0x18e].copy_from_slice(b"SF-001\0\0\0\0\0\0\0\0");
    rom[0x18e..0x190].copy_from_slice(&checksum.to_be_bytes());
    rom[0x1b0..0x1bc].fill(b' ');
    rom[0x1f0] = header;
    rom[0x1000..0x1002].copy_from_slice(&[0x11, 0x22]);
    rom[0x381000..0x381002].copy_from_slice(&[0x33, 0x44]);
    let program: [u16; 12] = [
        0x13fc, 0x80, 0, 0xe00, 0x3039, 0, 0x1000, 0x33c0, 0xff, 0x40, 0x60fe, 0x4e71,
    ];
    for base in [0x200, 0x380200] {
        for (index, word) in program.iter().enumerate() {
            rom[base + index * 2..base + index * 2 + 2].copy_from_slice(&word.to_be_bytes());
        }
    }
    let sum = rom[0x200..]
        .as_chunks::<2>()
        .0
        .iter()
        .fold(0_u16, |sum, p| {
            sum.wrapping_add(u16::from_be_bytes([p[0], p[1]]))
        });
    rom[0x3ffffe..].copy_from_slice(&checksum.wrapping_sub(sum).to_be_bytes());
    let path = std::path::Path::new("sf001-cheats.md");
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
fn md_sf001_rom_cheats_reach_normal_worker_watchpoint_and_receiver_state() {
    let imported = parse_cht_file_for_system(
        "cheat0_code = \"381000:A55A+001000:C77C\"\ncheat0_enable = true\n",
        ActiveSystem::Md,
    );
    let roundtrip = parse_cht_file_for_system(&export_cht_file(&imported), ActiveSystem::Md);
    assert_eq!(roundtrip[0].patches, imported[0].patches);
    for header in *b"UJE" {
        for checksum in [0x3e08, 0x0836] {
            let mut backend = fixture(header, checksum);
            let caps = backend.capabilities();
            assert!(caps.cheat_features.supports_rom_patches);
            assert!(caps.supports_instruction_trace && caps.supports_debugger);
            assert_eq!(caps.save_ram_kind.size(), 0x10000);
            let original = machine(&backend).rom_bytes().to_vec();
            let hash = backend.rom_hash();
            let mut current = Vec::new();
            assert!(
                update(
                    &mut backend,
                    &mut current,
                    collect_enabled_patches(&roundtrip, &[])
                )
                .is_none()
            );
            assert_eq!(machine(&backend).peek(0x1000), Some(0xc7));
            let mut expected = machine(&backend).clone();
            for _ in 0..3 {
                expected.step_instruction().unwrap();
            }
            let mut actions = DebugUiActions::none();
            actions.add_watchpoint = Some((0xff0041, 0xff0041, WatchType::Write));
            actions.trace_enabled = Some(true);
            let mut fault = WorkerRuntimeFault::default();
            let result = deliver(&mut backend, 1, actions, false, false, &mut fault);
            compare(&backend, &mut expected, &result, 0);
            assert_eq!(machine(&backend).peek(0xff0040), Some(0xa5));
            let hit = result.ui_data.cpu_debug.unwrap().hit_watchpoint.unwrap();
            assert_eq!((hit.address, hit.new_value), (0xff0041, Some(0x5a)));
            assert_eq!(machine(&backend).peek(0x1000), Some(0xa5));
            assert_eq!(machine(&backend).peek(0x381000), Some(0xa5));
            let raw = machine(&backend).encode_state().unwrap();
            let before = machine(&backend).clone();
            let old = current.clone();
            assert!(matches!(
                update(
                    &mut backend,
                    &mut current,
                    codes("381000:FFFF+400000:FF+FF0050:FF")
                ),
                Some(EmuResponse::CheatsUpdateFailed(_))
            ));
            assert_eq!(current, old);
            assert_eq!(machine(&backend), &before);
            assert!(update(&mut backend, &mut current, codes("381000:B66B")).is_none());
            backend.load_state_from_bytes(raw).unwrap();
            assert_eq!(machine(&backend).peek(0x1000), Some(0xb6));
            assert!(update(&mut backend, &mut current, Vec::new()).is_none());
            assert_eq!(machine(&backend).peek(0x1000), Some(0x33));
            Reset::reset(&mut backend);
            assert_eq!(machine(&backend).peek(0x1000), Some(0x11));
            assert_eq!(machine(&backend).rom_bytes(), original);
            assert_eq!(backend.rom_hash(), hash);
        }
    }
}
