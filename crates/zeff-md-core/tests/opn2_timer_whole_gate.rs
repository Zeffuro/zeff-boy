#[allow(dead_code)]
#[path = "opn2_machine/cases.rs"]
mod cases;
#[path = "opn2_machine/csm_timer.rs"]
mod csm_timer;
#[path = "opn2_machine/timer_b.rs"]
mod timer_b;
#[path = "opn2_machine/whole_witness.rs"]
mod whole_witness;

macro_rules! dual {
    ($name:ident, $body:path) => {
        #[cfg_attr(not(target_arch = "wasm32"), test)]
        #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
        fn $name() {
            $body();
            let line = concat!("OPN2_MACHINE_BODY_PASS ", stringify!($name));
            #[cfg(not(target_arch = "wasm32"))]
            println!("{line}");
            #[cfg(target_arch = "wasm32")]
            wasm_bindgen_test::console_log!("{}", line);
        }
    };
}

dual!(
    main_cpu_dac_stereo_and_status,
    cases::main_cpu_dac_stereo_and_status
);
dual!(
    uploaded_z80_fm_timer_and_restore,
    cases::uploaded_z80_fm_timer_and_restore
);
dual!(
    timer_a_csm_keys_transfer_level_without_flag_enable,
    csm_timer::timer_a_csm_keys_transfer_level_without_flag_enable
);
dual!(
    timer_b_polling_retains_stopped_phase_and_saved_future,
    timer_b::timer_b_polling_retains_stopped_phase_and_saved_future
);
dual!(
    raw_frontend_active_csm_manual_reload_clear_future_witness,
    whole_witness::raw_frontend_active_csm_manual_reload_clear_future_witness
);
