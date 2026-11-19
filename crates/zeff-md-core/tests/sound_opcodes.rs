macro_rules! native_cases {
    ($($name:ident),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                cases::$name();
            }
        )+
    };
}

#[allow(dead_code)]
#[path = "multi_transfer/cases.rs"]
pub(crate) mod multi_transfer_cases;
#[allow(dead_code)]
#[path = "sound/fixture.rs"]
pub(crate) mod sound_fixture;

mod ed_block {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        block_read_grants_are_live,
        block_write_and_reset_keep_prior_byte,
        block_failed_write_keeps_read_state,
        block_io_grants_hold_input_and_output,
        block_repeat_phase_keeps_base_effects,
    );
}

mod ed_internal {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        nibble_read_is_live_after_grant,
        nibble_grant_keeps_read_result,
        nibble_reset_and_fault_keep_uncommitted_registers,
        arithmetic_and_special_internal_grants,
    );
}

mod ed_io {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        ed_io_grant_keeps_terminal_and_registers,
        ed_io_reset_cancels_pending_input,
        ed_io_disconnected_ports_preserve_machine_state,
    );
}

mod ed_transfer {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        ed_word_address_high_is_live_after_grant,
        ed_word_keeps_low_and_reads_high_live,
        ed_word_write_grant_and_reset_keep_first_byte,
        ed_returns_keep_low_and_read_high_live,
        ed_second_access_faults_keep_progress,
    );
}

mod im0 {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        jump_low_is_latched_and_high_is_live,
        jump_reset_discards_pending_operand,
        conditional_jump_reads_high_before_deciding,
        pending_jump_defers_irq_and_pushes_target,
        immediate_loads_preserve_index_and_controller_rules,
    );
}

mod indexed {
    #[path = "cases.rs"]
    mod cases;
    native_cases!(
        indexed_register_memory_and_cb,
        prefix_grant_observes_modified_terminal,
        pending_prefix_clone_and_reset,
        terminal_fault_preserves_prefixes,
        retained_prefix_input_and_ignored_ed,
    );
}

mod indexed_cb {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        indexed_cb_terminal_is_live_after_grant,
        indexed_cb_read_result_and_register_copy_are_latched,
        indexed_bit_reads_after_grant_without_register_copy,
        indexed_cb_reset_and_fault_preserve_progress,
    );
}

mod indexed_memory {
    #[path = "cases.rs"]
    mod cases;
    native_cases!(
        indexed_address_and_read_after_grant,
        indexed_rmw_keeps_read_result,
        indexed_immediate_write_is_latched,
        indexed_reset_and_write_fault,
    );
}

mod interrupt_service {
    #[path = "cases.rs"]
    pub(crate) mod cases;

    native_cases!(
        manual_irq_modes_precede_prefixed_opcodes,
        concurrent_irq_preserves_live_line_and_partial_stack,
        failed_irq_write_retains_prior_progress_and_reset_line,
        im2_vector_high_is_live_after_grant,
        reset_and_halt_interrupts_keep_controller_state,
    );
}
