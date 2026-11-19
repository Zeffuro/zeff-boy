#[path = "ed_internal/cases.rs"]
mod cases;

#[test]
fn all_ed_internal_forms_match_literal_effects_and_steppers() {
    cases::all_ed_internal_forms_match_literal_effects_and_steppers();
}

#[test]
fn ed_internal_live_terminal_and_nibble_latches() {
    cases::ed_internal_live_terminal_and_nibble_latches();
}

#[test]
fn ed_internal_pending_clone_suspend_reset() {
    cases::ed_internal_pending_clone_suspend_reset();
}

#[test]
fn ed_internal_interrupt_boundaries() {
    cases::ed_internal_interrupt_boundaries();
}

#[test]
fn ed_internal_special_register_refresh_and_iff2() {
    cases::ed_internal_special_register_refresh_and_iff2();
}
