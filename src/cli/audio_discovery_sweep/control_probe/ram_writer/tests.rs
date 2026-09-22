use super::*;
use zeff_emu_common::debug::{TraceExecMode, TraceWrite};

fn state() -> State {
    State {
        pc: 0x8000,
        a: 1,
        x: 0,
        y: 0,
        sp: 0xff,
        p: 0,
        cycle: 7,
    }
}

fn step(tracker: &mut Tracker, before: State, after: State, bytes: &[u8], writes: &[(u16, u8)]) {
    let mut record = InstructionTraceRecord::new(
        TraceExecMode::Mos6502,
        u32::from(before.pc),
        Some(u64::from(before.pc - 0x8000)),
        0,
        before.cycle,
        bytes,
    );
    for &(address, value) in writes {
        record.push_write(TraceWrite {
            address: u32::from(address),
            old_value: 0,
            new_value: u32::from(value),
            width: TraceWriteWidth::Byte,
            kind: TraceWriteKind::Memory,
        });
    }
    tracker.step(before, after, Some(&record));
}

#[test]
fn masks_are_subsets_and_large_domains_are_withheld() {
    for (mask, expected) in [(5, json!([0, 1, 4, 5])), (255, Value::Null)] {
        let mut tracker = Tracker::new();
        let state = state();
        step(&mut tracker, state, state, &[0x29, mask], &[]);
        step(&mut tracker, state, state, &[0x85, 0xad], &[(0xad, 1)]);
        let writer = tracker.snapshot(0x8ad, 1).unwrap();
        assert_eq!(writer["value_constraint"]["values"], expected);
        assert_eq!(writer["source_offset"], 16);
        assert!(tracker.snapshot(0xad, 0).is_none());
    }
}

#[test]
fn aliases_equal_writes_and_unsupported_writers_replace_origin() {
    let mut tracker = Tracker::new();
    let mut state = state();
    step(&mut tracker, state, state, &[0x29, 1], &[]);
    step(&mut tracker, state, state, &[0x85, 0xad], &[(0xad, 1)]);
    let old = tracker.snapshot(0x8ad, 1).unwrap();
    state.pc += 2;
    step(&mut tracker, state, state, &[0x8d, 0xad, 8], &[(0x8ad, 1)]);
    assert_eq!(tracker.snapshot(0xad, 1).unwrap()["pc"], 0x8002);
    assert_eq!(old["pc"], 0x8000);
    for bytes in [&[0x9d, 0xad, 0][..], &[0xe6, 0xad][..], &[0x48][..]] {
        step(&mut tracker, state, state, &[0x85, 0xad], &[(0xad, 1)]);
        step(&mut tracker, state, state, bytes, &[(0x8ad, 1)]);
        assert!(tracker.snapshot(0xad, 1).is_none());
    }
}

#[test]
fn same_value_arithmetic_kills_constraint_and_budget_includes_store() {
    for (nops, unsupported, expected) in [(62, false, true), (63, false, false), (0, true, false)] {
        let mut tracker = Tracker::new();
        let state = state();
        step(&mut tracker, state, state, &[0x29, 1], &[]);
        for _ in 0..nops {
            step(&mut tracker, state, state, &[0xea], &[]);
        }
        if unsupported {
            step(&mut tracker, state, state, &[0x49, 0], &[]);
        }
        step(&mut tracker, state, state, &[0x85, 0xad], &[(0xad, 1)]);
        assert_eq!(
            !tracker.snapshot(0xad, 1).unwrap()["value_constraint"].is_null(),
            expected
        );
    }
}

#[test]
fn interrupt_keeps_ram_but_discards_register_constraint_and_missing_trace_clears_both() {
    let mut tracker = Tracker::new();
    let state = state();
    step(&mut tracker, state, state, &[0x29, 1], &[]);
    step(&mut tracker, state, state, &[0x85, 0xad], &[(0xad, 1)]);
    let interrupt = InstructionTraceRecord {
        event: Some(DebugEvent::Interrupt),
        ..Default::default()
    };
    tracker.step(state, state, Some(&interrupt));
    assert!(tracker.snapshot(0xad, 1).is_some());
    step(&mut tracker, state, state, &[0x85, 0xae], &[(0xae, 1)]);
    assert!(tracker.snapshot(0xae, 1).unwrap()["value_constraint"].is_null());
    tracker.step(state, state, None);
    assert!(tracker.snapshot(0xad, 1).is_none());
}

#[test]
fn write_overflow_cannot_keep_stale_ram() {
    let mut tracker = Tracker::new();
    let state = state();
    step(&mut tracker, state, state, &[0x85, 0xad], &[(0xad, 1)]);
    let record = InstructionTraceRecord {
        write_overflow: 1,
        ..Default::default()
    };
    tracker.step(state, state, Some(&record));
    assert!(tracker.snapshot(0xad, 1).is_none());
}
