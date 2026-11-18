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

mod machine {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        bounded_loading_and_startup,
        actual_program_and_boundary_copies,
        lanes_and_immutable_cartridge,
        stopped_clock_irq_and_return,
        level_seven_edge_rearming,
        distinct_reset_ownership,
        unsupported_access_is_host_fault_without_retry,
        address_error_remains_explicitly_unqualified,
        ordinary_trace_enters_mapped_vector_and_stack,
    );
}

mod input {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        controller_multiplexing,
        pin_direction_and_latch,
        register_lanes,
        serial_register_lanes_and_state,
        input_reset_and_copy,
        absent_devices_fail_once,
        concurrent_controller_access,
    );
}

mod six_button {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        six_button_sequence,
        six_button_effective_th,
        six_button_timeout,
        six_button_identity_and_mode,
        six_button_clone_and_ports,
        six_button_lanes_and_default,
    );
}

mod absent_expansion {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        absent_views_reject_main_transfers_and_preserve_completed_effects,
        absent_views_do_not_supply_peek_backing,
    );
}

mod open_bus {
    #[path = "cases.rs"]
    mod cases;
    use crate::sound_fixture as fixture;

    native_cases!(
        open_bus_literal_prefetch,
        open_bus_aliases_and_logical_addresses,
        open_bus_all_prefetched_high_bytes_without_execution,
        open_bus_work_ram_prefetch,
        open_bus_indirect_clone_and_reset,
        open_bus_guards_and_consumed_observation,
    );
}

mod requesters {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        literal_wait_ledger,
        three_requesters_all_arrival_orders,
        chained_conflicts_ignore_reservation_order,
        equality_only_identity_and_aliases,
        exact_ties_preserve_grant_sequence,
        unchanged_inputs_and_clones,
    );
}

mod pal_machine {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        actual_cpu_region_ports_and_clock,
        dma_crosses_field_and_preserves_source_failure,
    );
}

mod banked_ram {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        banked_ram_increment_reads_ff,
        banked_ram_all_banks_offsets_and_mirrors,
        banked_ram_all_byte_values_read_ff,
        banked_ram_manual_two_writes_and_window_wrap,
        banked_ram_main_interleave_preserves_disjoint_writes,
        banked_ram_grant_fault_clone_reset_and_guards,
    );
}

mod bank_reservation {
    #[path = "cases.rs"]
    pub(crate) mod cases;

    native_cases!(
        load_immediate_bank_write,
        load_absolute_byte_write,
        load_absolute_word_write,
        load_absolute_byte_read,
        load_absolute_word_read,
        ed_absolute_pairs_reserve_reads_and_writes,
        indexed_absolute_pairs_reserve_reads_and_writes,
        default_manual_loads_remain_nominal,
    );
    #[test]
    fn local_load_and_stack_timing() {
        cases::local_load_and_unclassified_stack_remain_nominal();
    }
    native_cases!(
        consecutive_bank_segments_make_progress,
        pending_bank_clone_and_reset,
        grant_preserves_latched_address_and_prior_write,
        fault_retains_prior_reserved_read_and_partial_write,
    );
}

mod stack_reservation {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        ordinary_push_pairs_reserve_both_writes,
        ordinary_pop_pairs_reserve_both_reads,
        indexed_push_pop_reserve_bank_data,
        return_restart_call_and_jump_reserve_bank_cycles,
        exchange_reserves_fixed_phases_and_retains_mixed_tails,
        consecutive_pushes_preserve_order_and_main_progress,
        pending_stack_clone_and_reset_preserve_prior_write,
        stack_grant_preserves_latched_value,
        stack_fault_retains_prior_bank_write,
        manual_and_local_stack_remain_nominal,
    );
}
