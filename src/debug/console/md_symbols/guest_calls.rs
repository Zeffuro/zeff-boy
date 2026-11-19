use super::*;

mod static_disassembly;
use crate::debug::CpuDebugSnapshot;
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, BackendRuntimeConfig, EmuBackend, load_backend_from_rom_source,
};
use crate::emu_thread::{RenderSettings, ReusableBuffers, SnapshotRequest};
use crate::symbols::{ExecMode, StorageLocation};

fn backend() -> EmuBackend {
    let mut bytes = vec![0; 0x800];
    bytes[..4].copy_from_slice(&0x00ff_fffc_u32.to_be_bytes());
    bytes[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    bytes[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    bytes[0x1f0] = b'U';
    for (address, words) in [
        (0x200, &[0x60fe_u16, 0x4e71][..]),
        (0x400, &[0x7007, 0x5280, 0x13c0, 0x00ff, 0x0040, 0x4e75][..]),
    ] {
        for (index, word) in words.iter().enumerate() {
            let offset = address + index * 2;
            bytes[offset..offset + 2].copy_from_slice(&word.to_be_bytes());
        }
    }
    let path = std::path::Path::new("console-guest-call.md");
    load_backend_from_rom_source(
        ActiveSystem::Md,
        path,
        path,
        Some(bytes),
        BackendLoadConfig {
            sample_rate: Some(48_000),
            md_region: Some(zeff_md_core::Region::NtscUs),
            md_controller: zeff_md_core::ControllerKind::ThreeButton,
            md_load_save_ram: false,
            md_host_persistence: false,
            ..Default::default()
        },
    )
    .unwrap()
    .backend
}

fn machine(backend: &EmuBackend) -> &zeff_md_core::Emulator {
    let EmuBackend::Md(md) = backend else {
        panic!("expected MD")
    };
    md.core()
}

fn cpu_snapshot(backend: &mut EmuBackend) -> CpuDebugSnapshot {
    crate::ui::collect_backend_snapshot(
        backend,
        &SnapshotRequest {
            debug_target: crate::debug::DebugTarget::Primary,
            debug_revision: 0,
            want_debug_info: true,
            want_perf_info: false,
            any_viewer_open: false,
            any_vram_viewer_open: false,
            show_oam_viewer: false,
            show_apu_viewer: false,
            show_disassembler: false,
            show_rom_info: false,
            show_memory_viewer: false,
            memory_view_start: 0,
            show_rom_viewer: false,
            show_instruction_trace: false,
            trace_cursor: Default::default(),
            rom_view_start: 0,
            last_disasm_pc: None,
            last_disasm_mapping: None,
            disasm_target: None,
            memory_search: None,
            rom_search: None,
            render: RenderSettings {
                color_correction: crate::settings::ColorCorrection::None,
                color_correction_matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                dmg_palette_preset: Default::default(),
                nes_palette_mode: Default::default(),
                nes_custom_palette: None,
                pce_overscan_mode: Default::default(),
                pce_palette_mode: Default::default(),
                sgb_border_enabled: false,
            },
        },
        ReusableBuffers {
            audio: None,
            vram: None,
            oam: None,
            memory_page: None,
            nes_chr: None,
            nes_nametable: None,
        },
    )
    .cpu_debug
    .unwrap()
}

fn command(
    state: &mut DebugConsoleState,
    symbols: &SymbolSession,
    backend: &mut EmuBackend,
    text: &str,
) -> DebugUiActions {
    let cpu = cpu_snapshot(backend);
    let mut memory = MemoryViewerState::new();
    memory.configure_for_system(System::Md);
    let mut rom = RomViewerState::new();
    let mut actions = DebugUiActions::none();
    run_command(
        state,
        text,
        CommandContext {
            supports_rom_breakpoints: true,
            symbols,
            cpu_debug: Some(&cpu),
            rom_debug: None,
            disassembly: None,
            views: DebugConsoleViews {
                memory: &mut memory,
                rom: &mut rom,
            },
            actions: &mut actions,
        },
    );
    actions
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_imported_symbol_console_call_and_undo_reach_real_backend_and_preserve_failed_tokens() {
    let module = import_symbols(
        "console.sym",
        b"00000400 0000000C T Entry",
        &ImportContext {
            target: TargetInfo { system: System::Md },
            image: ImageId(0),
            rom_region: RegionId(0),
            cpu_space: AddressSpaceId(0),
            source_name: None,
        },
    )
    .unwrap();
    assert_eq!(module.format, "GNU nm M68000 .sym");
    let mut symbols = SymbolSession::default();
    symbols.store.extend(module.symbols);
    let entry = symbols.store.lookup_name("Entry").next().unwrap().clone();
    assert_eq!(entry.location.exec_mode, ExecMode::M68000);
    assert_eq!(entry.location.cpu.unwrap().address, 0x400);
    assert!(entry.location.storage.is_none());
    let mut stale = entry;
    stale.name = "StaleStorage".into();
    stale.location.storage = Some(StorageLocation {
        image: ImageId(0),
        region: RegionId(0),
        offset: 0x402,
    });
    symbols.store.insert(stale);
    assert_eq!(completions("call ent", &symbols), ["call Entry"]);

    let mut backend = backend();
    assert!(backend.supports_guest_calls() && backend.supports_debugger());
    let mut state = DebugConsoleState::default();
    assert!(
        command(&mut state, &symbols, &mut backend, "call Entry 4")
            .guest_call
            .is_none()
    );
    assert!(!state.guest_call_pending && state.guest_call_undo.is_none());
    assert!(state.output.back().unwrap().contains("Suspend the CPU"));
    backend.debug_suspend();
    let original = machine(&backend).encode_frontend_state().unwrap();
    let caller = cpu_snapshot(&mut backend);
    let request = command(&mut state, &symbols, &mut backend, "call Entry 4")
        .guest_call
        .unwrap();
    assert_eq!(
        (
            request.name.as_str(),
            request.target,
            request.exec_mode,
            request.instruction_budget
        ),
        ("Entry", 0x400, ExecMode::M68000, 4)
    );
    assert!(request.storage_offset.is_none() && !request.explicit_overlay);
    assert!(state.guest_call_pending);
    assert!(
        command(&mut state, &symbols, &mut backend, "call Entry 4")
            .guest_call
            .is_none()
    );
    assert!(
        command(&mut state, &symbols, &mut backend, "callundo")
            .undo_guest_call
            .is_none()
    );
    let (instructions, token) = backend.execute_guest_call(&request).unwrap();
    assert_eq!((instructions, token.len()), (4, 16));
    state.guest_call_completed(&request.name, instructions, token.clone());
    assert!(!state.guest_call_pending);
    assert_eq!(state.guest_call_undo, Some(token.clone()));
    assert_eq!(
        state.output.back().unwrap(),
        "Entry returned after 4 instructions"
    );
    assert!(backend.is_suspended());
    assert_eq!(machine(&backend).registers().d[0], 8);
    assert_eq!(machine(&backend).peek(0xff0040), Some(8));
    assert_eq!(machine(&backend).registers().pc, caller.pc);
    assert!(machine(&backend).cpu_clock().0 > caller.cycles);
    let called = machine(&backend).encode_frontend_state().unwrap();

    let stale = command(&mut state, &symbols, &mut backend, "call StaleStorage 4")
        .guest_call
        .unwrap();
    assert_eq!((stale.target, stale.storage_offset), (0x400, Some(0x402)));
    let error = backend.execute_guest_call(&stale).unwrap_err().to_string();
    assert!(error.contains("no longer maps"));
    state.guest_call_failed(&stale.name, &error);
    assert!(!state.guest_call_pending);
    assert_eq!(state.guest_call_undo, Some(token.clone()));
    assert_eq!(machine(&backend).encode_frontend_state().unwrap(), called);

    backend.set_sample_rate(44_100);
    let changed = machine(&backend).encode_frontend_state().unwrap();
    let undo = command(&mut state, &symbols, &mut backend, "callundo")
        .undo_guest_call
        .unwrap();
    assert_eq!(undo, token);
    let error = backend.undo_guest_call(undo).unwrap_err().to_string();
    state.guest_call_undo_failed(&error);
    assert!(!state.guest_call_pending);
    assert_eq!(state.guest_call_undo, Some(token.clone()));
    assert_eq!(machine(&backend).encode_frontend_state().unwrap(), changed);
    backend.set_sample_rate(48_000);
    let undo = command(&mut state, &symbols, &mut backend, "callundo")
        .undo_guest_call
        .unwrap();
    assert_eq!(undo, token);
    backend.undo_guest_call(undo).unwrap();
    state.guest_call_undone();
    assert!(!state.guest_call_pending && state.guest_call_undo.is_none());
    assert_eq!(state.output.back().unwrap(), "Guest call undone");
    assert!(backend.is_suspended());
    assert_eq!(machine(&backend).encode_frontend_state().unwrap(), original);
    let restored = cpu_snapshot(&mut backend);
    assert_eq!(
        (restored.register_lines, restored.pc, restored.cycles),
        (caller.register_lines, caller.pc, caller.cycles)
    );
    assert!(
        command(&mut state, &symbols, &mut backend, "callundo")
            .undo_guest_call
            .is_none()
    );

    let patches = crate::cheats::parse_cheat_for_system(
        "000200:13FC+000202:005A+000204:00B0+000206:0000",
        ActiveSystem::Md,
    )
    .unwrap()
    .0;
    let EmuBackend::Md(md) = &mut backend else {
        unreachable!()
    };
    md.test_normal_core_mut()
        .unwrap()
        .install_rom_patches(&patches)
        .unwrap();
    let actions = DebugUiActions::none();
    let mut config = BackendRuntimeConfig::new(&actions);
    config.debug_continue = true;
    backend.apply_runtime_config(config);
    backend.step_frame();
    assert!(backend.is_suspended());
    assert!(
        backend
            .take_runtime_fault()
            .unwrap()
            .contains("machine fault")
    );
    let faulted = machine(&backend).clone();
    let request = command(&mut state, &symbols, &mut backend, "call Entry 4")
        .guest_call
        .unwrap();
    let error = backend
        .execute_guest_call(&request)
        .unwrap_err()
        .to_string();
    assert!(error.contains("execution is faulted"));
    state.guest_call_failed(&request.name, &error);
    assert!(!state.guest_call_pending && state.guest_call_undo.is_none());
    assert_eq!(machine(&backend), &faulted);
    assert!(state.output.back().unwrap().contains("Entry failed:"));
}

#[cfg(not(target_arch = "wasm32"))]
mod elf;
