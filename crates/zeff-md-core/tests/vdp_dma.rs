mod vdp_dma_cpu {
    #[path = "cases.rs"]
    mod cases;
    use super::vdp_dma_device::support;

    macro_rules! case {
        ($name:ident, $with:ident) => {
            #[test]
            fn $name() {
                cases::$with(&mut support::execute);
            }
        };
    }

    case!(copy_command_start, copy_command_start_with);
    case!(fill_wait_and_trigger, fill_wait_and_trigger_with);
    case!(fill_fifo_priority, fill_fifo_priority_with);
    case!(command_guards_and_busy, command_guards_and_busy_with);
    case!(
        waiting_clones_refresh_and_zero,
        waiting_clones_refresh_and_zero_with
    );
}

mod vdp_dma_device {
    #[path = "cases.rs"]
    mod cases;
    #[path = "guards.rs"]
    mod guards;
    #[path = "irq.rs"]
    mod irq;
    #[path = "long.rs"]
    mod long;
    #[path = "support.rs"]
    pub(crate) mod support;

    macro_rules! case {
        ($name:ident, $module:ident, $with:ident) => {
            #[test]
            fn $name() {
                $module::$with(&mut support::execute);
            }
        };
    }

    case!(fill_slots, cases, fill_slots_with);
    case!(copy_slots_and_clones, cases, copy_slots_and_clones_with);
    case!(all_increments, cases, all_increments_with);
    case!(overlap_and_partitions, cases, overlap_and_partitions_with);
    case!(
        source_wrap_and_zero_length,
        cases,
        source_wrap_and_zero_length_with
    );
    case!(
        dma_invalidates_cached_word,
        cases,
        dma_invalidates_cached_word_with
    );
    case!(busy_ports_and_restart, cases, busy_ports_and_restart_with);
    case!(long_partitions, long, long_partitions_with);
    case!(irq_tie_phases, irq, irq_tie_phases_with);
    case!(zero_count_completion, irq, zero_count_completion_with);
    case!(irq_tie_chunk_failure, irq, irq_tie_chunk_failure_with);
    case!(
        long_overlap_and_source_wrap,
        long,
        long_overlap_and_source_wrap_with
    );
    case!(
        zero_length_continuation,
        long,
        zero_length_continuation_with
    );
    case!(late_carry_and_iack, long, late_carry_and_iack_with);
    case!(
        long_active_field_margin,
        long,
        long_active_field_margin_with
    );
    case!(seed_validation, guards, seed_validation_with);
    case!(pending_inputs, guards, pending_inputs_with);
    case!(chronological_capacity, guards, chronological_capacity_with);
    case!(clock_guards, guards, clock_guards_with);
    case!(
        staged_failures_and_iack,
        guards,
        staged_failures_and_iack_with
    );
    case!(
        capture_and_future_margin,
        guards,
        capture_and_future_margin_with
    );
    case!(
        ordinary_cpu_dma_stays_guarded,
        guards,
        ordinary_cpu_dma_stays_guarded_with
    );
}

mod vdp_dma_external_regs {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn external_source_images_and_rewrites() {
        cases::external_source_images_and_rewrites();
    }

    #[test]
    fn external_counts_and_page_wrap() {
        cases::external_counts_and_page_wrap();
    }

    #[test]
    fn external_destinations_and_retirement() {
        cases::external_destinations_and_retirement();
    }

    #[test]
    fn external_failures_are_atomic() {
        cases::external_failures_are_atomic();
    }
}

mod vdp_dma_external {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn fifo_acceptance_and_tail() {
        cases::fifo_acceptance_and_tail();
    }
    #[test]
    fn source_pages_and_ram_mirrors() {
        cases::source_pages_and_ram_mirrors();
    }
    #[test]
    fn whole_horizon_source_fault_is_atomic() {
        cases::whole_horizon_source_fault_is_atomic();
    }
    #[test]
    fn zero_length_and_partitioned_transfer() {
        cases::zero_length_and_partitioned_transfer();
    }
    #[test]
    fn active_fifo_prefix_and_sprite_retirement() {
        cases::active_fifo_prefix_and_sprite_retirement();
    }
}

mod vdp_dma_phases {
    #[path = "batches.rs"]
    mod batches;
    #[path = "phases.rs"]
    mod phases;
    #[path = "support.rs"]
    mod support;

    #[test]
    fn copy_read_write_visibility() {
        phases::copy_read_write_visibility();
    }
    #[test]
    fn phase_clones_and_interleaving() {
        phases::phase_clones_and_interleaving();
    }
    #[test]
    fn phase_guards() {
        phases::phase_guards();
    }
    #[test]
    fn all_increment_batch_layouts() {
        batches::all_increment_batch_layouts();
    }
    #[test]
    fn overlap_and_chunks() {
        batches::overlap_and_chunks();
    }
    #[test]
    fn batch_capacity_and_failures() {
        batches::batch_capacity_and_failures();
    }
    #[test]
    fn batch_completion_and_zero_count() {
        batches::batch_completion_and_zero_count();
    }
}

mod vdp_external_machine {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn pal_rom_and_ram_dma_to_frame() {
        cases::pal_rom_and_ram_dma_to_frame();
    }

    #[test]
    fn actual_rom_and_ram_dma_to_frame() {
        cases::actual_rom_and_ram_dma_to_frame();
    }
    #[test]
    fn unsupported_source_freezes_without_deposit() {
        cases::unsupported_source_freezes_without_deposit();
    }
}

mod vdp_local_dma {
    #[path = "cases.rs"]
    mod cases;
    #[path = "guards.rs"]
    mod guards;
    #[path = "support.rs"]
    pub(crate) mod support;

    #[test]
    fn published_register_images() {
        guards::published_register_images();
    }

    #[test]
    fn fill_trigger_and_lanes() {
        cases::fill_trigger_and_lanes();
    }

    #[test]
    fn all_increments_and_live_fill_data() {
        cases::all_increments_and_live_fill_data();
    }

    #[test]
    fn copy_lanes_and_source_wrap() {
        cases::copy_lanes_and_source_wrap();
    }

    #[test]
    fn progressive_overlap() {
        cases::progressive_overlap();
    }

    #[test]
    fn full_count_and_completion() {
        cases::full_count_and_completion();
    }

    #[test]
    fn clones_and_retained_memory() {
        cases::clones_and_retained_memory();
    }

    #[test]
    fn transfer_faults_are_atomic() {
        guards::transfer_faults_are_atomic();
    }

    #[test]
    fn device_dma_guards_remain() {
        guards::device_dma_guards_remain();
    }
}
