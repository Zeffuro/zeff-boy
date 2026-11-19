#[path = "ed_transfer/cases.rs"]
mod cases;

#[test]
fn all_ed_transfer_forms_match_literal_effects_and_steppers() {
    cases::all_ed_transfer_forms_match_literal_effects_and_steppers();
}

#[test]
fn ed_transfer_live_reads_latches_and_wraps() {
    cases::ed_transfer_live_reads_latches_and_wraps();
}

#[test]
fn ed_transfer_pending_clone_suspend_reset() {
    cases::ed_transfer_pending_clone_suspend_reset();
}

#[test]
fn ed_transfer_interrupt_boundaries() {
    cases::ed_transfer_interrupt_boundaries();
}

#[test]
fn ed_terminal_fallback_fetch_repeat_and_io() {
    cases::ed_terminal_fallback_fetch_repeat_and_io();
}
