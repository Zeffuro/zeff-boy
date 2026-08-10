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
