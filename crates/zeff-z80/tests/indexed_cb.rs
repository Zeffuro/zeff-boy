#[path = "indexed_cb/cases.rs"]
mod cases;

#[test]
fn all_indexed_cb_forms_match_literal_effects_and_steppers() {
    cases::all_indexed_cb_forms_match_literal_effects_and_steppers();
}

#[test]
fn indexed_cb_displacements_and_bit_address_flags() {
    cases::indexed_cb_displacements_and_bit_address_flags();
}

#[test]
fn indexed_cb_live_fetches_and_latched_write() {
    cases::indexed_cb_live_fetches_and_latched_write();
}

#[test]
fn indexed_cb_pending_phases_clone_suspend_and_reset() {
    cases::indexed_cb_pending_phases_clone_suspend_and_reset();
}

#[test]
fn indexed_cb_prefixes_and_interrupt_boundaries() {
    cases::indexed_cb_prefixes_and_interrupt_boundaries();
}
