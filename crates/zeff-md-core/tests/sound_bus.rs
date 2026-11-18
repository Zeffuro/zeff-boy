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

mod sound {
    #[path = "cases.rs"]
    mod cases;
    #[path = "guards.rs"]
    mod guards;

    native_cases!(
        shared_value_and_boundary_continuation,
        main_sound_ram_lanes_and_mirrors,
        serial_bank_and_cartridge_requester,
        reset_and_halt_ownership,
    );
    #[test]
    fn rejected_sound_steps_preserve_machine() {
        guards::rejected_sound_steps_preserve_machine();
    }
    #[test]
    fn unavailable_main_access_is_host_fault() {
        guards::unavailable_main_access_is_host_fault();
    }
    #[test]
    fn independent_component_reset() {
        guards::independent_component_reset();
    }
    native_cases!(sound_control_lanes_and_busy_bit,);
}

mod sound_control {
    #[path = "cases.rs"]
    mod cases;
    use crate::sound_fixture as fixture;
    #[path = "lifecycle.rs"]
    mod lifecycle;

    native_cases!(
        control_mirror_request_and_reset,
        control_mirror_lanes_and_bits,
        control_mirror_busy_reads,
        control_mirror_adjacent_blocks,
    );

    #[test]
    fn control_mirror_hot_cpu_and_clone() {
        lifecycle::control_mirror_hot_cpu_and_clone();
    }

    #[test]
    fn control_mirror_concurrent_lifecycle() {
        lifecycle::control_mirror_concurrent_lifecycle();
    }
}

mod disconnected {
    #[path = "cases.rs"]
    mod cases;
    native_cases!(
        disconnected_ports_consume_clocks,
        unused_sound_memory_and_main_lanes,
        unsupported_devices_remain_faults,
    );
}

mod concurrent {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        delayed_write_and_pending_clone,
        requested_read_waits_once,
        reset_discards_unpublished_store,
        main_reset_preserves_pending_time,
        unsupported_sound_latches_after_prior_progress,
        halted_sound_ownership,
    );
}

mod prefix {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        cb_register_and_memory_operations,
        ed_block_iterations_and_wraparound,
        ed_faults_are_transactional,
        concurrent_ed_handover_and_pending_clone,
        concurrent_cb_memory_is_delayed,
        extended_fetch_and_bank_guards,
    );
}

mod transfer {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        immediate_operand_changes_after_grant,
        memory_read_and_write_resume_after_grant,
        transfer_fault_preserves_m1,
        transfer_reset_ownership,
    );
}

mod multi_transfer {
    use crate::multi_transfer_cases as cases;

    native_cases!(
        immediate_low_latched_high_live,
        address_and_partial_write_survive_grant,
        partial_write_fault_and_reset,
        partial_read_latched_low_live_high,
    );
}

mod stack {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        push_grant_preserves_completed_high_write,
        pop_grant_keeps_low_and_reads_high_live,
        exchange_grant_preserves_latched_pair_and_write_order,
        exchange_reset_and_second_write_fault,
        push_and_pop_second_access_faults_keep_progress,
    );
}

mod control_stack {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        call_reads_high_operand_after_grant,
        call_keeps_target_and_completed_write,
        return_keeps_low_and_reads_high_live,
        restart_keeps_completed_write_and_return_address,
        control_faults_and_reset_preserve_progress,
    );
}

mod rmw {
    #[path = "cases.rs"]
    mod cases;
    native_cases!(
        rmw_grant_keeps_read_result,
        rmw_reset_discards_write,
        rmw_write_fault_keeps_read_phase,
    );
}

mod psg {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        register_decoder,
        reloads_preserve_countdowns,
        noise_sequences_and_rates,
        digital_clock_and_pcm_policy,
        jump_and_partitioned_clocks,
        attenuation_and_mix,
    );
}

mod psg_machine {
    #[path = "cases.rs"]
    mod cases;

    native_cases!(
        cpu_writes_make_clocked_stereo,
        noise_and_sound_reset_ownership,
        drain_rate_clone_and_reset,
        actual_sound_cpu_pcm,
        sample_controls_keep_hardware_running,
    );
}

mod opn2_machine {
    #[path = "cases.rs"]
    mod cases;
    #[path = "csm_level.rs"]
    mod csm_level;
    #[path = "csm_timer.rs"]
    mod csm_timer;
    #[path = "timer_b.rs"]
    mod timer_b;

    #[test]
    fn timer_b_polling_retains_stopped_phase_and_saved_future() {
        timer_b::timer_b_polling_retains_stopped_phase_and_saved_future();
    }

    #[test]
    fn timer_a_csm_keys_transfer_level_without_flag_enable() {
        csm_timer::timer_a_csm_keys_transfer_level_without_flag_enable();
    }

    #[test]
    fn csm_mode_omits_ordinary_total_level_and_retains_am() {
        csm_level::csm_mode_omits_ordinary_total_level_and_retains_am();
    }

    native_cases!(
        channel_three_modes_share_special_frequency_ownership,
        main_cpu_dac_stereo_and_status,
        uploaded_z80_fm_timer_and_restore,
        live_sustain_target_must_match_the_decay_level_window,
        zero_level_rekey_uses_decay_while_nonzero_zero_attack_holds,
    );
}
