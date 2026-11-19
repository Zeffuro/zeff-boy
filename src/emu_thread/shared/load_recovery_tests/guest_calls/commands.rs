use super::*;

mod rom_breakpoints;
use crate::debug::DebugUiActions;
use crate::emu_backend::BackendRuntimeConfig;
use crate::emu_thread::commands::{
    CommonCommandContext, CommonCommandDispatch, CommonCommandEffects,
};
use crate::emu_thread::{AudioRecordingCapture, SharedFramebuffer, WorkerRuntimeFault};
use zeff_emu_common::debug::{DebugEvent, InstructionTraceRecord};
use zeff_emu_common::rewind::RewindBuffer;
use zeff_emu_common::time::Reset;
use zeff_md_core::{ControllerKind, TimingModel};

mod call_stack;
mod dma;
mod extra;
mod failures;
mod ownership;
mod sound_entry;
mod timed_ports;

const EFFECTS: &[u16] = &[
    0x13fc, 0x00a5, 0x00ff, 0x0042, 0x13fc, 0x005a, 0x0020, 0x0001, 0x13fc, 0x009f, 0x00c0, 0x0011,
];

fn fixture(
    header: u8,
    timing: TimingModel,
    kind: ControllerKind,
    extra: &[(u32, &[u16])],
) -> EmuBackend {
    let routines: Vec<_> = [
        (0x1200, &[0x60fe, 0x4e71][..]),
        (0x1300, &[0x4e72, 0x2700, 0x4e75][..]),
        (0x1400, &[0x13fc, 0x005a, 0x00b0, 0x0000, 0x4e75][..]),
        (0x1500, &[0x4e40, 0x4e75][..]),
    ]
    .into_iter()
    .map(|(address, end)| {
        let mut words = EFFECTS.to_vec();
        words.extend_from_slice(end);
        (address, words)
    })
    .collect();
    let mut blocks: Vec<_> = routines
        .iter()
        .map(|(address, words)| (*address, words.as_slice()))
        .collect();
    blocks.extend([
        (0x80, &[0x0000, 0x1600][..]),
        (0x1600, &[0x4e73, 0x4e71][..]),
    ]);
    blocks.extend_from_slice(extra);
    guest_call_backend_with(
        header,
        timing,
        kind,
        &blocks,
        timing == TimingModel::Standard,
    )
}

fn standard() -> EmuBackend {
    fixture(
        b'U',
        TimingModel::Standard,
        ControllerKind::ThreeButton,
        &[],
    )
}

fn machine(backend: &EmuBackend) -> &zeff_md_core::Emulator {
    let EmuBackend::Md(md) = backend else {
        panic!("expected MD")
    };
    md.core()
}

fn machine_mut(backend: &mut EmuBackend) -> &mut zeff_md_core::Emulator {
    let EmuBackend::Md(md) = backend else {
        panic!("expected MD")
    };
    md.test_normal_core_mut().unwrap()
}

fn frontend(backend: &EmuBackend) -> Vec<u8> {
    machine(backend).encode_frontend_state().unwrap()
}

fn request(target: u32, budget: u64) -> GuestCallRequest {
    GuestCallRequest {
        name: "nested".into(),
        target,
        storage_offset: Some(u64::from(target)),
        explicit_overlay: false,
        exec_mode: crate::symbols::ExecMode::M68000,
        instruction_budget: budget,
    }
}

fn configure(backend: &mut EmuBackend, actions: &DebugUiActions) {
    let mut config = BackendRuntimeConfig::new(actions);
    config.opcode_log_enabled = true;
    backend.apply_runtime_config(config);
}

#[derive(Debug, PartialEq)]
struct Session {
    registers: Vec<String>,
    cycles: u64,
    suspended: bool,
    definitions: String,
    hits: String,
    history: String,
    trace: Vec<InstructionTraceRecord>,
    trace_config: (bool, usize, usize, Option<u64>, Option<u64>),
}

fn session(backend: &mut EmuBackend) -> Session {
    let input = guest_call_input(0);
    let view = EmuThread::collect_ui_snapshot(backend, &input.snapshot, input.buffers);
    let cpu = view.cpu_debug.unwrap();
    let trace = view.instruction_trace.unwrap();
    Session {
        registers: cpu.register_lines,
        cycles: cpu.cycles,
        suspended: backend.is_suspended(),
        definitions: format!(
            "{:?}",
            (
                cpu.breakpoints,
                cpu.rom_breakpoints,
                cpu.one_shot_breakpoints,
                cpu.breakpoint_hit_conditions,
                cpu.watchpoints,
                cpu.event_breakpoints
            )
        ),
        hits: format!(
            "{:?}",
            (
                cpu.hit_breakpoint,
                cpu.hit_rom_breakpoint,
                cpu.hit_watchpoint,
                cpu.hit_event
            )
        ),
        history: format!("{:?}", cpu.recent_opcodes),
        trace: trace.entries,
        trace_config: (
            trace.enabled,
            trace.capacity,
            trace.retained,
            trace.oldest_sequence,
            trace.newest_sequence,
        ),
    }
}

struct Owner {
    backend: EmuBackend,
    framebuffer: SharedFramebuffer,
    uncapped: bool,
    rewind: RewindBuffer,
    cheats: Vec<crate::cheats::CheatPatch>,
    capture: AudioRecordingCapture,
    markers: Vec<crate::audio_recorder::AudioTimelineDiscontinuity>,
    fault: WorkerRuntimeFault,
}

impl Owner {
    fn new(mut backend: EmuBackend) -> Self {
        backend.step_frame();
        assert!(!backend.is_suspended());
        let mut samples = Vec::new();
        machine(&backend)
            .clone()
            .drain_audio_samples_into(&mut samples);
        assert!(samples.iter().any(|&sample| sample != 0.0));
        let mut actions = DebugUiActions::none();
        actions.add_breakpoint = Some(machine(&backend).registers().pc);
        actions.trace_enabled = Some(true);
        actions.trace_capacity = Some(64);
        configure(&mut backend, &actions);
        backend.step_frame();
        assert!(backend.is_suspended());
        let mut rewind = RewindBuffer::new(1, 1);
        rewind.push(&[1, 2, 3], &[]);
        Self {
            backend,
            framebuffer: Default::default(),
            uncapped: false,
            rewind,
            cheats: Vec::new(),
            capture: AudioRecordingCapture {
                active: true,
                semantic: true,
            },
            markers: Vec::new(),
            fault: Default::default(),
        }
    }

    fn dispatch(&mut self, command: EmuCommand) -> CommonCommandEffects {
        let result = CommonCommandContext {
            backend: &mut self.backend,
            shared_framebuffer: &self.framebuffer,
            uncapped_mode: &mut self.uncapped,
            rewind_buffer: &mut self.rewind,
            last_cheats: &mut self.cheats,
            audio_recording_capture: &mut self.capture,
            pending_audio_discontinuities: &mut self.markers,
            runtime_fault: &mut self.fault,
        }
        .dispatch(command);
        let CommonCommandDispatch::Handled(effects) = result else {
            panic!("command not handled")
        };
        effects
    }

    fn call(&mut self) -> Vec<u8> {
        let effects = self.dispatch(EmuCommand::ExecuteGuestCall(request(0x1000, 8)));
        assert!(effects.potentially_dirty);
        let Some(EmuResponse::GuestCallCompleted {
            instructions,
            undo_state,
            ..
        }) = effects.response
        else {
            panic!("call failed")
        };
        assert_eq!((instructions, undo_state.len()), (8, 16));
        undo_state
    }

    fn undo(&mut self, token: Vec<u8>) {
        let effects = self.dispatch(EmuCommand::UndoGuestCall(token));
        assert!(effects.potentially_dirty);
        assert!(matches!(
            effects.response,
            Some(EmuResponse::GuestCallUndone)
        ));
    }

    fn refused_undo(&mut self, token: Vec<u8>) {
        let before = frontend(&self.backend);
        let host = session(&mut self.backend);
        let markers = self.markers.clone();
        let effects = self.dispatch(EmuCommand::UndoGuestCall(token));
        assert!(!effects.potentially_dirty);
        assert!(matches!(
            effects.response,
            Some(EmuResponse::GuestCallUndoFailed(_))
        ));
        assert_eq!(frontend(&self.backend), before);
        assert_eq!(session(&mut self.backend), host);
        assert_eq!(self.markers, markers);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_guest_call_common_command_matrix_restores_exact_frontend_and_paused_session() {
    for header in *b"UJE" {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            for kind in [ControllerKind::ThreeButton, ControllerKind::SixButton] {
                let mut owner = Owner::new(fixture(header, timing, kind, &[]));
                let before = frontend(&owner.backend);
                let host = session(&mut owner.backend);
                let caller = machine(&owner.backend).registers();
                let token = owner.call();
                let machine = machine(&owner.backend);
                assert_eq!(
                    (machine.registers().d[0], machine.registers().a[1]),
                    (8, 0xff0400)
                );
                assert_eq!(machine.registers().status_register(), 0x2300);
                assert_eq!(
                    (machine.registers().pc, machine.registers().stack_pointer()),
                    (caller.pc, caller.stack_pointer())
                );
                assert!(machine.cpu_clock().0 > host.cycles);
                assert_eq!(
                    (machine.peek(0xff0040), machine.peek(0xff0041)),
                    (Some(8), Some(0x42))
                );
                assert!(owner.backend.is_suspended());
                let after = session(&mut owner.backend);
                assert_eq!(after.trace.len(), 8);
                assert_eq!(
                    after.trace.iter().map(|row| row.pc).collect::<Vec<_>>(),
                    CALLEE.iter().map(|(pc, _)| *pc).collect::<Vec<_>>()
                );
                owner.undo(token);
                assert_eq!(frontend(&owner.backend), before);
                assert_eq!(session(&mut owner.backend), host);
            }
        }
    }
    let user = [0x207c, 0, 0, 0x4e60, 0x46fc, 0x0315, 0x60fe, 0x4e71];
    let mut owner = Owner::new(fixture(
        b'U',
        TimingModel::Standard,
        ControllerKind::ThreeButton,
        &[(0x218, &user)],
    ));
    assert_eq!(machine(&owner.backend).registers().stack_pointer(), 0);
    let before = frontend(&owner.backend);
    let token = owner.call();
    assert_eq!(machine(&owner.backend).registers().stack_pointer(), 0);
    assert_eq!(
        machine(&owner.backend).registers().status_register(),
        0x0300
    );
    owner.undo(token);
    assert_eq!(frontend(&owner.backend), before);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_guest_call_common_effects_accept_only_success_and_guard_consumed_worker_fault() {
    use crate::audio_recorder::AudioTimelineDiscontinuity::{DebuggerMutation, GuestCallUndo};
    let mut owner = Owner::new(standard());
    let before = frontend(&owner.backend);
    let token = owner.call();
    assert_eq!(owner.markers, [DebuggerMutation]);
    assert_eq!(owner.rewind.peek().unwrap().state_bytes, [1, 2, 3]);
    assert!(owner.backend.take_runtime_fault().is_none());
    owner.fault.latch(Some("worker-only fault".into()));
    assert_eq!(
        owner.fault.take_pending_delivery().as_deref(),
        Some("worker-only fault")
    );
    assert!(owner.fault.take_pending_delivery().is_none());
    let called = frontend(&owner.backend);
    let host = session(&mut owner.backend);
    for _ in 0..2 {
        let effects = owner.dispatch(EmuCommand::ExecuteGuestCall(request(0x1000, 8)));
        assert!(!effects.potentially_dirty);
        let Some(EmuResponse::GuestCallFailed { error, .. }) = effects.response else {
            panic!("faulted call accepted")
        };
        assert!(error.contains("worker execution is faulted"));
        owner.refused_undo(token.clone());
        assert_eq!(frontend(&owner.backend), called);
        assert_eq!(session(&mut owner.backend), host);
        assert_eq!(owner.markers, [DebuggerMutation]);
        assert!(!owner.fault.can_step());
    }
    owner.fault = Default::default();
    owner.undo(token.clone());
    assert_eq!(frontend(&owner.backend), before);
    assert_eq!(owner.markers, [DebuggerMutation, GuestCallUndo]);
    owner.refused_undo(token);
    assert_eq!(owner.markers, [DebuggerMutation, GuestCallUndo]);

    let mut owner = Owner::new(standard());
    let token = owner.call();
    let pc = machine(&owner.backend).registers().pc;
    let patches = crate::cheats::parse_cheat_for_system(
        &format!(
            "{pc:06X}:13FC+{:06X}:005A+{:06X}:00B0+{:06X}:0000",
            pc + 2,
            pc + 4,
            pc + 6
        ),
        ActiveSystem::Md,
    )
    .unwrap()
    .0;
    machine_mut(&mut owner.backend)
        .install_rom_patches(&patches)
        .unwrap();
    let mut actions = DebugUiActions::none();
    actions.remove_breakpoints.push(pc);
    for _ in 0..2 {
        let mut config = BackendRuntimeConfig::new(&actions);
        config.debug_step = true;
        config.opcode_log_enabled = true;
        owner.backend.apply_runtime_config(config);
    }
    assert!(
        owner
            .backend
            .take_runtime_fault()
            .unwrap()
            .contains("machine fault")
    );
    assert!(owner.backend.take_runtime_fault().is_none());
    assert!(owner.backend.encode_state_bytes().is_err());
    assert!(owner.fault.can_step());
    let machine = machine(&owner.backend).clone();
    let host = session(&mut owner.backend);
    for command in [
        EmuCommand::ExecuteGuestCall(request(0x1000, 8)),
        EmuCommand::UndoGuestCall(token),
    ] {
        let effects = owner.dispatch(command);
        assert!(!effects.potentially_dirty);
        let error = match effects.response {
            Some(EmuResponse::GuestCallFailed { error, .. })
            | Some(EmuResponse::GuestCallUndoFailed(error)) => error,
            _ => panic!("faulted backend accepted a guest-call command"),
        };
        assert!(error.contains("execution is faulted"));
        assert_eq!(self::machine(&owner.backend), &machine);
        assert_eq!(session(&mut owner.backend), host);
        assert_eq!(owner.markers, [DebuggerMutation]);
    }
}

mod input_edits;

mod trace_stream;

mod sound_trace;

mod sound_watches;
