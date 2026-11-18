use super::md_pc_breakpoint_tests::{compare, deliver, fixture_with_timing, machine};
use super::*;
use crate::cheats::{collect_enabled_patches, export_cht_file, parse_cht_file_for_system};
use crate::debug::DebugUiActions;
use crate::emu_backend::{ActiveSystem, MdBackend};
use crate::emu_thread::EmuCommand;
use zeff_emu_common::debug::WatchType;
use zeff_md_core::{Emulator, LoadConfig, Region, TimingModel};

mod save_mapper;
mod sf001;

pub(super) fn update(
    backend: &mut EmuBackend,
    current: &mut Vec<crate::cheats::CheatPatch>,
    candidate: Vec<crate::cheats::CheatPatch>,
) -> Option<EmuResponse> {
    let result = super::super::commands::CommonCommandContext {
        backend,
        shared_framebuffer: &Default::default(),
        uncapped_mode: &mut false,
        rewind_buffer: &mut zeff_emu_common::rewind::RewindBuffer::new(0, 1),
        last_cheats: current,
        audio_recording_capture: &mut Default::default(),
        pending_audio_discontinuities: &mut Vec::new(),
        runtime_fault: &mut WorkerRuntimeFault::default(),
    }
    .dispatch(EmuCommand::UpdateCheats(candidate));
    let super::super::commands::CommonCommandDispatch::Handled(effects) = result else {
        panic!("cheat command not handled");
    };
    effects.response
}

pub(super) fn codes(text: &str) -> Vec<crate::cheats::CheatPatch> {
    crate::cheats::parse_cheat_for_system(text, ActiveSystem::Md)
        .unwrap()
        .0
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_rom_game_genie_and_watchpoints_reach_normal_worker_and_reject_updates_atomically() {
    let imported = parse_cht_file_for_system(
        "cheat0_desc = \"ROM word\"\ncheat0_code = \"MJBT-AMNA\"\ncheat0_enable = true\n",
        ActiveSystem::Md,
    );
    assert_eq!(imported.len(), 1);
    assert_eq!(
        parse_cht_file_for_system(&export_cht_file(&imported), ActiveSystem::Md)[0].patches,
        imported[0].patches
    );
    let patches = collect_enabled_patches(&imported, &[]);
    for header in *b"UJE" {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut backend = fixture_with_timing(
                header,
                &[0x3039, 0, 0x380, 0x33c0, 0xff, 0x40, 0x60f2, 0x4e71],
                timing,
            );
            let mut current = Vec::new();
            assert!(update(&mut backend, &mut current, patches.clone()).is_none());
            let mut expected = machine(&backend).clone();
            expected.step_instruction().unwrap();
            let mut fault = WorkerRuntimeFault::default();
            let mut actions = DebugUiActions::none();
            actions.add_watchpoint = Some((0x380, 0x381, WatchType::Read));
            let result = deliver(&mut backend, 1, actions, false, false, &mut fault);
            compare(&backend, &mut expected, &result, 0);
            assert_eq!(machine(&backend).registers().d[0] & 0xffff, 0xa55a);
            let hit = result
                .ui_data
                .cpu_debug
                .as_ref()
                .unwrap()
                .hit_watchpoint
                .as_ref()
                .unwrap();
            assert_eq!(
                (hit.address, hit.old_value, hit.new_value),
                (0x380, None, Some(0xa5))
            );
            let caps = result.ui_data.core_features.as_ref().unwrap();
            assert!(caps.supports_memory_watchpoints && caps.cheat_features.supports_rom_patches);
            assert!(caps.supports_debugger && caps.cheat_features.supports_libretro_database);

            expected.step_instruction().unwrap();
            let mut actions = DebugUiActions::none();
            actions
                .remove_watchpoints
                .push((0x380, 0x381, WatchType::Read));
            actions.add_watchpoint = Some((0xff0041, 0xff0041, WatchType::Write));
            let result = deliver(&mut backend, 1, actions, false, true, &mut fault);
            compare(&backend, &mut expected, &result, 0);
            assert_eq!(machine(&backend).peek(0xff0040), Some(0xa5));
            assert_eq!(machine(&backend).peek(0xff0041), Some(0x5a));
            let hit = result
                .ui_data
                .cpu_debug
                .as_ref()
                .unwrap()
                .hit_watchpoint
                .as_ref()
                .unwrap();
            assert_eq!(
                (hit.address, hit.old_value, hit.new_value),
                (0xff0041, Some(0), Some(0x5a))
            );

            let before = machine(&backend).clone();
            let old = current.clone();
            for invalid in ["000400:1234+FF0050:FF", "000004:FFFF"] {
                assert!(matches!(
                    update(&mut backend, &mut current, codes(invalid)),
                    Some(EmuResponse::CheatsUpdateFailed(_))
                ));
                assert_eq!(current, old);
                assert_eq!(machine(&backend), &before);
            }
            assert!(update(&mut backend, &mut current, codes("000380:0008")).is_none());
            assert_eq!(machine(&backend).peek(0x380), Some(0));
            assert_eq!(machine(&backend).peek(0x381), Some(8));
            assert!(update(&mut backend, &mut current, Vec::new()).is_none());
            assert_eq!(machine(&backend).peek(0x380), Some(0));
            assert_eq!(machine(&backend).peek(0x381), Some(0));
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_rom_cheats_reject_ineligible_mappers_without_publishing_mixed_ram_updates() {
    let mut rom = vec![0; 0x1000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x1f0] = b'U';
    rom[0x200..0x204].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
    let rom_hash = zeff_firmware::sha256_bytes(&rom);
    // Caller-granted SVP reads use the original image directly.
    let core = Emulator::new_synthetic_svp(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: true,
        },
        vec![0; 1024],
    )
    .unwrap();
    let mut backend = EmuBackend::Md(Box::new(MdBackend::new(
        core,
        "rom-cheat-ineligible.md".into(),
        "rom-cheat-ineligible.md".into(),
        rom_hash,
        false,
    )));
    let caps = backend.capabilities();
    assert!(!caps.cheat_features.supports_rom_patches);
    assert!(caps.cheat_features.supports_libretro_database);
    assert_eq!(caps.cheat_features.formats, &["Raw"]);
    let mut current = Vec::new();
    let mut state = crate::debug::CheatState::new();
    state.active_system = ActiveSystem::Md;
    state.libretro_platform = crate::libretro_common::LibretroPlatform::Md;
    let (tx, rx) = std::sync::mpsc::channel();
    state.libretro_rx = Some(rx);
    state.libretro_busy = true;
    tx.send(crate::debug::LibretroAsyncResult::Downloaded {
        filename: "Synthetic mixed.cht".into(),
        result: Ok(concat!(
            "cheat0_code = \"FF0040:08\"\ncheat0_enable = false\n",
            "cheat1_code = \"FF0040:FF+MJBT-AMNA\"\ncheat1_enable = false\n",
        )
        .into()),
    })
    .unwrap();
    drop(tx);
    crate::debug::poll_libretro_results_for_test(&mut state);
    assert_eq!(state.libretro_codes.len(), 2);
    assert!(state.libretro_codes.iter().all(|code| !code.enabled));
    state.libretro_codes[0].enabled = true;
    assert!(
        update(
            &mut backend,
            &mut current,
            collect_enabled_patches(&state.user_codes, &state.libretro_codes),
        )
        .is_none()
    );
    let before = machine(&backend).clone();
    let old = current.clone();
    state.libretro_codes[1].enabled = true;
    let candidate = collect_enabled_patches(&state.user_codes, &state.libretro_codes);
    let response = update(&mut backend, &mut current, candidate);
    let Some(EmuResponse::CheatsUpdateFailed(message)) = response else {
        panic!("unsupported ROM cheat was not rejected");
    };
    assert!(message.contains("qualified cartridge"));
    assert_eq!(current, old);
    assert_eq!(machine(&backend), &before);
    let result = EmuThread::handle_step_frames(
        &mut backend,
        md_audio_tests::input(1, false),
        &current,
        false,
        &mut zeff_emu_common::rewind::RewindBuffer::new(0, 1),
        &mut 0,
        &mut WorkerRuntimeFault::default(),
    );
    assert!(result.runtime_fault.is_none());
    assert_eq!(machine(&backend).peek(0xff0040), Some(8));
}
