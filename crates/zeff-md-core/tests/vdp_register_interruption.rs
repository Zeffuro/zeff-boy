#[path = "vdp_register_interruption/cases.rs"]
pub mod cases;

#[test]
fn register_writes_abandon_incomplete_fetches_at_scheduled_boundaries() {
    cases::register_writes_abandon_incomplete_fetches_at_scheduled_boundaries();
}

#[test]
fn parser_priority_and_rejected_registers_preserve_transactions() {
    cases::parser_priority_and_rejected_registers_preserve_transactions();
}

#[test]
fn cpu_register_writes_and_restored_continuations_agree() {
    cases::cpu_register_writes_and_restored_continuations_agree();
}

#[test]
fn refresh_gap_and_address_wrap_keep_only_completed_progress() {
    cases::refresh_gap_and_address_wrap_keep_only_completed_progress();
}
