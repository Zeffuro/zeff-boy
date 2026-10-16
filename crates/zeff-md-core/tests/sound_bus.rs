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

    #[test]
    fn shared_value_and_boundary_continuation() {
        cases::shared_value_and_boundary_continuation();
    }
    #[test]
    fn main_sound_ram_lanes_and_mirrors() {
        cases::main_sound_ram_lanes_and_mirrors();
    }
    #[test]
    fn serial_bank_and_cartridge_requester() {
        cases::serial_bank_and_cartridge_requester();
    }
    #[test]
    fn reset_and_halt_ownership() {
        cases::reset_and_halt_ownership();
    }
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
    #[test]
    fn sound_control_lanes_and_busy_bit() {
        cases::sound_control_lanes_and_busy_bit();
    }
}

mod sound_control {
    #[path = "cases.rs"]
    mod cases;
    use crate::sound_fixture as fixture;
    #[path = "lifecycle.rs"]
    mod lifecycle;

    #[test]
    fn control_mirror_request_and_reset() {
        cases::control_mirror_request_and_reset();
    }

    #[test]
    fn control_mirror_lanes_and_bits() {
        cases::control_mirror_lanes_and_bits();
    }

    #[test]
    fn control_mirror_busy_reads() {
        cases::control_mirror_busy_reads();
    }

    #[test]
    fn control_mirror_adjacent_blocks() {
        cases::control_mirror_adjacent_blocks();
    }

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
    #[test]
    fn disconnected_ports_consume_clocks() {
        cases::disconnected_ports_consume_clocks();
    }
    #[test]
    fn unused_sound_memory_and_main_lanes() {
        cases::unused_sound_memory_and_main_lanes();
    }
    #[test]
    fn unsupported_devices_remain_faults() {
        cases::unsupported_devices_remain_faults();
    }
}

mod concurrent {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn delayed_write_and_pending_clone() {
        cases::delayed_write_and_pending_clone();
    }
    #[test]
    fn requested_read_waits_once() {
        cases::requested_read_waits_once();
    }
    #[test]
    fn reset_discards_unpublished_store() {
        cases::reset_discards_unpublished_store();
    }
    #[test]
    fn main_reset_preserves_pending_time() {
        cases::main_reset_preserves_pending_time();
    }
    #[test]
    fn unsupported_sound_latches_after_prior_progress() {
        cases::unsupported_sound_latches_after_prior_progress();
    }
    #[test]
    fn halted_sound_ownership() {
        cases::halted_sound_ownership();
    }
}

mod prefix {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn cb_register_and_memory_operations() {
        cases::cb_register_and_memory_operations();
    }
    #[test]
    fn ed_block_iterations_and_wraparound() {
        cases::ed_block_iterations_and_wraparound();
    }
    #[test]
    fn ed_faults_are_transactional() {
        cases::ed_faults_are_transactional();
    }
    #[test]
    fn concurrent_ed_handover_and_pending_clone() {
        cases::concurrent_ed_handover_and_pending_clone();
    }
    #[test]
    fn concurrent_cb_memory_is_delayed() {
        cases::concurrent_cb_memory_is_delayed();
    }
    #[test]
    fn extended_fetch_and_bank_guards() {
        cases::extended_fetch_and_bank_guards();
    }
}

mod transfer {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn immediate_operand_changes_after_grant() {
        cases::immediate_operand_changes_after_grant();
    }
    #[test]
    fn memory_read_and_write_resume_after_grant() {
        cases::memory_read_and_write_resume_after_grant();
    }
    #[test]
    fn transfer_fault_preserves_m1() {
        cases::transfer_fault_preserves_m1();
    }
    #[test]
    fn transfer_reset_ownership() {
        cases::transfer_reset_ownership();
    }
}

mod multi_transfer {
    use crate::multi_transfer_cases as cases;

    #[test]
    fn immediate_low_latched_high_live() {
        cases::immediate_low_latched_high_live();
    }
    #[test]
    fn address_and_partial_write_survive_grant() {
        cases::address_and_partial_write_survive_grant();
    }
    #[test]
    fn partial_write_fault_and_reset() {
        cases::partial_write_fault_and_reset();
    }
    #[test]
    fn partial_read_latched_low_live_high() {
        cases::partial_read_latched_low_live_high();
    }
}

mod stack {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn push_grant_preserves_completed_high_write() {
        cases::push_grant_preserves_completed_high_write();
    }

    #[test]
    fn pop_grant_keeps_low_and_reads_high_live() {
        cases::pop_grant_keeps_low_and_reads_high_live();
    }

    #[test]
    fn exchange_grant_preserves_latched_pair_and_write_order() {
        cases::exchange_grant_preserves_latched_pair_and_write_order();
    }

    #[test]
    fn exchange_reset_and_second_write_fault() {
        cases::exchange_reset_and_second_write_fault();
    }

    #[test]
    fn push_and_pop_second_access_faults_keep_progress() {
        cases::push_and_pop_second_access_faults_keep_progress();
    }
}

mod control_stack {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn call_reads_high_operand_after_grant() {
        cases::call_reads_high_operand_after_grant();
    }

    #[test]
    fn call_keeps_target_and_completed_write() {
        cases::call_keeps_target_and_completed_write();
    }

    #[test]
    fn return_keeps_low_and_reads_high_live() {
        cases::return_keeps_low_and_reads_high_live();
    }

    #[test]
    fn restart_keeps_completed_write_and_return_address() {
        cases::restart_keeps_completed_write_and_return_address();
    }

    #[test]
    fn control_faults_and_reset_preserve_progress() {
        cases::control_faults_and_reset_preserve_progress();
    }
}

mod rmw {
    #[path = "cases.rs"]
    mod cases;
    #[test]
    fn rmw_grant_keeps_read_result() {
        cases::rmw_grant_keeps_read_result();
    }
    #[test]
    fn rmw_reset_discards_write() {
        cases::rmw_reset_discards_write();
    }
    #[test]
    fn rmw_write_fault_keeps_read_phase() {
        cases::rmw_write_fault_keeps_read_phase();
    }
}

mod psg {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn register_decoder() {
        cases::register_decoder();
    }
    #[test]
    fn reloads_preserve_countdowns() {
        cases::reloads_preserve_countdowns();
    }
    #[test]
    fn noise_sequences_and_rates() {
        cases::noise_sequences_and_rates();
    }
    #[test]
    fn digital_clock_and_pcm_policy() {
        cases::digital_clock_and_pcm_policy();
    }
    #[test]
    fn jump_and_partitioned_clocks() {
        cases::jump_and_partitioned_clocks();
    }
    #[test]
    fn attenuation_and_mix() {
        cases::attenuation_and_mix();
    }
}

mod psg_machine {
    #[path = "cases.rs"]
    mod cases;

    #[test]
    fn cpu_writes_make_clocked_stereo() {
        cases::cpu_writes_make_clocked_stereo();
    }
    #[test]
    fn noise_and_sound_reset_ownership() {
        cases::noise_and_sound_reset_ownership();
    }
    #[test]
    fn drain_rate_clone_and_reset() {
        cases::drain_rate_clone_and_reset();
    }
    #[test]
    fn actual_sound_cpu_pcm() {
        cases::actual_sound_cpu_pcm();
    }
    #[test]
    fn sample_controls_keep_hardware_running() {
        cases::sample_controls_keep_hardware_running();
    }
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

    #[test]
    fn channel_three_modes_share_special_frequency_ownership() {
        cases::channel_three_modes_share_special_frequency_ownership();
    }

    #[test]
    fn main_cpu_dac_stereo_and_status() {
        cases::main_cpu_dac_stereo_and_status();
    }

    #[test]
    fn uploaded_z80_fm_timer_and_restore() {
        cases::uploaded_z80_fm_timer_and_restore();
    }

    #[test]
    fn live_sustain_target_must_match_the_decay_level_window() {
        cases::live_sustain_target_must_match_the_decay_level_window();
    }

    #[test]
    fn zero_level_rekey_uses_decay_while_nonzero_zero_attack_holds() {
        cases::zero_level_rekey_uses_decay_while_nonzero_zero_attack_holds();
    }
}
