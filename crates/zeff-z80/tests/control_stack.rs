#[path = "control_stack/cases.rs"]
mod cases;

#[test]
fn all_control_stack_forms_match_literal_effects_and_steppers() {
    cases::all_control_stack_forms_match_literal_effects_and_steppers();
}

#[test]
fn control_stack_live_reads_and_latched_targets() {
    cases::control_stack_live_reads_and_latched_targets();
}

#[test]
fn control_stack_pending_phases_clone_suspend_and_reset() {
    cases::control_stack_pending_phases_clone_suspend_and_reset();
}

#[test]
fn control_stack_prefixes_and_interrupt_boundaries() {
    cases::control_stack_prefixes_and_interrupt_boundaries();
}

#[test]
fn control_stack_pc_wrap_and_instruction_aliases() {
    cases::control_stack_pc_wrap_and_instruction_aliases();
}
