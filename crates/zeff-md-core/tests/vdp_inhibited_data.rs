#[path = "vdp_inhibited_data/cases.rs"]
pub mod cases;

#[test]
fn inhibited_target_uses_one_fifo_slot_without_memory_writes() {
    cases::inhibited_target_uses_one_fifo_slot_without_memory_writes();
}

#[test]
fn inhibited_port_write_cancels_parser_and_keeps_cached_history() {
    cases::inhibited_port_write_cancels_parser_and_keeps_cached_history();
}

#[test]
fn inhibited_entries_preserve_interleaved_fifo_order() {
    cases::inhibited_entries_preserve_interleaved_fifo_order();
}

#[test]
fn full_inhibited_fifo_rejects_then_retries_at_its_first_slot() {
    cases::full_inhibited_fifo_rejects_then_retries_at_its_first_slot();
}

#[test]
fn adjacent_commands_and_dma_keep_their_data_boundaries() {
    cases::adjacent_commands_and_dma_keep_their_data_boundaries();
}
