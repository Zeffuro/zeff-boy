use super::*;
use crate::hardware::bus::ownership::{Requester, Reservation};
use crate::sound_trace::SoundTraceContinuation;
use zeff_emu_common::debug::{
    InstructionTraceStore, TraceObservationEnd, TraceObservationStart, TraceWriteKind,
};

pub(super) fn traced(
    rig: &mut Rig,
    continuation: &mut SoundTraceContinuation,
    output: &mut InstructionTraceStore,
    begin: u64,
    end: u64,
    occupied: &[Reservation],
) {
    let mut interval = SoundInterval::new(begin..end, &mut rig.audio);
    interval.occupied = occupied;
    rig.schedule.advance_owned(
        &mut rig.cpu,
        &mut rig.memory,
        &mut rig.cartridge,
        &mut rig.ram,
        (interval, &mut continuation.attach(output)),
    );
}

pub(super) fn normal(rig: &mut Rig, begin: u64, end: u64, occupied: &[Reservation]) {
    let mut interval = SoundInterval::new(begin..end, &mut rig.audio);
    interval.occupied = occupied;
    rig.schedule.advance_owned(
        &mut rig.cpu,
        &mut rig.memory,
        &mut rig.cartridge,
        &mut rig.ram,
        (interval, &mut crate::sound_trace::NoSoundCapture),
    );
}

pub(super) fn output() -> InstructionTraceStore {
    let mut output = InstructionTraceStore::default();
    output.set_enabled(true);
    output
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_trace_actual_deadlines_publish_ld_same_value_store_and_nop_once() {
    let mut rig = Rig::new(&[0x3e, 0x5a, 0x32, 0x00, 0x10, 0x00, 0x76], false);
    rig.memory.ram[0x1000] = 0x5a;
    let mut control = rig.clone();
    let mut continuation = SoundTraceContinuation::default();
    let mut output = output();
    let mut cursor = 0;
    for _ in 0..MAX_SEGMENTS {
        if output.len() == 3 {
            break;
        }
        let rows = output.len();
        let before_cpu = rig.cpu.clone();
        let before_memory = rig.memory.clone();
        traced(
            &mut rig,
            &mut continuation,
            &mut output,
            cursor,
            cursor + 1,
            &[],
        );
        normal(&mut control, cursor, cursor + 1, &[]);
        let pending = rig
            .schedule
            .pending
            .clone()
            .expect("actual pending segment");
        let deadline = pending.deadline;
        assert!(deadline > cursor + 1);
        assert_eq!(rig.cpu, before_cpu);
        assert_eq!(rig.memory, before_memory);
        assert_eq!(output.len(), rows);
        traced(
            &mut rig,
            &mut continuation,
            &mut output,
            cursor + 1,
            deadline - 1,
            &[],
        );
        normal(&mut control, cursor + 1, deadline - 1, &[]);
        assert_eq!(output.len(), rows);
        assert_eq!(rig.schedule.pending.as_ref(), Some(&pending));
        traced(
            &mut rig,
            &mut continuation,
            &mut output,
            deadline - 1,
            deadline,
            &[],
        );
        normal(&mut control, deadline - 1, deadline, &[]);
        assert!(rig.schedule.fault.is_none());
        assert_eq!(rig.cpu, pending.cpu);
        assert_eq!(rig.memory.ram[0x1000], 0x5a);
        equal(&rig, &control);
        cursor = deadline;
    }
    let rows = output.iter().copied().collect::<Vec<_>>();
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows.iter()
            .map(|r| (
                r.pc,
                r.instruction_bytes().to_vec(),
                r.cycle,
                r.cycle + r.boundary.unwrap().cpu_clocks
            ))
            .collect::<Vec<_>>(),
        [
            (0, vec![0x3e, 0x5a], 0, 7),
            (2, vec![0x32, 0x00, 0x10], 7, 20),
            (5, vec![0], 20, 24)
        ]
    );
    assert!(rows.iter().all(
        |r| r.observation.start == TraceObservationStart::Instruction
            && r.observation.frame_knowledge
                == zeff_emu_common::debug::TraceFrameKnowledge::Unknown
            && r.observation.end == TraceObservationEnd::Retired
            && r.physical_rom_offset.is_none()
    ));
    assert!(rows[0].writes().is_empty() && rows[2].writes().is_empty());
    assert_eq!(rows[1].writes().len(), 1);
    let write = rows[1].writes()[0];
    assert_eq!(
        (write.address, write.new_value, write.old_value, write.kind),
        (0x1000, 0x5a, None, TraceWriteKind::Memory)
    );
    assert_eq!(continuation.gaps(), 0);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_trace_epoch_commit_materialization_and_rejected_future_keep_exact_rows() {
    for cut in 1..130 {
        let program = [
            0x3e, 0x5a, 0x32, 0x00, 0x10, 0xdd, 0x21, 0x34, 0x12, 0x00, 0x76,
        ];
        let mut epoch = Rig::new(&program, true);
        let mut plain = Rig::new(&program, false);
        let mut a = SoundTraceContinuation::default();
        let mut b = SoundTraceContinuation::default();
        let mut rows_a = output();
        let mut rows_b = output();
        traced(&mut epoch, &mut a, &mut rows_a, 0, cut, &[]);
        traced(&mut plain, &mut b, &mut rows_b, 0, cut, &[]);
        epoch
            .schedule
            .materialize_epoch(&mut epoch.cpu, &epoch.memory, &mut a.attach(&mut rows_a));
        assert_eq!(
            rows_a.iter().copied().collect::<Vec<_>>(),
            rows_b.iter().copied().collect::<Vec<_>>()
        );
        let occupied = [Reservation {
            requester: Requester::VDP,
            ticks: cut..cut + 20,
        }];
        traced(&mut epoch, &mut a, &mut rows_a, cut, 900, &occupied);
        traced(&mut plain, &mut b, &mut rows_b, cut, 900, &occupied);
        epoch.schedule.finish_epoch_field(
            &mut epoch.cpu,
            &epoch.memory,
            &mut a.attach(&mut rows_a),
        );
        equal(&epoch, &plain);
        assert_eq!(
            rows_a.iter().copied().collect::<Vec<_>>(),
            rows_b.iter().copied().collect::<Vec<_>>()
        );
        assert_eq!(a.gaps(), 0);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_trace_rmw_shortening_restores_actual_pending_write_witness() {
    let mut rig = Rig::new(&[0x3e, 0x5a, 0x32, 0x00, 0x80, 0x76], false);
    rig.memory.bank = 0x1fe;
    let mut control = rig.clone();
    let mut continuation = SoundTraceContinuation::default();
    let mut rows = output();
    let mut cursor = 0;
    loop {
        traced(
            &mut rig,
            &mut continuation,
            &mut rows,
            cursor,
            cursor + 1,
            &[],
        );
        normal(&mut control, cursor, cursor + 1, &[]);
        let pending = rig.schedule.pending.as_ref().unwrap();
        if pending.any_bank_access {
            cursor += 1;
            break;
        }
        let deadline = pending.deadline;
        traced(
            &mut rig,
            &mut continuation,
            &mut rows,
            cursor + 1,
            deadline,
            &[],
        );
        normal(&mut control, cursor + 1, deadline, &[]);
        cursor = deadline;
        assert!(cursor < 1000);
    }
    let pending = rig.schedule.pending.as_ref().unwrap();
    let reservation = Reservation {
        requester: Requester::MAIN_RMW,
        ticks: pending.start..pending.deadline + 100,
    };
    traced(
        &mut rig,
        &mut continuation,
        &mut rows,
        cursor,
        cursor + 1,
        std::slice::from_ref(&reservation),
    );
    normal(
        &mut control,
        cursor,
        cursor + 1,
        std::slice::from_ref(&reservation),
    );
    assert!(rig.schedule.rmw_checkpoint.is_some());
    let retained = rows.len();
    rig.schedule.finish_rmw(
        &reservation,
        cursor + 1,
        &mut continuation.attach(&mut rows),
    );
    control.schedule.finish_rmw(
        &reservation,
        cursor + 1,
        &mut crate::sound_trace::NoSoundCapture,
    );
    let deadline = rig.schedule.pending.as_ref().unwrap().deadline;
    traced(
        &mut rig,
        &mut continuation,
        &mut rows,
        cursor + 1,
        deadline,
        &[],
    );
    normal(&mut control, cursor + 1, deadline, &[]);
    equal(&rig, &control);
    assert_eq!(rows.len(), retained + 1);
    let store = rows.iter().last().unwrap();
    assert_eq!(store.instruction_bytes(), [0x32, 0x00, 0x80]);
    assert_eq!(store.writes().len(), 1);
    assert_eq!(
        (store.writes()[0].address, store.writes()[0].new_value),
        (0x8000, 0x5a)
    );
    assert_eq!(rig.ram[0], 0x5a);
    assert_eq!(
        store.cycle + store.boundary.unwrap().cpu_clocks,
        rig.cpu.cycles()
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_trace_prefix_overflow_io_and_halt_poll_keep_actual_consumed_history() {
    let mut program = vec![0xdd; 20];
    program.extend([0x21, 0x34, 0x12, 0x3e, 0x5a, 0xd3, 0x7f, 0x76]);
    let mut rig = Rig::new(&program, true);
    let mut control = rig.clone();
    let mut continuation = SoundTraceContinuation::default();
    let mut rows = output();
    traced(&mut rig, &mut continuation, &mut rows, 0, 3000, &[]);
    normal(&mut control, 0, 3000, &[]);
    rig.schedule.finish_epoch_field(
        &mut rig.cpu,
        &rig.memory,
        &mut continuation.attach(&mut rows),
    );
    control.schedule.finish_epoch_field(
        &mut control.cpu,
        &control.memory,
        &mut crate::sound_trace::NoSoundCapture,
    );
    equal(&rig, &control);
    let first = rows.iter().next().unwrap();
    assert_eq!(first.instruction_bytes(), [0xdd; 16]);
    assert_eq!(first.instruction_overflow, 7);
    assert_eq!(first.observation.start, TraceObservationStart::Instruction);
    assert_eq!(rig.cpu.regs().ix, 0x1234);
    let io = rows.iter().find(|r| r.pc == 25).unwrap();
    assert_eq!(io.instruction_bytes(), [0xd3, 0x7f]);
    assert_eq!(io.writes().len(), 1);
    assert_eq!(
        (
            io.writes()[0].address,
            io.writes()[0].new_value,
            io.writes()[0].kind
        ),
        (0x7f, 0x5a, TraceWriteKind::Io)
    );
    let halt = rows
        .iter()
        .find(|r| r.instruction_bytes() == [0x76])
        .unwrap();
    assert_eq!(
        halt.boundary.unwrap().kind,
        zeff_emu_common::debug::TraceBoundaryKind::Instruction
    );
    assert!(rows.iter().any(|r| r.instruction_bytes().is_empty()
        && r.boundary.unwrap().kind == zeff_emu_common::debug::TraceBoundaryKind::Stopped));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_trace_enable_pending_suffix_reset_disable_and_fault_do_not_invent_prefix() {
    for end in [
        TraceObservationEnd::Reset,
        TraceObservationEnd::Disabled,
        TraceObservationEnd::Fault,
    ] {
        let mut rig = Rig::new(&[0xdd, 0xdd, 0xdd, 0x21, 0x34, 0x12, 0x76], false);
        normal(&mut rig, 0, 1, &[]);
        let first = rig.schedule.pending.as_ref().unwrap().deadline;
        let mut continuation = SoundTraceContinuation::default();
        let mut rows = output();
        traced(&mut rig, &mut continuation, &mut rows, 1, first, &[]);
        assert!(rows.is_empty());
        assert_eq!(continuation.gaps(), 1);
        traced(
            &mut rig,
            &mut continuation,
            &mut rows,
            first,
            first + 1,
            &[],
        );
        let second = rig.schedule.pending.as_ref().unwrap().deadline;
        traced(
            &mut rig,
            &mut continuation,
            &mut rows,
            first + 1,
            second,
            &[],
        );
        assert!(rows.is_empty());
        match end {
            TraceObservationEnd::Reset => rig.schedule.reset_cpu(
                &mut rig.cpu,
                &mut rig.memory,
                &mut continuation.attach(&mut rows),
            ),
            TraceObservationEnd::Disabled => continuation.disable(&mut rows),
            TraceObservationEnd::Fault => {
                assert!(rig.schedule.pending.is_none());
                let pc = usize::from(rig.cpu.regs().pc);
                assert_eq!(pc, 2);
                rig.memory.ram[pc..pc + 3].copy_from_slice(&[0x3a, 0x00, 0x7f]);
                traced(
                    &mut rig,
                    &mut continuation,
                    &mut rows,
                    second,
                    second + 300,
                    &[],
                );
                assert!(rig.schedule.fault.is_some());
                assert!(matches!(
                    rig.schedule.fault,
                    Some(crate::SoundFault::UnsupportedAccess {
                        address: 0x7f00,
                        direction: zeff_m68k::bus::Direction::Read,
                    })
                ));
            }
            _ => unreachable!(),
        }
        let partial = rows.iter().next().unwrap();
        assert_eq!(partial.observation.start, TraceObservationStart::Suffix);
        assert_eq!(partial.observation.end, end);
        assert_eq!(partial.cycle, 4);
        assert_eq!(partial.instruction_overflow, 0);
        assert!(partial.instruction_bytes().starts_with(&[0xdd]));
    }
}
