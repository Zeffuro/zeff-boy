#[path = "interrupt_service/cases.rs"]
mod cases;

#[test]
fn all_interrupt_services_match_literal_effects_and_steppers() {
    cases::all_interrupt_services_match_literal_effects_and_steppers();
}

#[test]
fn interrupt_vector_reads_and_stack_writes_are_live() {
    cases::interrupt_vector_reads_and_stack_writes_are_live();
}

#[test]
fn interrupt_pending_clone_suspend_reset() {
    cases::interrupt_pending_clone_suspend_reset();
}

#[test]
fn interrupt_priority_iff_and_ei_boundaries() {
    cases::interrupt_priority_iff_and_ei_boundaries();
}

#[test]
fn interrupt_reset_halt_and_im0_guards() {
    cases::interrupt_reset_halt_and_im0_guards();
}
