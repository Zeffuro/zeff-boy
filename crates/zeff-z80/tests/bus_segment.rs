#[path = "bus_segment/cases.rs"]
mod cases;

#[test]
fn complete_single_transfer_family_matches_legacy() {
    cases::complete_single_transfer_family_matches_legacy();
}

#[test]
fn latched_opcode_and_live_transfer_are_distinct() {
    cases::latched_opcode_and_live_transfer_are_distinct();
}

#[test]
fn interrupts_suspend_clone_and_reset_preserve_boundaries() {
    cases::interrupts_suspend_clone_and_reset_preserve_boundaries();
}

#[test]
fn wrapping_fetch_and_long_prefixes_resume_once() {
    cases::wrapping_fetch_and_long_prefixes_resume_once();
}

#[test]
fn aggregate_fallback_and_old_bounded_api_stay_exact() {
    cases::aggregate_fallback_and_old_bounded_api_stay_exact();
}
