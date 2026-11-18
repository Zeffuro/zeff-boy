use super::watches::{advance, rule};
use super::*;
use crate::constants::SOUND_CPU_DIVISOR;
use crate::hardware::bus::ownership::{Requester, Reservation};
use crate::sound_trace::{SoundAccessKind, SoundWatchRule, SoundWatchSpace};
use zeff_emu_common::debug::WatchType;

fn sound_start(core: &Emulator) -> u64 {
    core.bus.now.get().next_multiple_of(SOUND_CPU_DIVISOR)
}

fn arm(core: &Emulator, continuation: &mut SoundTraceContinuation, rule: SoundWatchRule) {
    continuation.edit_watchpoint(core, rule, true).unwrap();
}
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_watch_status_replay_replaces_prepared_read_and_publishes_one_actual_value() {
    let mut actual = machine(Region::NtscUs, &[0x60fe, 0x4e71], &[0x3a, 0, 0x40, 0x76]);
    let mut continuation = SoundTraceContinuation::default();
    let mut output = InstructionTraceStore::new(0);
    arm(
        &actual,
        &mut continuation,
        rule(0x4000, 0x4000, WatchType::Read),
    );
    let start = sound_start(&actual);
    actual.bus.audio.write_opn2(start, 0, 0x2a);
    let mut ordinary = actual.clone();
    advance(
        &mut actual,
        &mut continuation.attach(&mut output),
        start,
        start + 151,
    );
    advance(
        &mut ordinary,
        &mut crate::sound_trace::NoSoundCapture,
        start,
        start + 151,
    );
    assert_eq!(
        actual.sound_schedule.pending.as_ref().unwrap().deadline,
        start + 195
    );
    assert!(continuation.watch_hit().is_none());
    assert_eq!(continuation.watchpoints()[0].hits, 0);
    actual.bus.audio.write_opn2(start + 160, 1, 0x90);
    ordinary.bus.audio.write_opn2(start + 160, 1, 0x90);
    advance(
        &mut actual,
        &mut continuation.attach(&mut output),
        start + 160,
        start + 194,
    );
    assert!(continuation.watch_hit().is_none());
    advance(
        &mut ordinary,
        &mut crate::sound_trace::NoSoundCapture,
        start + 160,
        start + 194,
    );
    advance(
        &mut actual,
        &mut continuation.attach(&mut output),
        start + 194,
        start + 195,
    );
    advance(
        &mut ordinary,
        &mut crate::sound_trace::NoSoundCapture,
        start + 194,
        start + 195,
    );
    assert_eq!(actual.sound_cpu, ordinary.sound_cpu);
    assert_eq!(actual.bus.audio, ordinary.bus.audio);
    assert_eq!(actual.sound_cpu.regs().a, 0x80);
    assert_eq!(continuation.watchpoints()[0].hits, 1);
    assert_eq!(continuation.watch_hit().unwrap().access.value, 0x80);
    assert!(!output.is_enabled() && output.is_empty());
}
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_watch_io_space_halt_exclusion_and_paired_fork_restore_actual_attempts() {
    let mut actual = machine(
        Region::NtscUs,
        &[0x60fe, 0x4e71],
        &[0xdb, 0x12, 0xd3, 0x12, 0x76],
    );
    let mut continuation = SoundTraceContinuation::default();
    let mut output = InstructionTraceStore::new(0);
    let io = SoundWatchRule {
        space: SoundWatchSpace::Io,
        start: 0x12,
        end: 0x12,
        kind: WatchType::ReadWrite,
    };
    arm(&actual, &mut continuation, io);
    arm(&actual, &mut continuation, rule(0, 0x1fff, WatchType::Read));
    let before = actual.clone();
    let gaps = continuation.watch_gaps();
    let fork = continuation.fork(&mut actual, &mut output);
    for _ in 0..32 {
        actual
            .step_instruction_sound_traced(&mut continuation.attach(&mut output))
            .unwrap();
    }
    assert_eq!(continuation.watchpoints()[0].hits, 2);
    assert_eq!(continuation.watchpoints()[1].hits, 0);
    let first = continuation.watch_hit().unwrap();
    assert_eq!(
        (
            first.access.space,
            first.access.kind,
            first.access.address,
            first.access.value
        ),
        (
            SoundWatchSpace::Io,
            SoundAccessKind::Read,
            0x12,
            crate::constants::SOUND_DISCONNECTED_BYTE
        )
    );
    fork.restore(&mut actual, &mut continuation, &mut output);
    exact(&actual, &before);
    assert_eq!(continuation.watchpoints()[0].hits, 0);
    assert!(continuation.watch_hit().is_none());
    assert_eq!(continuation.watch_gaps(), gaps);
    let mut control = before;
    for _ in 0..32 {
        actual
            .step_instruction_sound_traced(&mut continuation.attach(&mut output))
            .unwrap();
        control.step_instruction().unwrap();
    }
    exact(&actual, &control);
    assert_eq!(continuation.watchpoints()[0].hits, 2);
    assert_eq!(continuation.watch_hit(), Some(first));
}
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_watch_rmw_reservation_restore_has_no_speculative_count() {
    for toggle_trace in [false, true] {
        let mut core = machine(Region::NtscUs, &[0x60fe, 0x4e71], &[0x3a, 0, 0x80, 0x76]);
        let mut continuation = SoundTraceContinuation::default();
        let mut output = InstructionTraceStore::new(0);
        output.set_enabled(toggle_trace);
        arm(
            &core,
            &mut continuation,
            rule(0x8000, 0x8000, WatchType::Read),
        );
        let start = sound_start(&core);
        advance(
            &mut core,
            &mut continuation.attach(&mut output),
            start,
            start + 151,
        );
        let cpu = core.sound_cpu.clone();
        let reservation = Reservation {
            requester: Requester::MAIN_RMW,
            ticks: start + 150..start + 900,
        };
        let mut interval = crate::hardware::sound::schedule::SoundInterval::new(
            start + 151..start + 160,
            &mut core.bus.audio,
        );
        interval.occupied = std::slice::from_ref(&reservation);
        core.sound_schedule.advance_owned(
            &mut core.sound_cpu,
            &mut core.bus.sound,
            &mut core.bus.cartridge,
            &mut core.bus.ram,
            (interval, &mut continuation.attach(&mut output)),
        );
        let reserved = core.sound_schedule.pending.as_ref().unwrap().deadline;
        assert_eq!(core.sound_cpu, cpu);
        assert_eq!(continuation.watchpoints()[0].hits, 0);
        let gaps = continuation.watch_gaps();
        if toggle_trace {
            continuation.trace_discontinuity(&mut output);
            output.clear();
            continuation.disable(&mut output);
            output.set_enabled(true);
            continuation.trace_discontinuity(&mut output);
            assert_eq!(continuation.watch_gaps(), gaps);
        }
        core.sound_schedule.finish_rmw(
            &reservation,
            start + 160,
            &mut continuation.attach(&mut output),
        );
        let deadline = core.sound_schedule.pending.as_ref().unwrap().deadline;
        assert!(deadline < reserved);
        advance(
            &mut core,
            &mut continuation.attach(&mut output),
            start + 160,
            deadline - 1,
        );
        assert_eq!(continuation.watchpoints()[0].hits, 0);
        advance(
            &mut core,
            &mut continuation.attach(&mut output),
            deadline - 1,
            deadline,
        );
        assert_eq!(continuation.watchpoints()[0].hits, 1);
        assert!(!continuation.watch_blocked());
        assert_eq!(continuation.watch_gaps(), gaps);
        let hit = continuation.watch_hit().unwrap();
        assert_eq!(hit.access.address, 0x8000);
        assert_eq!(output.is_enabled(), toggle_trace);
        assert!(output.is_empty());
        continuation.disable(&mut output);
        continuation.trace_discontinuity(&mut output);
        output.clear();
        assert_eq!(continuation.watch_hit(), Some(hit));
        assert_eq!(continuation.watchpoints()[0].hits, 1);
        assert_eq!(continuation.watch_gaps(), gaps);
    }
}
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_watch_failed_ports_restore_accepted_prefix_hits_and_owned_witness() {
    let mut core = machine(Region::NtscUs, &[0x60fe, 0x4e71], &[0xdb, 0x12, 0x18, 0xfc]);
    let mut continuation = SoundTraceContinuation::default();
    let mut output = InstructionTraceStore::new(0);
    arm(
        &core,
        &mut continuation,
        SoundWatchRule {
            space: SoundWatchSpace::Io,
            start: 0x12,
            end: 0x12,
            kind: WatchType::Read,
        },
    );
    for _ in 0..128 {
        core.step_instruction_sound_traced(&mut continuation.attach(&mut output))
            .unwrap();
        if core.sound_schedule.pending.as_ref().is_some_and(|p| {
            p.instruction.pc == 0
                && p.cpu.regs().a == crate::constants::SOUND_DISCONNECTED_BYTE
                && p.deadline.saturating_sub(core.bus.now.get()) <= 28
        }) {
            break;
        }
    }
    let before = core.clone();
    let counts = continuation.watchpoints().to_vec();
    let hit = continuation.watch_hit();
    let gaps = continuation.watch_gaps();
    let first = DebugPortWrite::Byte {
        address: 0xa11100,
        value: 1,
    };
    let mut prefix = core.clone();
    let mut witness = SoundTraceContinuation::default();
    let mut rows = InstructionTraceStore::new(0);
    witness
        .attach(&mut rows)
        .rollback(continuation.attach(&mut output).checkpoint());
    prefix
        .debug_write_ports_sound_traced(&[first], &mut witness.attach(&mut rows))
        .unwrap();
    assert!(prefix.sound_cpu.cycles() > before.sound_cpu.cycles());
    assert!(witness.watchpoints()[0].hits > counts[0].hits);
    assert!(
        core.debug_write_ports_sound_traced(
            &[
                first,
                DebugPortWrite::Word {
                    address: 0xc00004,
                    value: 0x8c80
                }
            ],
            &mut continuation.attach(&mut output)
        )
        .is_err()
    );
    exact(&core, &before);
    assert_eq!(continuation.watchpoints(), counts);
    assert_eq!(continuation.watch_hit(), hit);
    assert_eq!(continuation.watch_gaps(), gaps);
    assert!(output.is_empty() && !output.is_enabled());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_watch_enable_gap_omits_unwitnessed_pending_and_keeps_future_accesses() {
    let mut core = machine(
        Region::NtscUs,
        &[0x60fe, 0x4e71],
        &[0x3a, 0, 0x10, 0x18, 0xfb],
    );
    core.bus.sound.ram[0x1000] = 0x5a;
    let start = sound_start(&core);
    advance(
        &mut core,
        &mut crate::sound_trace::NoSoundCapture,
        start,
        start + 151,
    );
    let mut continuation = SoundTraceContinuation::default();
    let mut output = InstructionTraceStore::new(0);
    arm(
        &core,
        &mut continuation,
        rule(0x1000, 0x1000, WatchType::Read),
    );
    let gaps = continuation.watch_gaps();
    let deadline = core.sound_schedule.pending.as_ref().unwrap().deadline;
    advance(
        &mut core,
        &mut continuation.attach(&mut output),
        start + 151,
        deadline,
    );
    assert_eq!(continuation.watchpoints()[0].hits, 0);
    assert!(continuation.watch_hit().is_none());
    assert!(continuation.watch_gaps() > gaps);
    advance(
        &mut core,
        &mut continuation.attach(&mut output),
        deadline,
        start + 1000,
    );
    assert!(continuation.watchpoints()[0].hits > 0);
    assert_eq!(continuation.watch_hit().unwrap().access.value, 0x5a);
}
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_watch_irq_from_halt_matches_actual_stack_but_never_ack_or_halt_poll() {
    let mut core = machine(
        Region::NtscUs,
        &[0x60fe, 0x4e71],
        &[0x31, 0, 0x18, 0xed, 0x56, 0xfb, 0x76],
    );
    let mut continuation = SoundTraceContinuation::default();
    let mut output = InstructionTraceStore::new(0);
    arm(
        &core,
        &mut continuation,
        rule(0, 0x3fff, WatchType::ReadWrite),
    );
    for _ in 0..32 {
        core.step_instruction_sound_traced(&mut continuation.attach(&mut output))
            .unwrap();
    }
    assert!(core.sound_cpu.is_halted());
    assert_eq!(continuation.watchpoints()[0].hits, 0);
    core.bus.sound.irq_asserted = true;
    for _ in 0..16 {
        core.step_instruction_sound_traced(&mut continuation.attach(&mut output))
            .unwrap();
    }
    assert_eq!(continuation.watchpoints()[0].hits, 2);
    let hit = continuation.watch_hit().unwrap();
    assert_eq!(
        (hit.access.kind, hit.access.address, hit.access.value),
        (SoundAccessKind::Write, 0x17ff, 0)
    );
}
