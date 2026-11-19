#[path = "ed_block/cases.rs"]
mod cases;

#[test]
fn all_ed_block_forms_match_literal_effects_and_steppers() {
    cases::all_ed_block_forms_match_literal_effects_and_steppers();
}

#[test]
fn ed_block_live_operands_and_latches() {
    cases::ed_block_live_operands_and_latches();
}

#[test]
fn ed_block_pending_clone_suspend_reset() {
    cases::ed_block_pending_clone_suspend_reset();
}

#[test]
fn ed_block_interrupt_boundaries() {
    cases::ed_block_interrupt_boundaries();
}

#[test]
fn ed_block_repeat_flags_and_callback_offsets() {
    cases::ed_block_repeat_flags_and_callback_offsets();
}
