#[path = "stack/cases.rs"]
mod cases;

#[test]
fn all_stack_forms_match_literal_effects_and_steppers() {
    cases::all_stack_forms_match_literal_effects_and_steppers();
}

#[test]
fn stack_live_reads_and_latched_writes() {
    cases::stack_live_reads_and_latched_writes();
}

#[test]
fn stack_pending_phases_clone_suspend_and_reset() {
    cases::stack_pending_phases_clone_suspend_and_reset();
}

#[test]
fn stack_prefixes_and_interrupt_boundaries() {
    cases::stack_prefixes_and_interrupt_boundaries();
}

#[test]
fn stack_pc_wrap_and_instruction_aliases() {
    cases::stack_pc_wrap_and_instruction_aliases();
}
