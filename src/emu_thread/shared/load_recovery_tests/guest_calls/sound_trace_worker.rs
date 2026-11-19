use super::*;
use crate::ui::{InstructionTraceBatch, TraceCursor, TraceTarget};
use zeff_emu_common::debug::{TraceFrameKnowledge, TraceWriteKind};
use zeff_emu_common::time::MachineTiming;

fn fixture(header: u8) -> EmuBackend {
    let mut words = vec![0x33fc, 0x100, 0xa1, 0x1100, 0x33fc, 0x100, 0xa1, 0x1200];
    for (address, value) in [(0xc00011, 0x80), (0xc00011, 0x10), (0xc00011, 0x90)] {
        words.extend([0x13fc, value, (address >> 16) as u16, address as u16]);
    }
    for (address, value) in [0x3e, 0x5a, 0x32, 0, 0x10, 0, 0x76]
        .into_iter()
        .enumerate()
        .map(|(address, value)| (address as u16, value))
        .chain([(0x1000, 0x5a)])
    {
        words.extend([0x13fc, value, 0xa0, address]);
    }
    words.extend([
        0x33fc, 0x100, 0xa1, 0x1200, 0x33fc, 0, 0xa1, 0x1100, 0x60fe, 0x4e71,
    ]);
    let mut backend = guest_call_backend_with(
        header,
        zeff_md_core::TimingModel::Standard,
        zeff_md_core::ControllerKind::ThreeButton,
        &[(0x200, &words)],
        false,
    );
    backend.debug_suspend();
    backend
}

fn request(step: bool, cursor: TraceCursor) -> FrameInput {
    let mut input = guest_call_input(0);
    input.debug_step = step;
    input.snapshot.trace_cursor = cursor;
    input
}

fn control_run(backend: &mut EmuBackend, input: FrameInput) -> FrameResult {
    EmuThread::handle_step_frames(
        backend,
        input,
        &[],
        false,
        &mut zeff_emu_common::rewind::RewindBuffer::new(0, 1),
        &mut 10,
        &mut WorkerRuntimeFault::default(),
    )
}

fn batch(frame: FrameResult) -> InstructionTraceBatch {
    frame.ui_data.instruction_trace.unwrap()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_standard_sound_worker_opt_in_publishes_real_ld_store_nop_and_keeps_primary_and_pcm() {
    for header in *b"UJE" {
        let mut worker = EmuThread::spawn(fixture(header), false);
        let mut control = fixture(header);
        let cursor = TraceCursor {
            target: TraceTarget::Sound,
            ..Default::default()
        };
        let mut enable = request(false, cursor);
        enable.debug_actions.sound_trace.enabled = Some(true);
        enable.debug_actions.trace_enabled = Some(true);
        let initial = batch(guest_call_deliver(&worker, enable));
        assert!(initial.enabled && initial.entries.is_empty());
        assert!(initial.sound.unwrap().rejection.is_none());
        let identity = initial.stream;
        let mut saw_pending = false;
        let mut last = Vec::new();
        for _ in 0..28 {
            let EmuBackend::Md(md) = &control else {
                unreachable!()
            };
            if let Some(deadline) = md.core().sound_deadline() {
                assert!(deadline.get() >= md.core().timing_snapshot().now().get());
                saw_pending = true;
                let inert = batch(guest_call_deliver(&worker, request(false, cursor)));
                assert_eq!(inert.entries, last);
            }
            let actual = guest_call_deliver(&worker, request(true, cursor));
            let expected = control_run(&mut control, request(true, TraceCursor::default()));
            assert!(actual.runtime_fault.is_none() && expected.runtime_fault.is_none());
            assert_eq!(pcm(&actual), pcm(&expected));
            assert_eq!(
                actual.ui_data.cpu_debug.as_ref().unwrap().register_lines,
                expected.ui_data.cpu_debug.as_ref().unwrap().register_lines
            );
            assert_eq!(capture(&worker), control.encode_state_bytes().unwrap());
            let rows = batch(actual);
            assert_eq!(rows.target, TraceTarget::Sound);
            assert_eq!(rows.stream, identity);
            last = rows.entries;
            if last.len() >= 3 {
                break;
            }
        }
        assert!(saw_pending);
        assert!(last.len() >= 3);
        for (row, (bytes, terminal)) in last.iter().take(3).zip([
            (&[0x3e, 0x5a][..], 7),
            (&[0x32, 0, 0x10][..], 20),
            (&[0][..], 24),
        ]) {
            assert_eq!(row.instruction_bytes(), bytes);
            assert_eq!(row.cycle + row.boundary.unwrap().cpu_clocks, terminal);
            assert_eq!(
                row.observation.frame_knowledge,
                TraceFrameKnowledge::Unknown
            );
            assert!(row.physical_rom_offset.is_none());
        }
        let writes = last[1].writes();
        assert_eq!(writes.len(), 1);
        assert_eq!(
            (
                writes[0].kind,
                writes[0].address,
                writes[0].old_value,
                writes[0].new_value
            ),
            (TraceWriteKind::Memory, 0x1000, None, 0x5a)
        );
        let primary = batch(guest_call_deliver(
            &worker,
            request(false, TraceCursor::default()),
        ));
        assert!(!primary.entries.is_empty());
        assert!(
            primary
                .entries
                .iter()
                .all(|row| row.mode == TraceExecMode::M68000)
        );
        let mut resume = guest_call_input(2);
        resume.debug_continue = true;
        let future = guest_call_deliver(&worker, resume);
        let mut resume = guest_call_input(2);
        resume.debug_continue = true;
        let expected = control_run(&mut control, resume);
        assert_eq!(pcm(&future), pcm(&expected));
        assert!(future.audio_samples.iter().any(|sample| *sample != 0.0));
        assert_eq!(capture(&worker), control.encode_state_bytes().unwrap());
        worker.shutdown();
    }
}

mod watchpoints;
