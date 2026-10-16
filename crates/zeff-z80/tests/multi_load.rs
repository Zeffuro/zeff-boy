#[path = "multi_load/cases.rs"]
mod cases;

#[test]
fn all_load_encodings_match_legacy_and_literal_phases() {
    cases::all_load_encodings_match_legacy_and_literal_phases();
}

#[test]
fn operands_and_data_latch_at_individual_transfers() {
    cases::operands_and_data_latch_at_individual_transfers();
}

#[test]
fn completed_writes_survive_copy_discard_and_reset() {
    cases::completed_writes_survive_copy_discard_and_reset();
}

#[test]
fn interrupts_and_suspension_wait_for_load_completion() {
    cases::interrupts_and_suspension_wait_for_load_completion();
}

#[test]
fn wrapping_and_repeated_prefixes_preserve_load_progress() {
    cases::wrapping_and_repeated_prefixes_preserve_load_progress();
}
