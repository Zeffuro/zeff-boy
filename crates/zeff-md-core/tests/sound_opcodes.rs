#[allow(dead_code)]
#[path = "multi_transfer/cases.rs"]
pub(crate) mod multi_transfer_cases;
#[allow(dead_code)]
#[path = "sound/fixture.rs"]
pub(crate) mod sound_fixture;

mod ed_block {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn block_read_grants_are_live() {
        cases::block_read_grants_are_live();
    }
    #[test]
    fn block_write_and_reset_keep_prior_byte() {
        cases::block_write_and_reset_keep_prior_byte();
    }
    #[test]
    fn block_failed_write_keeps_read_state() {
        cases::block_failed_write_keeps_read_state();
    }
    #[test]
    fn block_io_grants_hold_input_and_output() {
        cases::block_io_grants_hold_input_and_output();
    }
    #[test]
    fn block_repeat_phase_keeps_base_effects() {
        cases::block_repeat_phase_keeps_base_effects();
    }
}

mod ed_internal {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn nibble_read_is_live_after_grant() {
        cases::nibble_read_is_live_after_grant();
    }

    #[test]
    fn nibble_grant_keeps_read_result() {
        cases::nibble_grant_keeps_read_result();
    }

    #[test]
    fn nibble_reset_and_fault_keep_uncommitted_registers() {
        cases::nibble_reset_and_fault_keep_uncommitted_registers();
    }

    #[test]
    fn arithmetic_and_special_internal_grants() {
        cases::arithmetic_and_special_internal_grants();
    }
}

mod ed_io {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn ed_io_grant_keeps_terminal_and_registers() {
        cases::ed_io_grant_keeps_terminal_and_registers();
    }

    #[test]
    fn ed_io_reset_cancels_pending_input() {
        cases::ed_io_reset_cancels_pending_input();
    }

    #[test]
    fn ed_io_disconnected_ports_preserve_machine_state() {
        cases::ed_io_disconnected_ports_preserve_machine_state();
    }
}

mod ed_transfer {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn ed_word_address_high_is_live_after_grant() {
        cases::ed_word_address_high_is_live_after_grant();
    }

    #[test]
    fn ed_word_keeps_low_and_reads_high_live() {
        cases::ed_word_keeps_low_and_reads_high_live();
    }

    #[test]
    fn ed_word_write_grant_and_reset_keep_first_byte() {
        cases::ed_word_write_grant_and_reset_keep_first_byte();
    }

    #[test]
    fn ed_returns_keep_low_and_read_high_live() {
        cases::ed_returns_keep_low_and_read_high_live();
    }

    #[test]
    fn ed_second_access_faults_keep_progress() {
        cases::ed_second_access_faults_keep_progress();
    }
}

mod im0 {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn jump_low_is_latched_and_high_is_live() {
        cases::jump_low_is_latched_and_high_is_live();
    }

    #[test]
    fn jump_reset_discards_pending_operand() {
        cases::jump_reset_discards_pending_operand();
    }

    #[test]
    fn conditional_jump_reads_high_before_deciding() {
        cases::conditional_jump_reads_high_before_deciding();
    }

    #[test]
    fn pending_jump_defers_irq_and_pushes_target() {
        cases::pending_jump_defers_irq_and_pushes_target();
    }

    #[test]
    fn immediate_loads_preserve_index_and_controller_rules() {
        cases::immediate_loads_preserve_index_and_controller_rules();
    }
}

mod indexed {
    #[path = "cases.rs"]
    mod cases;
    #[test]
    fn indexed_register_memory_and_cb() {
        cases::indexed_register_memory_and_cb();
    }
    #[test]
    fn prefix_grant_observes_modified_terminal() {
        cases::prefix_grant_observes_modified_terminal();
    }
    #[test]
    fn pending_prefix_clone_and_reset() {
        cases::pending_prefix_clone_and_reset();
    }
    #[test]
    fn terminal_fault_preserves_prefixes() {
        cases::terminal_fault_preserves_prefixes();
    }
    #[test]
    fn retained_prefix_input_and_ignored_ed() {
        cases::retained_prefix_input_and_ignored_ed();
    }
}

mod indexed_cb {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn indexed_cb_terminal_is_live_after_grant() {
        cases::indexed_cb_terminal_is_live_after_grant();
    }

    #[test]
    fn indexed_cb_read_result_and_register_copy_are_latched() {
        cases::indexed_cb_read_result_and_register_copy_are_latched();
    }

    #[test]
    fn indexed_bit_reads_after_grant_without_register_copy() {
        cases::indexed_bit_reads_after_grant_without_register_copy();
    }

    #[test]
    fn indexed_cb_reset_and_fault_preserve_progress() {
        cases::indexed_cb_reset_and_fault_preserve_progress();
    }
}

mod indexed_memory {
    #[path = "cases.rs"]
    mod cases;
    #[test]
    fn indexed_address_and_read_after_grant() {
        cases::indexed_address_and_read_after_grant();
    }
    #[test]
    fn indexed_rmw_keeps_read_result() {
        cases::indexed_rmw_keeps_read_result();
    }
    #[test]
    fn indexed_immediate_write_is_latched() {
        cases::indexed_immediate_write_is_latched();
    }
    #[test]
    fn indexed_reset_and_write_fault() {
        cases::indexed_reset_and_write_fault();
    }
}

mod interrupt_service {
    #[path = "cases.rs"]
    pub(crate) mod cases;

    #[test]
    fn manual_irq_modes_precede_prefixed_opcodes() {
        cases::manual_irq_modes_precede_prefixed_opcodes();
    }
    #[test]
    fn concurrent_irq_preserves_live_line_and_partial_stack() {
        cases::concurrent_irq_preserves_live_line_and_partial_stack();
    }
    #[test]
    fn failed_irq_write_retains_prior_progress_and_reset_line() {
        cases::failed_irq_write_retains_prior_progress_and_reset_line();
    }
    #[test]
    fn im2_vector_high_is_live_after_grant() {
        cases::im2_vector_high_is_live_after_grant();
    }
    #[test]
    fn reset_and_halt_interrupts_keep_controller_state() {
        cases::reset_and_halt_interrupts_keep_controller_state();
    }
}
