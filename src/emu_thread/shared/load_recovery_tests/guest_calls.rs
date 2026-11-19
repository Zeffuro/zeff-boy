use super::*;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_thread::GuestCallRequest;
use zeff_emu_common::debug::{TraceBoundaryKind, TraceExecMode, WatchType};

mod call_stack;
mod commands;
mod io;
mod memory_search;
#[cfg(not(target_arch = "wasm32"))]
mod native_persistence;
mod rom_breakpoints;
mod sound_trace_worker;
mod static_disassembly;

const RETURN_PC: u32 = 0x21c;
const CALLEE: &[(u32, &[u16])] = &[
    (0x1000, &[0x7007]),
    (0x1002, &[0x5280]),
    (0x1004, &[0x227c, 0x00ff, 0x0400]),
    (0x100a, &[0x13c0, 0x00ff, 0x0040]),
    (0x1010, &[0x4eb9, 0x0000, 0x1100]),
    (0x1100, &[0x13fc, 0x0042, 0x00ff, 0x0041]),
    (0x1108, &[0x4e75]),
    (0x1016, &[0x4e75]),
];

fn guest_call_backend() -> EmuBackend {
    guest_call_backend_with(
        b'U',
        zeff_md_core::TimingModel::Standard,
        zeff_md_core::ControllerKind::ThreeButton,
        &[],
        false,
    )
}

fn guest_call_backend_with(
    header: u8,
    timing: zeff_md_core::TimingModel,
    controller: zeff_md_core::ControllerKind,
    blocks: &[(u32, &[u16])],
    sram: bool,
) -> EmuBackend {
    let mut rom = vec![0; 0x2000];
    rom[..4].copy_from_slice(&0x00ff_fffc_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x1f0] = header;
    if sram {
        rom[0x1b0..0x1bc]
            .copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20, 0x3f, 0xff]);
    }
    let main = [
        0x13fc_u16, 0x0080, 0x00c0, 0x0011, 0x13fc, 0x0010, 0x00c0, 0x0011, 0x13fc, 0x0090, 0x00c0,
        0x0011, 0x46fc, 0x2315, 0x60fe, 0x4e71,
    ];
    for (address, words) in std::iter::once((0x200, main.as_slice()))
        .chain(CALLEE.iter().copied())
        .chain(blocks.iter().copied())
    {
        for (index, word) in words.iter().enumerate() {
            let offset = address as usize + index * 2;
            rom[offset..offset + 2].copy_from_slice(&word.to_be_bytes());
        }
    }
    let path = std::path::Path::new("guest-call.md");
    load_backend_from_rom_source(
        ActiveSystem::Md,
        path,
        path,
        Some(rom),
        BackendLoadConfig {
            sample_rate: Some(48_000),
            md_region: Some(match header {
                b'J' => zeff_md_core::Region::NtscJapan,
                b'E' => zeff_md_core::Region::Pal,
                _ => zeff_md_core::Region::NtscUs,
            }),
            md_timing: timing,
            md_controller: controller,
            md_load_save_ram: false,
            md_host_persistence: false,
            ..Default::default()
        },
    )
    .unwrap()
    .backend
}

fn guest_call_input(frames: usize) -> FrameInput {
    let mut request = input(frames, 0, 0);
    request.snapshot.want_debug_info = true;
    request.snapshot.show_instruction_trace = true;
    request.snapshot.show_memory_viewer = true;
    request.snapshot.memory_view_start = 0xff0000;
    request
}

fn guest_call_deliver(thread: &EmuThread, request: FrameInput) -> FrameResult {
    thread.send(EmuCommand::StepFrames(Box::new(request)));
    receive_frame(thread)
}

fn history(frame: &FrameResult) -> Vec<(u32, Vec<u8>)> {
    frame
        .ui_data
        .cpu_debug
        .as_ref()
        .unwrap()
        .recent_opcodes
        .iter()
        .map(|row| (row.address, row.bytes.clone()))
        .collect()
}

fn watchpoints(frame: &FrameResult) -> Vec<(u32, u32, WatchType)> {
    frame
        .ui_data
        .cpu_debug
        .as_ref()
        .unwrap()
        .watchpoints
        .iter()
        .map(|row| (row.address, row.end_address, row.watch_type))
        .collect()
}

fn paused(frame: &FrameResult) {
    assert_eq!(frame.advanced_frames, 0);
    assert_eq!(frame.completed_step_requests, 1);
    assert!(frame.runtime_fault.is_none());
    assert!(
        frame
            .ui_data
            .core_features
            .as_ref()
            .unwrap()
            .memory_editing
            .as_ref()
            .unwrap()
            .enabled
    );
}

fn guest_call_normal_worker_round_trip() {
    let mut subject = EmuThread::spawn(guest_call_backend(), false);
    let mut control = EmuThread::spawn(guest_call_backend(), false);
    let warm = step(&subject, 4, 0, 0);
    let expected = step(&control, 4, 0, 0);
    assert_eq!((warm.advanced_frames, expected.advanced_frames), (4, 4));
    assert!(warm.runtime_fault.is_none() && expected.runtime_fault.is_none());
    assert!(warm.audio_samples.iter().any(|&sample| sample != 0.0));
    assert_eq!(pcm(&warm), pcm(&expected));
    assert_eq!(capture(&subject), capture(&control));
    let stop = || {
        let mut request = guest_call_input(1);
        request.debug_actions.add_breakpoint = Some(RETURN_PC);
        request.debug_actions.add_watchpoint = Some((0xff0050, 0xff0050, WatchType::Read));
        request.debug_actions.trace_enabled = Some(true);
        request.debug_actions.trace_capacity = Some(64);
        request
    };
    let stopped = guest_call_deliver(&subject, stop());
    let expected = guest_call_deliver(&control, stop());
    paused(&stopped);
    paused(&expected);
    let features = stopped.ui_data.core_features.as_ref().unwrap();
    assert!(features.supports_guest_calls && features.supports_debugger);
    let before_cpu = stopped.ui_data.cpu_debug.as_ref().unwrap();
    assert_eq!(
        (before_cpu.pc, before_cpu.hit_breakpoint),
        (RETURN_PC, Some(RETURN_PC))
    );
    assert!(before_cpu.register_lines[9].ends_with("SR:2315"));
    assert!(history(&stopped).is_empty());
    assert!(
        stopped
            .ui_data
            .instruction_trace
            .as_ref()
            .unwrap()
            .entries
            .is_empty()
    );
    let before = capture(&subject);
    assert_eq!(before, capture(&control));
    subject.send(EmuCommand::ExecuteGuestCall(GuestCallRequest {
        name: "nested".into(),
        target: 0x1000,
        storage_offset: Some(0x1000),
        explicit_overlay: false,
        exec_mode: crate::symbols::ExecMode::M68000,
        instruction_budget: 8,
    }));
    let EmuResponse::GuestCallCompleted {
        name,
        instructions,
        undo_state,
    } = receive_response(&subject)
    else {
        panic!("normal MD worker guest call failed");
    };
    assert_eq!(
        (name.as_str(), instructions, undo_state.len()),
        ("nested", 8, 16)
    );
    assert_ne!(capture(&subject), before);
    let completed = guest_call_deliver(&subject, guest_call_input(1));
    paused(&completed);
    let cpu = completed.ui_data.cpu_debug.as_ref().unwrap();
    assert_eq!(cpu.pc, RETURN_PC);
    assert!(cpu.cycles > before_cpu.cycles);
    assert!(cpu.register_lines[0].starts_with("D0:00000008"));
    assert!(cpu.register_lines[4].ends_with("A1:00FF0400"));
    assert!(cpu.register_lines[9].ends_with("SR:2300"));
    assert_eq!(cpu.breakpoints, before_cpu.breakpoints);
    assert_eq!(watchpoints(&completed), watchpoints(&stopped));
    assert!(
        cpu.hit_breakpoint.is_none() && cpu.hit_watchpoint.is_none() && cpu.hit_event.is_none()
    );
    let memory = completed.ui_data.memory_page.as_ref().unwrap();
    assert!(memory.contains(&(0xff0040, 8)) && memory.contains(&(0xff0041, 0x42)));
    assert_eq!(
        history(&completed),
        CALLEE
            .iter()
            .rev()
            .map(|(pc, words)| (*pc, words[0].to_be_bytes().to_vec()))
            .collect::<Vec<_>>()
    );
    let trace = completed.ui_data.instruction_trace.as_ref().unwrap();
    assert!(trace.enabled);
    assert_eq!(
        (trace.retained, trace.capacity, trace.entries.len()),
        (8, zeff_emu_common::debug::MIN_TRACE_CAPACITY, 8)
    );
    assert!(trace.entries[0].cycle > before_cpu.cycles);
    for (row, &(pc, words)) in trace.entries.iter().zip(CALLEE) {
        assert_eq!((row.pc, row.mode), (pc, TraceExecMode::M68000));
        assert_eq!(
            row.instruction_bytes(),
            words
                .iter()
                .flat_map(|word| word.to_be_bytes())
                .collect::<Vec<_>>()
        );
        let boundary = row.boundary.unwrap();
        assert_eq!(boundary.kind, TraceBoundaryKind::Instruction);
        assert!(boundary.cpu_clocks > 0 && !boundary.machine_fault);
        assert!(boundary.final_exception.is_none() && row.event.is_none());
    }
    let called = capture(&subject);
    subject.send(EmuCommand::UndoGuestCall(vec![0; 16]));
    assert!(matches!(
        receive_response(&subject),
        EmuResponse::GuestCallUndoFailed(_)
    ));
    assert_eq!(capture(&subject), called);
    let refused = guest_call_deliver(&subject, guest_call_input(1));
    paused(&refused);
    assert!(refused.audio_samples.is_empty());
    assert_eq!(history(&refused), history(&completed));
    assert_eq!(
        refused.ui_data.instruction_trace.as_ref().unwrap().entries,
        trace.entries
    );
    subject.send(EmuCommand::UndoGuestCall(undo_state.clone()));
    assert!(matches!(
        receive_response(&subject),
        EmuResponse::GuestCallUndone
    ));
    assert_eq!(capture(&subject), before);
    let undone = guest_call_deliver(&subject, guest_call_input(1));
    paused(&undone);
    assert!(undone.audio_samples.is_empty());
    let cpu = undone.ui_data.cpu_debug.as_ref().unwrap();
    assert_eq!(cpu.register_lines, before_cpu.register_lines);
    assert_eq!(
        (cpu.pc, cpu.cycles, cpu.hit_breakpoint),
        (RETURN_PC, before_cpu.cycles, Some(RETURN_PC))
    );
    assert_eq!(cpu.breakpoints, before_cpu.breakpoints);
    assert_eq!(watchpoints(&undone), watchpoints(&stopped));
    assert_eq!(history(&undone), history(&stopped));
    let trace = undone.ui_data.instruction_trace.as_ref().unwrap();
    assert!(trace.enabled && trace.entries.is_empty());
    assert_eq!(trace.capacity, zeff_emu_common::debug::MIN_TRACE_CAPACITY);
    for token in [undo_state, vec![0; 16]] {
        subject.send(EmuCommand::UndoGuestCall(token));
        assert!(matches!(
            receive_response(&subject),
            EmuResponse::GuestCallUndoFailed(_)
        ));
        assert_eq!(capture(&subject), before);
        let refused = guest_call_deliver(&subject, guest_call_input(1));
        paused(&refused);
        assert!(refused.audio_samples.is_empty());
        assert!(refused.audio_timeline_discontinuities.is_empty());
    }
    for thread in [&subject, &control] {
        let mut request = guest_call_input(0);
        request.debug_actions.remove_breakpoints.push(RETURN_PC);
        request
            .debug_actions
            .remove_watchpoints
            .push((0xff0050, 0xff0050, WatchType::Read));
        request.debug_actions.trace_enabled = Some(false);
        request.debug_continue = true;
        assert_eq!(guest_call_deliver(thread, request).advanced_frames, 0);
    }
    for _ in 0..2 {
        let actual = step(&subject, 1, 0, 0);
        let expected = step(&control, 1, 0, 0);
        assert_eq!((actual.advanced_frames, expected.advanced_frames), (1, 1));
        assert!(actual.runtime_fault.is_none() && expected.runtime_fault.is_none());
        assert!(actual.audio_samples.iter().any(|&sample| sample != 0.0));
        assert_eq!(pcm(&actual), pcm(&expected));
        assert_eq!(capture(&subject), capture(&control));
        assert_eq!(
            subject.shared_framebuffer().load_full(),
            control.shared_framebuffer().load_full()
        );
    }
    subject.shutdown();
    control.shutdown();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn md_guest_call_native_normal_worker_nested_undo_preserves_paused_session_and_future_pcm() {
    guest_call_normal_worker_round_trip();
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
fn md_guest_call_wasm_normal_worker_nested_undo_preserves_paused_session_and_future_pcm() {
    guest_call_normal_worker_round_trip();
}

mod trace_stream_worker;
