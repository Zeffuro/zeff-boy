mod vdp_control {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn literal_command_fields() {
        cases::literal_command_fields();
    }

    #[test]
    fn register_requests_and_ignored_tag_bit() {
        cases::register_requests_and_ignored_tag_bit();
    }

    #[test]
    fn command_and_register_encoding_partition() {
        cases::command_and_register_encoding_partition();
    }

    #[test]
    fn register_pattern_after_first_half_is_command_data() {
        cases::register_pattern_after_first_half_is_command_data();
    }

    #[test]
    fn unused_second_word_bits_do_not_change_request() {
        cases::unused_second_word_bits_do_not_change_request();
    }

    #[test]
    fn cancellation_and_pending_copies() {
        cases::cancellation_and_pending_copies();
    }

    #[test]
    fn independent_decoder_instances() {
        cases::independent_decoder_instances();
    }

    #[test]
    fn machine_port_admission_guards() {
        cases::machine_ports_remain_unadmitted();
    }
}

mod vdp_control_fields {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn first_words_replace_low_fields() {
        cases::first_words_replace_low_fields();
    }
    #[test]
    fn second_words_gate_dma_bit() {
        cases::second_words_gate_dma_bit();
    }
    #[test]
    fn invalid_images_preserve_pending_command() {
        cases::invalid_images_preserve_pending_command();
    }
    #[test]
    fn write_targets_ignore_only_cd4() {
        cases::write_targets_ignore_only_cd4();
    }
    #[test]
    fn register_write_preserves_vsram_target() {
        cases::register_write_preserves_vsram_target();
    }
}

mod vdp_register_bank {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn published_bank_tracks_payloads() {
        cases::published_bank_tracks_payloads();
    }
    #[test]
    fn table_addresses_follow_published_bits() {
        cases::table_addresses_follow_published_bits();
    }
    #[test]
    fn replacement_and_rejections_preserve_bank() {
        cases::replacement_and_rejections_preserve_bank();
    }
    #[test]
    fn published_addresses_and_increment_drive_fifo() {
        cases::published_addresses_and_increment_drive_fifo();
    }
}

mod vdp_registers {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn known_phase_deadlines_and_boundaries() {
        cases::known_phase_deadlines_and_boundaries();
    }
    #[test]
    fn register_payload_partition() {
        cases::register_payload_partition();
    }
    #[test]
    fn copied_payload_and_chunk_invariance() {
        cases::copied_payload_and_chunk_invariance();
    }
    #[test]
    fn busy_and_invalid_requests_are_atomic() {
        cases::busy_and_invalid_requests_are_atomic();
    }
    #[test]
    fn reverse_time_and_overflow_are_atomic() {
        cases::reverse_time_and_overflow_are_atomic();
    }
    #[test]
    fn decoder_requests_wait_for_publication() {
        cases::decoder_requests_wait_for_publication();
    }
}

mod vdp_increment {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn publication_changes_only_at_the_deadline() {
        cases::publication_changes_only_at_the_deadline();
    }

    #[test]
    fn all_increment_values_survive_publication() {
        cases::all_increment_values_survive_publication();
    }

    #[test]
    fn unrelated_requests_and_guards_preserve_increment() {
        cases::unrelated_requests_and_guards_preserve_increment();
    }

    #[test]
    fn cloned_pending_settings_and_chunk_invariance() {
        cases::cloned_pending_settings_and_chunk_invariance();
    }
}

mod vdp_reads {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn completed_words_ignore_odd_address_without_swapping() {
        cases::completed_words_ignore_odd_address_without_swapping();
    }
    #[test]
    fn cached_words_are_owned_and_retained() {
        cases::cached_words_are_owned_and_retained();
    }
    #[test]
    fn fetch_waits_for_the_entire_fifo() {
        cases::fetch_waits_for_the_entire_fifo();
    }
    #[test]
    fn read_failures_preserve_cache_and_command() {
        cases::read_failures_preserve_cache_and_command();
    }
    #[test]
    fn published_increment_and_cloned_read_streams() {
        cases::published_increment_and_cloned_read_streams();
    }
}

mod vdp_writes {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn vram_word_lanes_and_edges() {
        cases::vram_word_lanes_and_edges();
    }
    #[test]
    fn color_word_masks_and_aliases() {
        cases::color_word_masks_and_aliases();
    }
    #[test]
    fn fifo_capture_order_and_reuse() {
        cases::fifo_capture_order_and_reuse();
    }
    #[test]
    fn full_and_invalid_pushes_are_atomic() {
        cases::full_and_invalid_pushes_are_atomic();
    }
    #[test]
    fn unsupported_completions_preserve_memory_and_queue() {
        cases::unsupported_completions_preserve_memory_and_queue();
    }
    #[test]
    fn copied_pending_writes_and_retained_slots() {
        cases::copied_pending_writes_and_retained_slots();
    }
    #[test]
    fn fifo_history_retains_unmasked_data() {
        cases::fifo_history_retains_unmasked_data();
    }
}

mod vdp_status {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn fifo_flags_follow_occupancy_and_reuse() {
        cases::fifo_flags_follow_occupancy_and_reuse();
    }
    #[test]
    fn failed_transfers_preserve_fifo_flags() {
        cases::failed_transfers_preserve_fifo_flags();
    }
    #[test]
    fn read_cache_readiness_is_independent() {
        cases::read_cache_readiness_is_independent();
    }
}

mod vdp_prefetch {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn fifo_forwarding_and_segmented_vram_fetch() {
        cases::fifo_forwarding_and_segmented_vram_fetch();
    }

    #[test]
    fn automatic_next_fetch_advances_once_and_retains_old_word() {
        cases::automatic_next_fetch_advances_once_and_retains_old_word();
    }

    #[test]
    fn color_read_uses_raw_retained_fifo_tail_and_one_slot() {
        cases::color_read_uses_raw_retained_fifo_tail_and_one_slot();
    }

    #[test]
    fn unknown_history_rejects_entire_command_and_keeps_pending_writes() {
        cases::unknown_history_rejects_entire_command_and_keeps_pending_writes();
    }

    #[test]
    fn control_invalidation_and_interrupted_second_phase_are_atomic() {
        cases::control_invalidation_and_interrupted_second_phase_are_atomic();
    }

    #[test]
    fn phase_zero_command_transition_cancels_pending_fetch() {
        cases::phase_zero_command_transition_cancels_pending_fetch();
    }

    #[test]
    fn refresh_and_strict_next_slot_boundaries() {
        cases::refresh_and_strict_next_slot_boundaries();
    }

    #[test]
    fn unsupported_address_increment_and_carry_reject_before_fetch() {
        cases::unsupported_address_increment_and_carry_reject_before_fetch();
    }

    #[test]
    fn mode_guards_preserve_ready_cache_and_pending_control() {
        cases::mode_guards_preserve_ready_cache_and_pending_control();
    }

    #[test]
    fn command_and_consumption_preflight_future_irq_and_clock_tail() {
        cases::command_and_consumption_preflight_future_irq_and_clock_tail();
    }

    #[test]
    fn acknowledge_fetch_collision_leaves_cache_clock_and_memory_unchanged() {
        cases::acknowledge_fetch_collision_leaves_cache_clock_and_memory_unchanged();
    }
}

mod vdp_data {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn byte_order_and_split_service() {
        cases::byte_order_and_split_service();
    }
    #[test]
    fn color_masks_and_address_aliases() {
        cases::color_masks_and_address_aliases();
    }
    #[test]
    fn captured_target_increment_and_parser() {
        cases::captured_target_increment_and_parser();
    }
    #[test]
    fn full_ring_and_retirement_wait() {
        cases::full_ring_and_retirement_wait();
    }
    #[test]
    fn refresh_and_large_horizon() {
        cases::refresh_and_large_horizon();
    }
    #[test]
    fn mode_address_and_clock_guards() {
        cases::mode_address_and_clock_guards();
    }
    #[test]
    fn publication_and_acknowledge_atomicity() {
        cases::publication_and_acknowledge_atomicity();
    }
}

mod vdp_ports {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn settled_command_fields() {
        cases::settled_command_fields();
    }
    #[test]
    fn delayed_register_publication() {
        cases::delayed_register_publication();
    }
    #[test]
    fn pending_clone_and_bootstrap() {
        cases::pending_clone_and_bootstrap();
    }
    #[test]
    fn dma_gate_and_rejection() {
        cases::dma_gate_and_rejection();
    }
    #[test]
    fn register_fault_atomicity() {
        cases::register_fault_atomicity();
    }
    #[test]
    fn unsupported_region_atomicity() {
        cases::unsupported_region_atomicity();
    }
    #[test]
    fn unsupported_command_atomicity() {
        cases::unsupported_command_atomicity();
    }
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

    #[test]
    fn invalidation_retains_completed_history() {
        cases::invalidation_retains_completed_history();
    }
    #[test]
    fn accepted_control_writes_require_another_fetch() {
        cases::accepted_control_writes_require_another_fetch();
    }
    #[test]
    fn accepted_data_writes_invalidate_after_capture() {
        cases::accepted_data_writes_invalidate_after_capture();
    }
}

mod vdp_byte_service {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn vram_bytes_publish_in_order() {
        cases::vram_bytes_publish_in_order();
    }
    #[test]
    fn partial_heads_preserve_occupancy_and_clones() {
        cases::partial_heads_preserve_occupancy_and_clones();
    }
    #[test]
    fn completed_api_does_not_replay_first_byte() {
        cases::completed_api_does_not_replay_first_byte();
    }
    #[test]
    fn rejected_service_preserves_state() {
        cases::rejected_service_preserves_state();
    }
}

mod vdp_color_reads {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn color_word_masks_and_address_aliases() {
        cases::color_word_masks_and_address_aliases();
    }
    #[test]
    fn defined_and_inherited_bit_basis() {
        cases::defined_and_inherited_bit_basis();
    }
    #[test]
    fn retained_fifo_ring_and_cache_ownership() {
        cases::retained_fifo_ring_and_cache_ownership();
    }
    #[test]
    fn unknown_history_and_pending_fifo_guards() {
        cases::unknown_history_and_pending_fifo_guards();
    }
    #[test]
    fn holes_and_unsupported_targets_preserve_state() {
        cases::holes_and_unsupported_targets_preserve_state();
    }
}
