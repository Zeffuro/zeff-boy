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

mod vdp_control {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        literal_command_fields,
        register_requests_and_ignored_tag_bit,
        command_and_register_encoding_partition,
        register_pattern_after_first_half_is_command_data,
        unused_second_word_bits_do_not_change_request,
        cancellation_and_pending_copies,
        independent_decoder_instances,
    );

    #[test]
    fn machine_port_admission_guards() {
        cases::machine_ports_remain_unadmitted();
    }
}

mod vdp_control_fields {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        first_words_replace_low_fields,
        second_words_gate_dma_bit,
        invalid_images_preserve_pending_command,
        write_targets_ignore_only_cd4,
        register_write_preserves_vsram_target,
    );
}

mod vdp_register_bank {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        published_bank_tracks_payloads,
        table_addresses_follow_published_bits,
        replacement_and_rejections_preserve_bank,
        published_addresses_and_increment_drive_fifo,
    );
}

mod vdp_registers {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        known_phase_deadlines_and_boundaries,
        register_payload_partition,
        copied_payload_and_chunk_invariance,
        busy_and_invalid_requests_are_atomic,
        reverse_time_and_overflow_are_atomic,
        decoder_requests_wait_for_publication,
    );
}

mod vdp_increment {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        publication_changes_only_at_the_deadline,
        all_increment_values_survive_publication,
        unrelated_requests_and_guards_preserve_increment,
        cloned_pending_settings_and_chunk_invariance,
    );
}

mod vdp_reads {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        completed_words_ignore_odd_address_without_swapping,
        cached_words_are_owned_and_retained,
        fetch_waits_for_the_entire_fifo,
        read_failures_preserve_cache_and_command,
        published_increment_and_cloned_read_streams,
    );
}

mod vdp_writes {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        vram_word_lanes_and_edges,
        color_word_masks_and_aliases,
        fifo_capture_order_and_reuse,
        full_and_invalid_pushes_are_atomic,
        unsupported_completions_preserve_memory_and_queue,
        copied_pending_writes_and_retained_slots,
        fifo_history_retains_unmasked_data,
    );
}

mod vdp_status {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        fifo_flags_follow_occupancy_and_reuse,
        failed_transfers_preserve_fifo_flags,
        read_cache_readiness_is_independent,
    );
}

mod vdp_prefetch {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        fifo_forwarding_and_segmented_vram_fetch,
        automatic_next_fetch_advances_once_and_retains_old_word,
        color_read_uses_raw_retained_fifo_tail_and_one_slot,
        unknown_history_rejects_entire_command_and_keeps_pending_writes,
        control_invalidation_and_interrupted_second_phase_are_atomic,
        phase_zero_command_transition_cancels_pending_fetch,
        refresh_and_strict_next_slot_boundaries,
        unsupported_address_increment_and_carry_reject_before_fetch,
        mode_guards_preserve_ready_cache_and_pending_control,
        command_and_consumption_preflight_future_irq_and_clock_tail,
        acknowledge_fetch_collision_leaves_cache_clock_and_memory_unchanged,
    );
}

mod vdp_data {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        byte_order_and_split_service,
        color_masks_and_address_aliases,
        captured_target_increment_and_parser,
        full_ring_and_retirement_wait,
        refresh_and_large_horizon,
        mode_address_and_clock_guards,
        publication_and_acknowledge_atomicity,
    );
}

mod vdp_ports {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        settled_command_fields,
        delayed_register_publication,
        pending_clone_and_bootstrap,
        dma_gate_and_rejection,
        register_fault_atomicity,
        unsupported_region_atomicity,
        unsupported_command_atomicity,
    );
}

mod vdp_status_port {
    #[path = "cases.rs"]
    mod cases;

    macro_rules! status_tests {
        ($($name:ident),+ $(,)?) => {
            $(#[test]
            fn $name() {
                cases::$name();
            })+
        };
    }

    status_tests!(
        bootstrap_driven_bits_and_ntsc_regions,
        hblank_edges_and_slot_boundary,
        status_cancels_only_parser_half_and_restarts_command,
        fifo_flags_and_raw_history_are_read_only,
        vertical_pending_survives_status_until_acknowledge,
        ready_cache_rejection_is_atomic,
        pending_prefetch_rejection_is_atomic,
        invalid_cache_keeps_buffered_word,
        display_history_remains_unknown_after_disable,
        unknown_snapshot_and_mode_guards_are_atomic,
    );
}

mod vdp_cache_invalidation {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        invalidation_retains_completed_history,
        accepted_control_writes_require_another_fetch,
        accepted_data_writes_invalidate_after_capture,
    );
}

mod vdp_byte_service {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        vram_bytes_publish_in_order,
        partial_heads_preserve_occupancy_and_clones,
        completed_api_does_not_replay_first_byte,
        rejected_service_preserves_state,
    );
}

mod vdp_color_reads {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        color_word_masks_and_address_aliases,
        defined_and_inherited_bit_basis,
        retained_fifo_ring_and_cache_ownership,
        unknown_history_and_pending_fifo_guards,
        holes_and_unsupported_targets_preserve_state,
    );
}
