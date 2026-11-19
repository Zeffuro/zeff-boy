#[path = "vdp_prefetch_continuation/cases.rs"]
mod cases;

#[test]
fn odd_increment_prefetch_restores_and_cancels_through_the_main_bus() {
    cases::main_bus_restore_and_cancellation();
}

#[test]
fn odd_refill_uses_aligned_word_and_restores_running_cpu() {
    cases::main_bus_odd_refill_continuation();
}
