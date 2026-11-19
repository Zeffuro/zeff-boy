#[path = "vdp_read_inhibition/cases.rs"]
pub mod cases;

#[test]
fn inhibited_commands_latch_without_prefetch_and_reject_data_accesses() {
    cases::inhibited_commands_latch_without_prefetch_and_reject_data_accesses();
}

#[test]
fn register_looking_second_words_keep_command_priority_with_dma_disabled() {
    cases::register_looking_second_words_keep_command_priority_with_dma_disabled();
}

#[test]
fn active_unsupported_reads_and_dma_dispatch_keep_their_boundaries() {
    cases::active_unsupported_reads_and_dma_dispatch_keep_their_boundaries();
}
