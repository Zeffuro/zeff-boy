#[path = "indexed_memory/cases.rs"]
mod cases;

#[test]
fn all_indexed_memory_forms_match_legacy() {
    cases::all_indexed_memory_forms_match_legacy();
}

#[test]
fn indexed_effects_match_literal_results() {
    cases::indexed_effects_match_literal_results();
}

#[test]
fn indexed_operands_are_live_until_their_phase() {
    cases::indexed_operands_are_live_until_their_phase();
}

#[test]
fn indexed_pending_phases_clone_suspend_and_reset() {
    cases::indexed_pending_phases_clone_suspend_and_reset();
}

#[test]
fn indexed_prefixes_and_interrupts_preserve_boundaries() {
    cases::indexed_prefixes_and_interrupts_preserve_boundaries();
}
