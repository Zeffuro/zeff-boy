mod sound_target;

use super::md_pc_breakpoint_tests::{compare, deliver, machine};
use super::*;
use crate::debug::{DebugUiActions, DisassemblyTarget};
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::symbols::import::{ImportContext, TargetInfo, import_symbols};
use crate::symbols::{AddressSpaceId, ImageId, RegionId, SymbolSession};
use zeff_emu_common::debug::WatchType;
use zeff_md_core::TimingModel;

fn fixture(header: u8, timing: TimingModel) -> EmuBackend {
    let mut rom = vec![0; 0x10400];
    rom[..4].copy_from_slice(&0xFFFFFC_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x10200_u32.to_be_bytes());
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x1f0] = header;
    for (index, word) in [0x33fc_u16, 0xa55a, 0x00ff, 0x8040, 0x60f6]
        .into_iter()
        .enumerate()
    {
        rom[0x10200 + index * 2..0x10202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let path = std::path::Path::new("symbols.md");
    let mut backend = load_backend_from_rom_source(
        ActiveSystem::Md,
        path,
        path,
        Some(rom),
        BackendLoadConfig {
            md_timing: timing,
            md_load_save_ram: false,
            md_host_persistence: false,
            sample_rate: Some(44_100),
            ..Default::default()
        },
    )
    .unwrap()
    .backend;
    deliver(
        &mut backend,
        0,
        DebugUiActions::none(),
        false,
        false,
        &mut WorkerRuntimeFault::default(),
    );
    backend
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_imported_symbols_reach_worker_high_pc_watchpoint_and_read_only_disassembly() {
    let module = import_symbols(
        "game.sym",
        b"00010200 8 T Entry\n00FF8041 1 B Counter",
        &ImportContext {
            target: TargetInfo {
                system: ActiveSystem::Md,
            },
            image: ImageId(0),
            rom_region: RegionId(0),
            cpu_space: AddressSpaceId(0),
            source_name: None,
        },
    )
    .unwrap();
    let mut symbols = SymbolSession::default();
    symbols.store.extend(module.symbols);
    let entry = u32::try_from(symbols.resolve_cpu_name("Entry").unwrap()).unwrap();
    let counter = u32::try_from(symbols.resolve_cpu_name("Counter").unwrap()).unwrap();
    assert_eq!((entry, counter), (0x10200, 0xFF8041));
    assert!(symbols.resolve_rom_name("Entry").is_none());
    for header in *b"UJE" {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut backend = fixture(header, timing);
            let mut expected = machine(&backend).clone();
            let mut fault = WorkerRuntimeFault::default();
            let mut actions = DebugUiActions::none();
            actions.add_breakpoint = Some(entry);
            let mut result = deliver(&mut backend, 1, actions, false, false, &mut fault);
            compare(&backend, &mut expected, &result, 0);
            let cpu = result.ui_data.cpu_debug.as_ref().unwrap();
            assert_eq!(cpu.hit_breakpoint, Some(entry));
            assert_eq!(cpu.pc, entry);
            let disassembly = result.ui_data.disassembly_view.as_mut().unwrap();
            symbols.annotate_disassembly(disassembly);
            assert!(disassembly.lines.iter().any(|line| line.address == entry
                && line.bytes.as_slice() == [0x33, 0xfc, 0xa5, 0x5a, 0x00, 0xff, 0x80, 0x40]
                && line.symbol.as_deref() == Some("Entry")
                && line.storage_offset.is_none()));
            let caps = result.ui_data.core_features.as_ref().unwrap();
            assert!(caps.supports_symbol_loading && backend.supports_symbol_loading());
            assert!(caps.supports_debugger && caps.supports_guest_calls);
            assert!(result.ui_data.instruction_trace.is_none());
            let mut input = super::md_audio_tests::input(0, false);
            input.snapshot.want_debug_info = true;
            input.snapshot.show_disassembler = true;
            input.snapshot.disasm_target =
                Some(DisassemblyTarget::Cpu(crate::debug::CpuDisassemblyTarget {
                    cpu_address: entry + 8,
                    storage_offset: None,
                    bank: None,
                    thumb: None,
                }));
            let mut navigated = EmuThread::handle_step_frames(
                &mut backend,
                input,
                &[],
                false,
                &mut zeff_emu_common::rewind::RewindBuffer::new(0, 1),
                &mut 0,
                &mut fault,
            );
            compare(&backend, &mut expected, &navigated, 0);
            let view = navigated.ui_data.disassembly_view.as_mut().unwrap();
            symbols.annotate_disassembly(view);
            assert_eq!(view.cursor.cpu_address().unwrap(), entry + 8);
            assert!(view.is_navigation_target && !view.is_static_target);
            assert_eq!(view.lines[0].control_target, Some(entry));
            assert_eq!(
                view.lines[0].control_target_symbol.as_deref(),
                Some("Entry")
            );
            assert_eq!(machine(&backend).registers().pc, entry);
            let mut actions = DebugUiActions::none();
            actions.add_watchpoint = Some((counter, counter, WatchType::Write));
            actions.memory_writes.push((counter, 0));
            expected.step_instruction().unwrap();
            let result = deliver(&mut backend, 1, actions, false, true, &mut fault);
            compare(&backend, &mut expected, &result, 0);
            let hit = result
                .ui_data
                .cpu_debug
                .as_ref()
                .unwrap()
                .hit_watchpoint
                .as_ref()
                .unwrap();
            assert_eq!((hit.address, hit.new_value), (counter, Some(0x5a)));
            assert_eq!(machine(&backend).peek(counter), Some(0x5a));
            assert_eq!(machine(&backend).registers().pc, entry + 8);
        }
    }
}
