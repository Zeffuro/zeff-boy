use super::*;
use crate::debug::{CheatState, LibretroAsyncResult, poll_libretro_results_for_test};
use crate::emu_thread::shared::md_rom_cheat_tests::{codes, update};

mod transport;

fn fixture(header: u8, timing: TimingModel) -> EmuBackend {
    let mut rom = vec![0; 0x12000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x1f0] = header;
    let program: [u16; 23] = [
        0x3039, 0, 0x332, 0x33c0, 0xff, 0x40, 0x3039, 1, 0x14d2, 0x33c0, 0xff, 0x42, 0x3039, 1,
        0x1504, 0x33c0, 0xff, 0x44, 0x13f9, 0xff, 0x100, 0xff, 0x101,
    ];
    for (index, word) in program.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    rom[0x22e..0x230].copy_from_slice(&0x60f4_u16.to_be_bytes());
    let path = Path::new("database-cheats.md");
    load_backend_from_rom_source(
        ActiveSystem::Md,
        path,
        path,
        Some(rom),
        BackendLoadConfig {
            md_timing: timing,
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
fn md_database_download_result_enable_update_and_guest_reads() {
    let mut state = CheatState::new();
    state.active_system = ActiveSystem::Md;
    state.libretro_platform = crate::libretro_common::LibretroPlatform::Md;
    state.cheats_dirty = false;
    let (tx, rx) = std::sync::mpsc::channel();
    state.libretro_rx = Some(rx);
    state.libretro_busy = true;
    tx.send(LibretroAsyncResult::Downloaded {
        filename: "Synthetic database.cht".into(),
        result: Ok(concat!(
            "cheat0_desc = \"First word\"\ncheat0_code = \"AABT-AN3W\"\ncheat0_enable = false\n",
            "cheat1_desc = \"Paired words\"\ncheat1_code = \"RYLA-C60W+RYLT-C6TE\"\ncheat1_enable = false\n",
            "cheat2_desc = \"RAM byte\"\ncheat2_code = \"FF0100:A5\"\ncheat2_enable = false\n",
        ).into()),
    }).unwrap();
    drop(tx);
    poll_libretro_results_for_test(&mut state);
    assert!(!state.libretro_busy && state.libretro_rx.is_none() && state.cheats_dirty);
    assert_eq!(
        state.libretro_status.as_deref(),
        Some("Imported 3 cheat(s) from Synthetic database.cht")
    );
    assert_eq!(state.libretro_codes.len(), 3);
    assert!(state.libretro_codes.iter().all(|code| !code.enabled));
    assert!(collect_enabled_patches(&state.user_codes, &state.libretro_codes).is_empty());
    assert_eq!(state.libretro_codes[0].patches, codes("000332:6600"));
    assert_eq!(
        state.libretro_codes[1].patches,
        codes("0114D2:4E75+011504:4E75")
    );
    for code in &mut state.libretro_codes {
        code.enabled = true;
    }
    let saved = export_cht_file(&state.libretro_codes);
    let restored = parse_cht_file_for_system(&saved, ActiveSystem::Md);
    let patches = collect_enabled_patches(&state.user_codes, &restored);
    for header in *b"UJE" {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut backend = fixture(header, timing);
            assert!(
                backend
                    .capabilities()
                    .cheat_features
                    .supports_libretro_database
            );
            let source_hash = backend.rom_hash();
            let mut current = Vec::new();
            assert!(update(&mut backend, &mut current, patches.clone()).is_none());
            assert_eq!(current, patches);
            let result = step(&mut backend, 2, &current);
            assert!(result.runtime_fault.is_none() && result.replay_error.is_none());
            assert_eq!(result.advanced_frames, 2);
            for (address, value) in [
                (0xff0040, 0x66),
                (0xff0041, 0),
                (0xff0042, 0x4e),
                (0xff0043, 0x75),
                (0xff0044, 0x4e),
                (0xff0045, 0x75),
                (0xff0101, 0xa5),
            ] {
                assert_eq!(byte(&backend, address), value);
            }
            assert_eq!(backend.rom_hash(), source_hash);
            let EmuBackend::Md(md) = &backend else {
                unreachable!()
            };
            assert_eq!(&md.core().rom_bytes()[0x332..0x334], &[0, 0]);
        }
    }
}
