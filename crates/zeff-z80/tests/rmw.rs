#[path = "rmw/cases.rs"]
mod cases;
#[test]
fn all_cb_and_memory_inc_dec_match_legacy() {
    cases::all_cb_and_memory_inc_dec_match_legacy();
}
#[test]
fn read_value_and_flags_stay_latched() {
    cases::read_value_and_flags_stay_latched();
}
#[test]
fn cb_terminal_fetch_is_live() {
    cases::cb_terminal_fetch_is_live();
}
#[test]
fn pending_write_defers_interrupts_and_reset() {
    cases::pending_write_defers_interrupts_and_reset();
}
