use super::*;
use crate::app::{PendingSymbolLoad, SymbolLoadResult};
use crate::symbols::SymbolSession;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn settle(app: &mut App) {
    let started = Instant::now();
    while app.pending_symbol_load.is_some() || app.pending_sound_symbol_load.is_some() {
        app.poll_symbol_load();
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "symbol loader timeout"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn md_sound_native_async_loader_keeps_target_slots_and_refuses_stale_owner_reply() {
    let path = std::env::temp_dir().join(format!(
        "zeff-sound-symbols-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir(&path).unwrap();
    let main = path.join("main.sym");
    let sound = path.join("sound.sym");
    std::fs::write(&main, b"000200 T MainEntry").unwrap();
    std::fs::write(&sound, b"[labels]\n00:0200 SoundEntry").unwrap();
    let mut settings = crate::settings::Settings::default();
    settings.ui.check_for_updates = false;
    let mut app = crate::app::construct::create(None, settings, None, false);
    app.active_system = ActiveSystem::Md;
    app.rom_info.rom_hash = Some([0x5a; 32]);
    let rom = path.join("game.md");
    app.start_symbol_load_request(
        DebugTarget::Primary,
        ActiveSystem::Md,
        (rom.clone(), rom.clone()),
        [0x5a; 32],
        Some(main.clone()),
    );
    app.start_symbol_load_request(
        DebugTarget::Sound,
        ActiveSystem::Md,
        (rom.clone(), rom.clone()),
        [0x5a; 32],
        Some(sound.clone()),
    );
    app.debug_windows.select_debug_target(DebugTarget::Sound);
    settle(&mut app);
    assert_eq!(
        app.symbols.symbol_name_at_cpu_address(0x200),
        Some("MainEntry")
    );
    assert_eq!(
        app.sound_symbols.symbol_name_at_cpu_address(0x200),
        Some("SoundEntry")
    );
    assert_eq!(app.symbols.resolve_cpu_name("SoundEntry"), None);
    assert_eq!(app.sound_symbols.resolve_cpu_name("MainEntry"), None);
    assert_eq!(app.sound_symbols.target(), DebugTarget::Sound);
    let old_binding = SymbolLoadBinding {
        owner: app.symbol_load_owner,
        target: DebugTarget::Primary,
        rom_hash: [0x5a; 32],
        request_id: app.next_symbol_load_id,
    };
    let (sender, receiver) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let old_main = main.clone();
    let thread = std::thread::spawn(move || {
        let session = SymbolSession::load_for_paths_with_sidecar(
            ActiveSystem::Md,
            &old_main,
            &old_main,
            [0x5a; 32],
            &old_main,
        );
        wait.recv().unwrap();
        sender
            .send(SymbolLoadResult {
                binding: old_binding,
                request_id: old_binding.request_id,
                elapsed: Duration::ZERO,
                session,
            })
            .unwrap();
    });
    app.pending_symbol_load = Some(PendingSymbolLoad {
        binding: old_binding,
        request_id: old_binding.request_id,
        receiver,
    });
    app.debug_windows.select_debug_target(DebugTarget::Primary);
    app.debug_windows.select_debug_target(DebugTarget::Sound);
    release.send(()).unwrap();
    thread.join().unwrap();
    app.poll_symbol_load();
    assert_eq!(
        app.symbols.symbol_name_at_cpu_address(0x200),
        Some("MainEntry")
    );
    assert_eq!(
        app.sound_symbols.symbol_name_at_cpu_address(0x200),
        Some("SoundEntry")
    );

    let (sender, receiver) = mpsc::channel();
    let old = SymbolSession::load_md_sound([0x5a; 32], &sound);
    let stale_binding = SymbolLoadBinding {
        target: DebugTarget::Sound,
        ..old_binding
    };
    app.start_symbol_load_for_paths(ActiveSystem::Md, rom.clone(), rom, [0x5a; 32], false);
    assert_ne!(app.symbol_load_owner, stale_binding.owner);
    app.pending_sound_symbol_load = Some(PendingSymbolLoad {
        binding: stale_binding,
        request_id: stale_binding.request_id,
        receiver,
    });
    let thread = std::thread::spawn(move || {
        sender
            .send(SymbolLoadResult {
                binding: stale_binding,
                request_id: stale_binding.request_id,
                elapsed: Duration::ZERO,
                session: old,
            })
            .unwrap()
    });
    thread.join().unwrap();
    app.poll_symbol_load();
    assert!(app.sound_symbols.store.is_empty());
    assert!(app.pending_sound_symbol_load.is_none());
    let mut action = crate::debug::DebugUiActions::none();
    action.select_target = Some(DebugTarget::Sound);
    app.merge_debug_actions(action);
    let mut stale = crate::debug::DebugUiActions::none();
    stale.disasm_target = Some(crate::debug::DisassemblyTarget::RomStorage(0x200));
    app.merge_debug_actions(stale);
    assert!(app.debug_windows.disasm_target.is_none());
    let old_revision = app.debug_windows.debug_revision;
    app.debug_windows.select_debug_target(DebugTarget::Primary);
    app.debug_windows.select_debug_target(DebugTarget::Sound);
    let mut delayed = crate::debug::DebugUiActions::none();
    delayed.target = DebugTarget::Sound;
    delayed.target_revision = Some(old_revision);
    delayed.disasm_target = Some(crate::debug::DisassemblyTarget::Cpu(
        crate::debug::CpuDisassemblyTarget {
            cpu_address: 0x200,
            storage_offset: None,
            bank: None,
            thumb: None,
        },
    ));
    app.merge_debug_actions(delayed);
    assert!(app.debug_windows.disasm_target.is_none());
    let mut bytes = vec![0; 0x204];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    bytes[0x200..].copy_from_slice(&[0x4e, 0x71, 0x60, 0xfe]);
    let core = zeff_md_core::Emulator::new(bytes, zeff_md_core::LoadConfig::default()).unwrap();
    let mut request =
        app.build_snapshot_request(&crate::debug::dock::TabDataRequirements::default(), true);
    request.want_debug_info = true;
    request.show_disassembler = true;
    request.debug_target = DebugTarget::Primary;
    request.debug_revision = old_revision;
    let stale_data = crate::ui::collect_md_snapshot(&core, &request, None);
    assert!(stale_data.cpu_debug.is_some() && stale_data.disassembly_view.is_some());
    app.process_ui_frame_data(stale_data);
    assert!(app.cached_ui_data.as_ref().unwrap().cpu_debug.is_none());
    assert!(
        app.cached_ui_data
            .as_ref()
            .unwrap()
            .disassembly_view
            .is_none()
    );
    let mut rejected = crate::debug::DebugUiActions::none();
    rejected.target = DebugTarget::Sound;
    rejected.add_breakpoint = Some(0x200);
    rejected.step_requested = true;
    app.merge_debug_actions(rejected);
    assert!(app.pending_debug_actions.add_breakpoint.is_none());
    assert!(!app.debug_requests.step);
    assert!(app.debug_windows.select_debug_target(DebugTarget::Primary));
    app.debug_windows.debug_revision = u64::MAX - 1;
    assert!(app.debug_windows.select_debug_target(DebugTarget::Sound));
    assert_eq!(app.debug_windows.debug_revision, u64::MAX);
    app.debug_windows.reset_target_sessions();
    assert_eq!(app.debug_windows.debug_target, DebugTarget::Primary);
    assert_eq!(app.debug_windows.debug_revision, u64::MAX);
    let mut exhausted_action = crate::debug::DebugUiActions::none();
    exhausted_action.target_revision = Some(u64::MAX);
    exhausted_action.disasm_target = Some(crate::debug::DisassemblyTarget::Cpu(
        crate::debug::CpuDisassemblyTarget::m68000(0x200),
    ));
    request.debug_target = DebugTarget::Primary;
    request.debug_revision = u64::MAX;
    let exhausted_response = crate::ui::collect_md_snapshot(&core, &request, None);
    assert!(exhausted_response.cpu_debug.is_some());
    assert!(exhausted_response.disassembly_view.is_some());
    app.cached_ui_data = Some(crate::ui::collect_md_snapshot(&core, &request, None));
    app.debug_windows.reset_target_sessions();
    app.merge_debug_actions(exhausted_action);
    assert!(app.debug_windows.disasm_target.is_none());
    app.process_ui_frame_data(exhausted_response);
    let cached = app.cached_ui_data.as_ref().unwrap();
    assert!(cached.cpu_debug.is_none() && cached.disassembly_view.is_none());
    let mut legacy = crate::debug::DebugUiActions::none();
    legacy.disasm_target = Some(crate::debug::DisassemblyTarget::Cpu(
        crate::debug::CpuDisassemblyTarget::m68000(0x204),
    ));
    app.merge_debug_actions(legacy);
    assert_eq!(
        app.debug_windows.disasm_target,
        Some(crate::debug::DisassemblyTarget::Cpu(
            crate::debug::CpuDisassemblyTarget::m68000(0x204),
        ))
    );
    std::fs::remove_file(main).unwrap();
    std::fs::remove_file(sound).unwrap();
    std::fs::remove_dir(path).unwrap();
}
