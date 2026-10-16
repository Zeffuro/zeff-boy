#[path = "ed_io/cases.rs"]
mod cases;

#[test]
fn all_ed_io_forms_match_literal_effects_and_steppers() {
    cases::all_ed_io_forms_match_literal_effects_and_steppers();
}

#[test]
fn ed_io_live_terminal_and_port_reads() {
    cases::ed_io_live_terminal_and_port_reads();
}

#[test]
fn ed_io_pending_clone_suspend_reset() {
    cases::ed_io_pending_clone_suspend_reset();
}

#[test]
fn ed_io_interrupt_boundaries() {
    cases::ed_io_interrupt_boundaries();
}

#[test]
fn ed_io_repeated_prefixes_and_callback_offsets() {
    cases::ed_io_repeated_prefixes_and_callback_offsets();
}
