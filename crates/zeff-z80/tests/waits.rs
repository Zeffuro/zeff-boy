#[path = "waits/cases.rs"]
mod cases;

#[test]
fn checked_segment_wait_counts() {
    cases::checked_segment_wait_counts();
}

#[test]
fn push_waits_preserve_owned_phases() {
    cases::push_waits_preserve_owned_phases();
}

#[test]
fn accounting_preserves_ei_interrupt_halt_and_inspection() {
    cases::accounting_preserves_ei_interrupt_halt_and_inspection();
}

#[test]
fn waits_keep_continuation_clone_reset_and_default_apis() {
    cases::waits_keep_continuation_clone_reset_and_default_apis();
}
