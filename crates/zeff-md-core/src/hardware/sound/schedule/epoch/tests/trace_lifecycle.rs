use super::trace::{normal, output, traced};
use super::*;
use crate::sound_trace::SoundTraceContinuation;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_trace_disabled_execution_cannot_revive_staged_prefix_history() {
    use zeff_emu_common::debug::{TraceObservationEnd, TraceObservationStart};
    let mut rig = Rig::new(
        &[0xdd, 0xdd, 0xdd, 0xdd, 0xdd, 0xdd, 0x21, 0x34, 0x12, 0x76],
        false,
    );
    let mut control = rig.clone();
    let mut continuation = SoundTraceContinuation::default();
    let mut rows = output();
    let mut cursor = 0;
    for index in 0..2 {
        traced(
            &mut rig,
            &mut continuation,
            &mut rows,
            cursor,
            cursor + 1,
            &[],
        );
        normal(&mut control, cursor, cursor + 1, &[]);
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
        cursor = deadline;
        if index == 0 {
            continuation.disable(&mut rows);
        }
    }
    assert_eq!(rig.cpu.cycles(), 8);
    assert_eq!(rows.len(), 1);
    let interrupted = rows.iter().next().unwrap();
    assert_eq!(interrupted.instruction_bytes(), [0xdd]);
    assert_eq!(interrupted.observation.end, TraceObservationEnd::Disabled);
    rows.set_enabled(true);
    traced(
        &mut rig,
        &mut continuation,
        &mut rows,
        cursor,
        cursor + 500,
        &[],
    );
    normal(&mut control, cursor, cursor + 500, &[]);
    equal(&rig, &control);
    let suffix = rows
        .iter()
        .find(|r| r.observation.start == TraceObservationStart::Suffix)
        .unwrap();
    assert_eq!(
        suffix.instruction_bytes(),
        [0xdd, 0xdd, 0xdd, 0xdd, 0x21, 0x34, 0x12]
    );
    assert_eq!(suffix.cycle, 8);
    assert_eq!(suffix.instruction_overflow, 0);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_trace_interruption_retains_accepted_disconnected_write_and_excludes_failed_tail()
{
    use zeff_emu_common::debug::TraceObservationEnd;
    for end in [
        TraceObservationEnd::Fault,
        TraceObservationEnd::Reset,
        TraceObservationEnd::Disabled,
    ] {
        let mut rig = Rig::new(&[0x21, 0x34, 0x12, 0x22, 0xff, 0x7e, 0x76], false);
        let mut control = rig.clone();
        let mut continuation = SoundTraceContinuation::default();
        let mut rows = output();
        let mut cursor = 0;
        while rig.cpu.cycles() < 23 {
            traced(
                &mut rig,
                &mut continuation,
                &mut rows,
                cursor,
                cursor + 1,
                &[],
            );
            normal(&mut control, cursor, cursor + 1, &[]);
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
            cursor = deadline;
            assert!(rig.schedule.fault.is_none());
            assert!(rig.cpu.cycles() <= 23);
        }
        assert_eq!(rows.len(), 1);
        match end {
            TraceObservationEnd::Fault => {
                traced(
                    &mut rig,
                    &mut continuation,
                    &mut rows,
                    cursor,
                    cursor + 300,
                    &[],
                );
                normal(&mut control, cursor, cursor + 300, &[]);
                assert!(matches!(
                    rig.schedule.fault,
                    Some(SoundFault::UnsupportedAccess {
                        address: 0x7f00,
                        ..
                    })
                ));
            }
            TraceObservationEnd::Reset => {
                rig.schedule.reset_cpu(
                    &mut rig.cpu,
                    &mut rig.memory,
                    &mut continuation.attach(&mut rows),
                );
                control.schedule.reset_cpu(
                    &mut control.cpu,
                    &mut control.memory,
                    &mut crate::sound_trace::NoSoundCapture,
                );
            }
            TraceObservationEnd::Disabled => continuation.disable(&mut rows),
            _ => unreachable!(),
        }
        equal(&rig, &control);
        let row = rows.iter().last().unwrap();
        assert_eq!(row.instruction_bytes(), [0x22, 0xff, 0x7e]);
        assert_eq!(row.observation.end, end);
        assert_eq!(row.writes().len(), 1);
        assert_eq!(
            (
                row.writes()[0].address,
                row.writes()[0].new_value,
                row.writes()[0].old_value
            ),
            (0x7eff, 0x34, None)
        );
        assert_eq!(row.cycle + row.boundary.unwrap().cpu_clocks, 23);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_trace_refresh_replaces_staged_attempt_after_real_status_sampling() {
    let mut rig = Rig::new(&[0x3a, 0, 0x40, 0x76], false);
    rig.audio.write_opn2(0, 0, 0x2a);
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
        if pending.has_read_replay() {
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
    let pending = rig.schedule.pending.as_ref().unwrap().clone();
    assert_eq!(pending.cpu.regs().a, 0);
    assert!(cursor + 1 < pending.deadline);
    rig.audio.write_opn2(cursor + 1, 1, 0x90);
    control.audio.write_opn2(cursor + 1, 1, 0x90);
    traced(
        &mut rig,
        &mut continuation,
        &mut rows,
        cursor + 1,
        pending.deadline,
        &[],
    );
    normal(&mut control, cursor + 1, pending.deadline, &[]);
    equal(&rig, &control);
    assert_eq!(rig.cpu.regs().a, 0x80);
    let row = rows.iter().next().unwrap();
    assert_eq!(row.instruction_bytes(), [0x3a, 0, 0x40]);
    assert!(row.writes().is_empty());
    assert!(
        row.register_deltas()
            .iter()
            .any(|d| d.register == 0 && d.value == 0x80)
    );
    assert_eq!(
        row.cycle + row.boundary.unwrap().cpu_clocks,
        rig.cpu.cycles()
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_trace_late_main_fault_retains_real_commits_and_early_clock_fault_is_pure() {
    use crate::hardware::bus::scheduled::ScheduledBus;
    use zeff_m68k::timing::{ClockedBus, CpuClocks};
    let mut actual = super::callbacks::Machine::new(true);
    let mut expected = super::callbacks::Machine::new(true);
    let mut continuation = SoundTraceContinuation::default();
    let mut rows = output();
    for _ in 0..8 {
        let clock = actual.bus.cpu_clock;
        let a = ScheduledBus {
            bus: &mut actual.bus,
            cpu: &mut actual.cpu,
            schedule: &mut actual.schedule,
            capture: &mut continuation.attach(&mut rows),
        }
        .read_cycle(super::callbacks::access(8), clock);
        assert_eq!(a, expected.read(8, None));
    }
    let retained = rows.iter().copied().collect::<Vec<_>>();
    let pending = actual.schedule.clone();
    let before = actual.bus.now;
    let a = ScheduledBus {
        bus: &mut actual.bus,
        cpu: &mut actual.cpu,
        schedule: &mut actual.schedule,
        capture: &mut continuation.attach(&mut rows),
    }
    .read_cycle(super::callbacks::access(8), CpuClocks(0));
    assert_eq!(a, expected.read(8, Some(CpuClocks(0))));
    assert!(a.result.is_err());
    assert_eq!(actual.bus.now, before);
    assert_eq!(actual.schedule, pending);
    assert_eq!(rows.iter().copied().collect::<Vec<_>>(), retained);
    let clock = actual.bus.cpu_clock;
    let a = ScheduledBus {
        bus: &mut actual.bus,
        cpu: &mut actual.cpu,
        schedule: &mut actual.schedule,
        capture: &mut continuation.attach(&mut rows),
    }
    .read_cycle(super::callbacks::access(0x800000), clock);
    assert_eq!(a, expected.read(0x800000, None));
    assert!(a.result.is_err());
    assert!(actual.bus.now > before);
    assert!(rows.len() > retained.len());
    assert_eq!(
        rows.iter()
            .take(retained.len())
            .copied()
            .collect::<Vec<_>>(),
        retained
    );
    actual.schedule.finish_epoch_field(
        &mut actual.cpu,
        &actual.bus.sound,
        &mut continuation.attach(&mut rows),
    );
    expected.schedule.finish_epoch_field(
        &mut expected.cpu,
        &expected.bus.sound,
        &mut crate::sound_trace::NoSoundCapture,
    );
    assert_eq!(actual.bus, expected.bus);
    assert_eq!(actual.cpu, expected.cpu);
    assert_eq!(actual.schedule, expected.schedule);
}
