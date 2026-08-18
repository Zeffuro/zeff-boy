use super::*;

#[test]
fn add_remove_breakpoint() {
    let mut dc = DebugController::new();
    dc.add_breakpoint(0x1234);
    assert!(dc.has_breakpoint(0x1234));
    dc.remove_breakpoint(0x1234);
    assert!(!dc.has_breakpoint(0x1234));
}

#[test]
fn toggle_breakpoint() {
    let mut dc = DebugController::new();
    assert!(!dc.has_breakpoint(0x100));
    dc.toggle_breakpoint(0x100);
    assert!(dc.has_breakpoint(0x100));
    dc.toggle_breakpoint(0x100);
    assert!(!dc.has_breakpoint(0x100));
}

#[test]
fn one_shot_breakpoint_removes_itself() {
    let mut dc = DebugController::new();
    dc.add_one_shot_breakpoint(0x1234);
    assert_eq!(
        dc.iter_one_shot_breakpoints().collect::<Vec<_>>(),
        vec![0x1234]
    );
    assert!(dc.should_break(0x1234));
    assert!(!dc.has_breakpoint(0x1234));
    assert!(!dc.should_break(0x1234));

    let mut dc = AddressDebugController::new();
    dc.add_one_shot_breakpoint(0x0200_0000);
    assert!(dc.should_break(0x0200_0000));
    assert!(dc.iter_breakpoints().next().is_none());
    assert!(!dc.should_break(0x0200_0000));
}

#[test]
fn breakpoint_waits_for_hit_count() {
    let mut dc = DebugController::new();
    dc.add_breakpoint_after(0x1234, 3);
    assert!(!dc.should_break(0x1234));
    assert_eq!(dc.hit_breakpoint, None);
    assert!(!dc.should_break(0x1234));
    assert_eq!(dc.hit_breakpoint, None);
    assert!(dc.should_break(0x1234));
    assert_eq!(dc.iter_breakpoint_hit_conditions().next().unwrap().hits, 3);

    let mut dc = AddressDebugController::new();
    dc.add_breakpoint_after(0x0200_0000, 2);
    assert!(!dc.should_break(0x0200_0000));
    assert_eq!(dc.hit_breakpoint, None);
    assert!(dc.should_break(0x0200_0000));
    assert_eq!(
        dc.iter_breakpoint_hit_conditions()
            .next()
            .unwrap()
            .target_hits,
        2
    );
}

#[test]
fn event_breakpoints_are_persistent_and_clear_hits() {
    let mut dc = DebugController::new();
    assert!(!dc.any_active());
    dc.set_event_breakpoint(DebugEvent::Interrupt, true);
    assert!(dc.any_active());
    assert!(dc.check_event(DebugEvent::Interrupt));
    assert_eq!(dc.hit_event, Some(DebugEvent::Interrupt));
    dc.clear_hits();
    assert_eq!(dc.hit_event, None);
    assert!(dc.has_event_breakpoint(DebugEvent::Interrupt));

    let mut dc = AddressDebugController::new();
    dc.set_event_breakpoint(DebugEvent::Dma, true);
    assert!(dc.check_event(DebugEvent::Dma));
    assert_eq!(
        dc.iter_event_breakpoints().collect::<Vec<_>>(),
        [DebugEvent::Dma]
    );
    dc.set_event_breakpoint(DebugEvent::Dma, false);
    assert!(!dc.check_event(DebugEvent::Dma));
}

#[test]
fn should_break_on_breakpoint() {
    let mut dc = DebugController::new();
    dc.add_breakpoint(0x200);
    assert!(dc.should_break(0x200));
    assert_eq!(dc.hit_breakpoint, Some(0x200));
}

#[test]
fn break_on_next() {
    let mut dc = DebugController::new();
    dc.break_on_next = true;
    assert!(dc.should_break(0x0));
    assert!(!dc.break_on_next);
}

#[test]
fn watchpoint_write() {
    let mut dc = DebugController::new();
    dc.add_watchpoint(0x300, WatchType::Write);
    assert!(dc.has_watchpoints());
    dc.check_watch_write(0x300, 10, 20);
    assert!(dc.hit_watchpoint.is_some());
    let hit = dc
        .hit_watchpoint
        .expect("watchpoint should have been triggered");
    assert_eq!(hit.address, 0x300);
    assert_eq!(hit.old_value, 10);
    assert_eq!(hit.new_value, 20);
}

#[test]
fn watchpoint_no_trigger_on_same_value() {
    let mut dc = DebugController::new();
    dc.add_watchpoint(0x400, WatchType::Write);
    dc.check_watch_write(0x400, 5, 5);
    assert!(dc.hit_watchpoint.is_none());
}

#[test]
fn duplicate_watchpoint_not_added() {
    let mut dc = DebugController::new();
    dc.add_watchpoint(0x500, WatchType::Read);
    dc.add_watchpoint(0x500, WatchType::Read);
    assert_eq!(dc.watchpoints.len(), 1);
}

#[test]
fn ranged_watchpoint_matches_and_removes() {
    let mut dc = DebugController::new();
    dc.add_watchpoint_range(0xC000, 0xC00F, WatchType::ReadWrite);
    dc.check_watch_read(0xC008, 0x42);
    assert_eq!(dc.hit_watchpoint.unwrap().address, 0xC008);

    dc.clear_hits();
    dc.remove_watchpoint(0xC000, 0xC00F, WatchType::ReadWrite);
    dc.check_watch_write(0xC008, 0x42, 0x43);
    assert!(dc.hit_watchpoint.is_none());
    assert!(!dc.has_watchpoints());
}

#[test]
fn address_ranged_watchpoint_orders_bounds() {
    let mut dc = AddressDebugController::new();
    dc.add_watchpoint_range(0x0200_001F, 0x0200_0010, WatchType::Write);
    assert_eq!(dc.watchpoints[0].address, 0x0200_0010);
    assert_eq!(dc.watchpoints[0].end_address, 0x0200_001F);
    dc.check_watch_write(0x0200_0018, 0, 1);
    assert_eq!(dc.hit_watchpoint.unwrap().address, 0x0200_0018);
}

#[test]
fn opcode_log_push_and_recent() {
    let mut log = OpcodeLog::<(u16, u8)>::new();
    log.set_enabled(true);
    log.push((0x100, 0xAB));
    log.push((0x102, 0xCD));
    let recent = log.recent(10);
    assert_eq!(recent, vec![(0x102, 0xCD), (0x100, 0xAB)]);
}

#[test]
fn opcode_log_disabled_ignores_push() {
    let mut log = OpcodeLog::<(u16, u8)>::new();
    log.set_enabled(false);
    log.push((0x100, 0xAB));
    assert!(log.recent(10).is_empty());
}

#[test]
fn opcode_log_clear_resets() {
    let mut log = OpcodeLog::<(u16, u8, bool)>::new();
    log.set_enabled(true);
    log.push((0x100, 0xAB, false));
    log.push((0x102, 0xCB, true));
    log.clear();
    assert!(log.recent(10).is_empty());
}

#[test]
fn opcode_log_wraps_at_capacity() {
    let mut log = OpcodeLog::<(u16, u8)>::new();
    log.set_enabled(true);
    for i in 0..64u16 {
        log.push((i, i as u8));
    }
    let recent = log.recent(32);
    assert_eq!(recent.len(), 32);
    assert_eq!(recent[0], (63, 63));
    assert_eq!(recent[31], (32, 32));
}

#[test]
fn instruction_trace_is_inert_when_disabled() {
    let mut trace = InstructionTraceStore::new(0);
    assert_eq!(trace.capacity(), MIN_TRACE_CAPACITY);
    assert_eq!(trace.push(InstructionTraceRecord::default()), None);
    assert!(trace.is_empty());
    assert_eq!(trace.iter().count(), 0);
}

#[test]
fn instruction_trace_evicts_oldest_in_order() {
    let mut trace = InstructionTraceStore::new(MIN_TRACE_CAPACITY);
    trace.set_enabled(true);
    for pc in 0..=MIN_TRACE_CAPACITY as u32 {
        trace.push(InstructionTraceRecord::new(
            TraceExecMode::Sm83,
            pc,
            Some(u64::from(pc)),
            4,
            u64::from(pc),
            &[0],
        ));
    }

    let entries = trace.iter().collect::<Vec<_>>();
    assert_eq!(entries.len(), MIN_TRACE_CAPACITY);
    assert_eq!(entries[0].pc, 1);
    assert_eq!(entries[0].sequence, 1);
    assert_eq!(entries.last().unwrap().pc, MIN_TRACE_CAPACITY as u32);
    assert_eq!(entries.last().unwrap().sequence, MIN_TRACE_CAPACITY as u64);
}

#[test]
fn instruction_trace_clamps_and_resizes_capacity() {
    let mut trace = InstructionTraceStore::new(usize::MAX);
    assert_eq!(trace.capacity(), MAX_TRACE_CAPACITY);
    trace.set_capacity(0);
    assert_eq!(trace.capacity(), MIN_TRACE_CAPACITY);
    trace.set_capacity(MIN_TRACE_CAPACITY * 2);
    trace.set_enabled(true);
    for pc in 0..MIN_TRACE_CAPACITY as u32 + 4 {
        trace.push(InstructionTraceRecord::new(
            TraceExecMode::Z80,
            pc,
            None,
            0,
            0,
            &[],
        ));
    }
    trace.set_capacity(MIN_TRACE_CAPACITY);
    assert_eq!(trace.iter().next().unwrap().pc, 4);
    assert_eq!(
        trace.iter().last().unwrap().pc,
        MIN_TRACE_CAPACITY as u32 + 3
    );
}

#[test]
fn instruction_trace_entries_after_reports_retained_range() {
    let mut trace = InstructionTraceStore::new(MIN_TRACE_CAPACITY);
    trace.set_enabled(true);
    for pc in 0..MIN_TRACE_CAPACITY as u32 + 2 {
        trace.push(InstructionTraceRecord::new(
            TraceExecMode::V30,
            pc,
            None,
            0,
            0,
            &[],
        ));
    }

    assert_eq!(trace.oldest_sequence(), Some(2));
    assert_eq!(trace.newest_sequence(), Some(MIN_TRACE_CAPACITY as u64 + 1));
    assert_eq!(
        trace
            .entries_after(Some(0), 2)
            .iter()
            .map(|entry| entry.sequence)
            .collect::<Vec<_>>(),
        [2, 3]
    );
    assert_eq!(
        trace
            .entries_after(Some(MIN_TRACE_CAPACITY as u64), 4)
            .iter()
            .map(|entry| entry.sequence)
            .collect::<Vec<_>>(),
        [MIN_TRACE_CAPACITY as u64 + 1]
    );
    assert!(trace.entries_after(trace.newest_sequence(), 4).is_empty());
    assert!(trace.entries_after(None, 0).is_empty());
}

#[test]
fn instruction_trace_clear_keeps_capture_enabled_and_sequence() {
    let mut trace = InstructionTraceStore::new(MIN_TRACE_CAPACITY);
    trace.set_enabled(true);
    assert_eq!(trace.push(InstructionTraceRecord::default()), Some(0));
    trace.clear();
    assert!(trace.is_empty());
    assert!(trace.is_enabled());
    assert_eq!(trace.push(InstructionTraceRecord::default()), Some(1));
}

#[test]
fn instruction_trace_preserves_delta_and_write_order_with_overflow() {
    let mut entry = InstructionTraceRecord::new(
        TraceExecMode::Arm,
        0x0800_0000,
        Some(0),
        12,
        34,
        &[0x01, 0x02, 0x03],
    );
    for register in 0..MAX_TRACE_REGISTER_DELTAS as u8 + 2 {
        entry.push_register_delta(RegisterDelta {
            register,
            value: u32::from(register),
        });
    }
    for address in 0..MAX_TRACE_WRITES as u32 + 2 {
        entry.push_write(TraceWrite {
            address,
            old_value: address,
            new_value: address + 1,
            width: TraceWriteWidth::Word,
            kind: TraceWriteKind::Io,
        });
    }

    assert_eq!(entry.register_deltas().len(), MAX_TRACE_REGISTER_DELTAS);
    assert_eq!(entry.register_deltas()[0].register, 0);
    assert_eq!(
        entry.register_deltas().last().unwrap().register,
        MAX_TRACE_REGISTER_DELTAS as u8 - 1
    );
    assert_eq!(entry.register_delta_overflow, 2);
    assert_eq!(entry.writes().len(), MAX_TRACE_WRITES);
    assert_eq!(entry.writes()[0].address, 0);
    assert_eq!(
        entry.writes().last().unwrap().address,
        MAX_TRACE_WRITES as u32 - 1
    );
    assert_eq!(entry.write_overflow, 2);
    assert_eq!(entry.instruction_len, 3);
}

#[test]
fn address_debug_controller_handles_wide_breakpoints() {
    let mut dc = AddressDebugController::new();
    dc.add_breakpoint(0x0800_1234);
    assert!(dc.should_break(0x0800_1234));
    assert_eq!(dc.hit_breakpoint, Some(0x0800_1234));
    dc.remove_breakpoint(0x0800_1234);
    dc.clear_hits();
    assert!(!dc.should_break(0x0800_1234));
}

#[test]
fn address_debug_controller_watch_write() {
    let mut dc = AddressDebugController::new();
    dc.add_watchpoint(0x0200_0000, WatchType::Write);
    dc.check_watch_write(0x0200_0000, 1, 2);
    let hit = dc.hit_watchpoint.expect("watch write should hit");
    assert_eq!(hit.address, 0x0200_0000);
    assert_eq!(hit.old_value, 1);
    assert_eq!(hit.new_value, 2);
}
