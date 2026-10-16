#[allow(dead_code)]
#[path = "multi_transfer/cases.rs"]
pub(crate) mod multi_transfer_cases;
#[allow(dead_code)]
#[path = "sound/fixture.rs"]
pub(crate) mod sound_fixture;

mod machine {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn bounded_loading_and_startup() {
        cases::bounded_loading_and_startup();
    }
    #[test]
    fn actual_program_and_boundary_copies() {
        cases::actual_program_and_boundary_copies();
    }
    #[test]
    fn lanes_and_immutable_cartridge() {
        cases::lanes_and_immutable_cartridge();
    }
    #[test]
    fn stopped_clock_irq_and_return() {
        cases::stopped_clock_irq_and_return();
    }
    #[test]
    fn level_seven_edge_rearming() {
        cases::level_seven_edge_rearming();
    }
    #[test]
    fn distinct_reset_ownership() {
        cases::distinct_reset_ownership();
    }
    #[test]
    fn unsupported_access_is_host_fault_without_retry() {
        cases::unsupported_access_is_host_fault_without_retry();
    }
    #[test]
    fn address_error_remains_explicitly_unqualified() {
        cases::address_error_remains_explicitly_unqualified();
    }
    #[test]
    fn ordinary_trace_enters_mapped_vector_and_stack() {
        cases::ordinary_trace_enters_mapped_vector_and_stack();
    }
}

mod input {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn controller_multiplexing() {
        cases::controller_multiplexing();
    }
    #[test]
    fn pin_direction_and_latch() {
        cases::pin_direction_and_latch();
    }
    #[test]
    fn register_lanes() {
        cases::register_lanes();
    }
    #[test]
    fn serial_register_lanes_and_state() {
        cases::serial_register_lanes_and_state();
    }
    #[test]
    fn input_reset_and_copy() {
        cases::input_reset_and_copy();
    }
    #[test]
    fn absent_devices_fail_once() {
        cases::absent_devices_fail_once();
    }
    #[test]
    fn concurrent_controller_access() {
        cases::concurrent_controller_access();
    }
}

mod six_button {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn six_button_sequence() {
        cases::six_button_sequence();
    }

    #[test]
    fn six_button_effective_th() {
        cases::six_button_effective_th();
    }

    #[test]
    fn six_button_timeout() {
        cases::six_button_timeout();
    }

    #[test]
    fn six_button_identity_and_mode() {
        cases::six_button_identity_and_mode();
    }

    #[test]
    fn six_button_clone_and_ports() {
        cases::six_button_clone_and_ports();
    }

    #[test]
    fn six_button_lanes_and_default() {
        cases::six_button_lanes_and_default();
    }
}

mod absent_expansion {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn absent_views_reject_main_transfers_and_preserve_completed_effects() {
        cases::absent_views_reject_main_transfers_and_preserve_completed_effects();
    }

    #[test]
    fn absent_views_do_not_supply_peek_backing() {
        cases::absent_views_do_not_supply_peek_backing();
    }
}

mod open_bus {
    #[path = "cases.rs"]
    mod cases;
    use crate::sound_fixture as fixture;

    #[test]
    fn open_bus_literal_prefetch() {
        cases::open_bus_literal_prefetch();
    }

    #[test]
    fn open_bus_aliases_and_logical_addresses() {
        cases::open_bus_aliases_and_logical_addresses();
    }

    #[test]
    fn open_bus_all_prefetched_high_bytes_without_execution() {
        cases::open_bus_all_prefetched_high_bytes_without_execution();
    }

    #[test]
    fn open_bus_work_ram_prefetch() {
        cases::open_bus_work_ram_prefetch();
    }

    #[test]
    fn open_bus_indirect_clone_and_reset() {
        cases::open_bus_indirect_clone_and_reset();
    }

    #[test]
    fn open_bus_guards_and_consumed_observation() {
        cases::open_bus_guards_and_consumed_observation();
    }
}

mod requesters {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn literal_wait_ledger() {
        cases::literal_wait_ledger();
    }

    #[test]
    fn three_requesters_all_arrival_orders() {
        cases::three_requesters_all_arrival_orders();
    }

    #[test]
    fn chained_conflicts_ignore_reservation_order() {
        cases::chained_conflicts_ignore_reservation_order();
    }

    #[test]
    fn equality_only_identity_and_aliases() {
        cases::equality_only_identity_and_aliases();
    }

    #[test]
    fn exact_ties_preserve_grant_sequence() {
        cases::exact_ties_preserve_grant_sequence();
    }

    #[test]
    fn unchanged_inputs_and_clones() {
        cases::unchanged_inputs_and_clones();
    }
}

mod pal_machine {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn actual_cpu_region_ports_and_clock() {
        cases::actual_cpu_region_ports_and_clock();
    }

    #[test]
    fn dma_crosses_field_and_preserves_source_failure() {
        cases::dma_crosses_field_and_preserves_source_failure();
    }
}

mod banked_ram {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn banked_ram_increment_reads_ff() {
        cases::banked_ram_increment_reads_ff();
    }

    #[test]
    fn banked_ram_all_banks_offsets_and_mirrors() {
        cases::banked_ram_all_banks_offsets_and_mirrors();
    }

    #[test]
    fn banked_ram_all_byte_values_read_ff() {
        cases::banked_ram_all_byte_values_read_ff();
    }

    #[test]
    fn banked_ram_manual_two_writes_and_window_wrap() {
        cases::banked_ram_manual_two_writes_and_window_wrap();
    }

    #[test]
    fn banked_ram_main_interleave_preserves_disjoint_writes() {
        cases::banked_ram_main_interleave_preserves_disjoint_writes();
    }

    #[test]
    fn banked_ram_grant_fault_clone_reset_and_guards() {
        cases::banked_ram_grant_fault_clone_reset_and_guards();
    }
}

mod bank_reservation {
    #[path = "cases.rs"]
    pub(crate) mod cases;

    #[test]
    fn load_immediate_bank_write() {
        cases::load_immediate_bank_write();
    }
    #[test]
    fn load_absolute_byte_write() {
        cases::load_absolute_byte_write();
    }
    #[test]
    fn load_absolute_word_write() {
        cases::load_absolute_word_write();
    }
    #[test]
    fn load_absolute_byte_read() {
        cases::load_absolute_byte_read();
    }
    #[test]
    fn load_absolute_word_read() {
        cases::load_absolute_word_read();
    }
    #[test]
    fn ed_absolute_pairs_reserve_reads_and_writes() {
        cases::ed_absolute_pairs_reserve_reads_and_writes();
    }
    #[test]
    fn indexed_absolute_pairs_reserve_reads_and_writes() {
        cases::indexed_absolute_pairs_reserve_reads_and_writes();
    }
    #[test]
    fn default_manual_loads_remain_nominal() {
        cases::default_manual_loads_remain_nominal();
    }
    #[test]
    fn local_load_and_stack_timing() {
        cases::local_load_and_unclassified_stack_remain_nominal();
    }
    #[test]
    fn consecutive_bank_segments_make_progress() {
        cases::consecutive_bank_segments_make_progress();
    }
    #[test]
    fn pending_bank_clone_and_reset() {
        cases::pending_bank_clone_and_reset();
    }
    #[test]
    fn grant_preserves_latched_address_and_prior_write() {
        cases::grant_preserves_latched_address_and_prior_write();
    }
    #[test]
    fn fault_retains_prior_reserved_read_and_partial_write() {
        cases::fault_retains_prior_reserved_read_and_partial_write();
    }
}

mod stack_reservation {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn ordinary_push_pairs_reserve_both_writes() {
        cases::ordinary_push_pairs_reserve_both_writes();
    }
    #[test]
    fn ordinary_pop_pairs_reserve_both_reads() {
        cases::ordinary_pop_pairs_reserve_both_reads();
    }
    #[test]
    fn indexed_push_pop_reserve_bank_data() {
        cases::indexed_push_pop_reserve_bank_data();
    }
    #[test]
    fn return_restart_call_and_jump_reserve_bank_cycles() {
        cases::return_restart_call_and_jump_reserve_bank_cycles();
    }
    #[test]
    fn exchange_reserves_fixed_phases_and_retains_mixed_tails() {
        cases::exchange_reserves_fixed_phases_and_retains_mixed_tails();
    }
    #[test]
    fn consecutive_pushes_preserve_order_and_main_progress() {
        cases::consecutive_pushes_preserve_order_and_main_progress();
    }
    #[test]
    fn pending_stack_clone_and_reset_preserve_prior_write() {
        cases::pending_stack_clone_and_reset_preserve_prior_write();
    }
    #[test]
    fn stack_grant_preserves_latched_value() {
        cases::stack_grant_preserves_latched_value();
    }
    #[test]
    fn stack_fault_retains_prior_bank_write() {
        cases::stack_fault_retains_prior_bank_write();
    }
    #[test]
    fn manual_and_local_stack_remain_nominal() {
        cases::manual_and_local_stack_remain_nominal();
    }
}
