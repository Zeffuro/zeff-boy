use super::*;

#[test]
fn add_and_remove_breakpoint() {
    let mut dc = DebugController::new();
    dc.add_breakpoint(0x8000);
    assert!(dc.has_breakpoint(0x8000));
    dc.remove_breakpoint(0x8000);
    assert!(!dc.has_breakpoint(0x8000));
}

#[test]
fn toggle_breakpoint_adds_and_removes() {
    let mut dc = DebugController::new();
    dc.toggle_breakpoint(0xC000);
    assert!(dc.has_breakpoint(0xC000));
    dc.toggle_breakpoint(0xC000);
    assert!(!dc.has_breakpoint(0xC000));
}

#[test]
fn should_break_at_breakpoint() {
    let mut dc = DebugController::new();
    dc.add_breakpoint(0x8000);
    assert!(dc.should_break(0x8000));
    assert_eq!(dc.hit_breakpoint, Some(0x8000));
}

#[test]
fn should_not_break_without_breakpoints() {
    let mut dc = DebugController::new();
    assert!(!dc.should_break(0x8000));
}

#[test]
fn break_on_next_fires_once() {
    let mut dc = DebugController::new();
    dc.break_on_next = true;
    assert!(dc.should_break(0x8000));
    assert!(!dc.should_break(0x8001));
}

#[test]
fn watchpoint_write_detects_change() {
    let mut dc = DebugController::new();
    dc.add_watchpoint(0x0000, WatchType::Write);
    dc.check_watch_write(0x0000, 0x00, 0x42);
    let hit = dc.hit_watchpoint.unwrap();
    assert_eq!(hit.address, 0x0000);
    assert_eq!(hit.old_value, 0x00);
    assert_eq!(hit.new_value, 0x42);
}

#[test]
fn watchpoint_write_ignores_same_value() {
    let mut dc = DebugController::new();
    dc.add_watchpoint(0x0000, WatchType::Write);
    dc.check_watch_write(0x0000, 0x42, 0x42);
    assert!(dc.hit_watchpoint.is_none());
}

#[test]
fn watchpoint_read_fires() {
    let mut dc = DebugController::new();
    dc.add_watchpoint(0x0000, WatchType::Read);
    dc.check_watch_read(0x0000, 0x55);
    let hit = dc.hit_watchpoint.unwrap();
    assert_eq!(hit.address, 0x0000);
}

#[test]
fn duplicate_watchpoint_not_added() {
    let mut dc = DebugController::new();
    dc.add_watchpoint(0x0000, WatchType::Write);
    dc.add_watchpoint(0x0000, WatchType::Write);
    assert_eq!(dc.watchpoints.len(), 1);
}

#[test]
fn clear_hits_resets_state() {
    let mut dc = DebugController::new();
    dc.add_breakpoint(0x8000);
    dc.should_break(0x8000);
    dc.clear_hits();
    assert!(dc.hit_breakpoint.is_none());
    assert!(dc.hit_watchpoint.is_none());
}

