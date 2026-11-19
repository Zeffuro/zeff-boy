use super::*;
use crate::sound_trace::{SoundCapture, SoundTraceContinuation};
use zeff_emu_common::debug::{InstructionTraceStore, TraceBoundaryKind, TraceObservationEnd};

fn machine(region: Region, main: &[u16], program: &[u8]) -> Emulator {
    let mut rom = vec![0; 0x4000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[30 * 4..30 * 4 + 4].copy_from_slice(&0x300_u32.to_be_bytes());
    rom[0x300..0x304].copy_from_slice(&[0x4e, 0x73, 0x4e, 0x71]);
    for (index, word) in main.iter().enumerate() {
        rom[0x200 + 2 * index..0x202 + 2 * index].copy_from_slice(&word.to_be_bytes());
    }
    let mut core = Emulator::new(
        rom,
        LoadConfig {
            region,
            concurrent_sound: true,
        },
    )
    .unwrap();
    core.bus.sound.ram[..program.len()].copy_from_slice(program);
    core.bus.sound.set_reset(false);
    core
}

fn store() -> InstructionTraceStore {
    let mut rows = InstructionTraceStore::default();
    rows.set_enabled(true);
    rows
}
fn rows(output: &InstructionTraceStore) -> Vec<zeff_emu_common::debug::InstructionTraceRecord> {
    output.iter().copied().collect()
}
fn exact(actual: &Emulator, control: &Emulator) {
    assert_eq!(actual, control);
    assert_eq!(
        actual.encode_state().unwrap(),
        control.encode_state().unwrap()
    );
    assert_eq!(
        actual.encode_frontend_state().unwrap(),
        control.encode_frontend_state().unwrap()
    );
    assert_eq!(
        actual.encode_debugger_checkpoint().unwrap(),
        control.encode_debugger_checkpoint().unwrap()
    );
    assert_eq!(actual.framebuffer(), control.framebuffer());
    let mut a = actual.clone();
    let mut b = control.clone();
    assert_eq!(
        a.drain_audio_samples()
            .into_iter()
            .map(f32::to_bits)
            .collect::<Vec<_>>(),
        b.drain_audio_samples()
            .into_iter()
            .map(f32::to_bits)
            .collect::<Vec<_>>()
    );
}

const TONE: &[u8] = &[
    0x3e, 0x80, 0x32, 0x11, 0x7f, 0x3e, 0x10, 0x32, 0x11, 0x7f, 0x3e, 0x90, 0x32, 0x11, 0x7f, 0,
    0x18, 0xfe,
];

#[derive(Default)]
struct MainWords(Vec<u16>);
impl InstructionObserver for MainWords {
    fn instruction_word(&mut self, _address: u32, value: u16) {
        self.0.push(value);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_trace_core_api_keeps_main_callbacks_raw_frontend_owned_fields_and_nonzero_pcm() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut actual = machine(region, &[0x60fe, 0x4e71], TONE);
        let mut control = actual.clone();
        let mut continuation = SoundTraceContinuation::default();
        let mut output = store();
        let mut main = MainWords::default();
        let mut baseline = MainWords::default();
        for _ in 0..64 {
            assert_eq!(
                actual.step_instruction_observers(&mut main, &mut continuation.attach(&mut output)),
                control.step_instruction_traced(&mut baseline)
            );
            exact(&actual, &control);
        }
        assert_eq!(main.0, baseline.0);
        let writes = output
            .iter()
            .flat_map(|r| r.writes())
            .map(|w| (w.address, w.new_value))
            .collect::<Vec<_>>();
        assert_eq!(writes, [(0x7f11, 0x80), (0x7f11, 0x10), (0x7f11, 0x90)]);
        for _ in 0..2 {
            actual
                .step_field_sound_traced(&mut continuation.attach(&mut output))
                .unwrap();
            control.step_field().unwrap();
            exact(&actual, &control);
            assert!(actual.sound_schedule.epoch.is_none());
            let mut samples = Vec::new();
            actual.clone().drain_audio_samples_into(&mut samples);
            assert!(samples.iter().any(|sample| *sample != 0.0));
        }
        assert_eq!(continuation.gaps(), 0);
        actual
            .reset_cpu_sound_traced(&mut continuation.attach(&mut output))
            .unwrap();
        control.reset_cpu().unwrap();
        exact(&actual, &control);
        assert_eq!(
            actual
                .warm_reset_sound_traced(&mut continuation.attach(&mut output))
                .unwrap(),
            control.warm_reset().unwrap()
        );
        exact(&actual, &control);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_trace_stop_irq_and_reset_use_real_empty_byte_interrupt_and_interrupted_rows() {
    let program = [0x31, 0xfd, 0x1f, 0xed, 0x56, 0xfb, 0x76, 0x18, 0xfd];
    let mut actual = machine(Region::NtscUs, &[0x4e72, 0x2000, 0x60fe, 0x4e71], &program);
    actual.bus.sound.ram[0x38..0x3b].copy_from_slice(&[0xfb, 0xed, 0x4d]);
    let mut control = actual.clone();
    let mut continuation = SoundTraceContinuation::default();
    let mut output = store();
    for _ in 0..64 {
        assert_eq!(
            actual.step_instruction_sound_traced(&mut continuation.attach(&mut output)),
            control.step_instruction()
        );
    }
    assert!(actual.main_cpu_stopped() && actual.sound_cpu.is_halted());
    assert!(actual.set_sound_interrupt(true) && control.set_sound_interrupt(true));
    assert!(actual.set_interrupt_level(6) && control.set_interrupt_level(6));
    for _ in 0..32 {
        assert_eq!(
            actual.step_instruction_sound_traced(&mut continuation.attach(&mut output)),
            control.step_instruction()
        );
    }
    exact(&actual, &control);
    assert!(
        output
            .iter()
            .any(|r| r.boundary.unwrap().kind == TraceBoundaryKind::Interrupt
                && r.instruction_bytes().is_empty())
    );
    assert!(
        output
            .iter()
            .any(|r| r.boundary.unwrap().kind == TraceBoundaryKind::Stopped
                && r.instruction_bytes().is_empty())
    );
    let retained = rows(&output);
    let prior = actual.clone();
    let gaps = continuation.gaps();
    assert!(
        actual
            .warm_reset_sound_traced(&mut continuation.attach(&mut output))
            .is_err()
    );
    assert_eq!(actual, prior);
    assert_eq!(rows(&output), retained);
    assert_eq!(continuation.gaps(), gaps);
    assert!(actual.reset_sound_cpu_traced(&mut continuation.attach(&mut output)));
    assert!(control.reset_sound_cpu());
    exact(&actual, &control);
    assert!(
        output
            .iter()
            .all(|r| r.observation.end == TraceObservationEnd::Retired
                || r.observation.end == TraceObservationEnd::Reset)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_trace_timed_port_failure_restores_paired_output_partial_and_counter() {
    let mut actual = machine(Region::NtscUs, &[0x60fe, 0x4e71], TONE);
    let mut continuation = SoundTraceContinuation::default();
    let mut output = store();
    let mut ready = false;
    for _ in 0..128 {
        actual
            .step_instruction_sound_traced(&mut continuation.attach(&mut output))
            .unwrap();
        if actual
            .sound_schedule
            .pending
            .as_ref()
            .is_some_and(|p| p.deadline.saturating_sub(actual.bus.now.get()) <= 28)
        {
            ready = true;
            break;
        }
    }
    assert!(ready);
    let before = actual.clone();
    let checkpoint = continuation.attach(&mut output).checkpoint();
    let mut saved = before.clone();
    let mut saved_continuation = SoundTraceContinuation::default();
    let mut saved_output = store();
    saved_continuation
        .attach(&mut saved_output)
        .rollback(checkpoint);
    let before_rows = rows(&output);
    let before_gaps = continuation.gaps();
    let first = DebugPortWrite::Byte {
        address: 0xa11100,
        value: 1,
    };
    let mut prefix = actual.clone();
    let mut prefix_continuation = SoundTraceContinuation::default();
    let mut prefix_output = store();
    prefix_continuation
        .attach(&mut prefix_output)
        .rollback(continuation.attach(&mut output).checkpoint());
    prefix
        .debug_write_ports_sound_traced(
            &[first],
            &mut prefix_continuation.attach(&mut prefix_output),
        )
        .unwrap();
    assert!(prefix.sound_cpu.cycles() > before.sound_cpu.cycles());
    let writes = [
        first,
        DebugPortWrite::Word {
            address: 0xc00004,
            value: 0x8c80,
        },
    ];
    assert!(matches!(
        actual.debug_write_ports_sound_traced(&writes, &mut continuation.attach(&mut output)),
        Err(DebugPortError::Execution { index: 1, .. })
    ));
    assert_eq!(actual, before);
    assert_eq!(rows(&output), before_rows);
    assert_eq!(continuation.gaps(), before_gaps);
    assert!(actual.reset_sound_cpu_traced(&mut continuation.attach(&mut output)));
    assert!(saved.reset_sound_cpu_traced(&mut saved_continuation.attach(&mut saved_output)));
    exact(&actual, &saved);
    assert_eq!(rows(&output), rows(&saved_output));
    for _ in 0..8 {
        assert_eq!(
            actual.step_instruction_sound_traced(&mut continuation.attach(&mut output)),
            saved.step_instruction_sound_traced(&mut saved_continuation.attach(&mut saved_output))
        );
    }
    exact(&actual, &saved);
    assert_eq!(rows(&output), rows(&saved_output));
}

mod transactions;

mod watches;

mod watch_bounds;
mod watch_lifecycle;
