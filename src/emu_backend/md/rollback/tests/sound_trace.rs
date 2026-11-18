use super::*;
use crate::debug::DebugUiActions;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_sound_owner_blocks_rollback_before_factory_and_stays_frozen_during_lease() {
    let mut md = backend(false);
    md.step_frame();
    let config = MdRollbackConfig::capture(md.core()).unwrap();
    let before = md.core().clone();
    let mut actions = DebugUiActions::none();
    actions.sound_trace.enabled = Some(true);
    md.apply_debug_actions(&actions);
    let active = md.sound_instruction_trace_stream();
    assert!(md.sound_instruction_trace().is_enabled());
    assert!(!md.sound_trace.idle());
    assert!(md.begin_rollback(&config).is_err());
    assert!(md.rollback_host_baseline().is_err());
    assert_eq!(md.sound_instruction_trace_stream(), active);
    same(md.core(), &before);
    actions.sound_trace.enabled = Some(false);
    actions.sound_trace.clear = true;
    md.apply_debug_actions(&actions);
    assert!(md.sound_trace.idle());
    let frozen = md.sound_trace.frozen().unwrap();
    let lease = md.begin_rollback(&config).unwrap();
    actions.sound_trace.enabled = Some(true);
    md.apply_debug_actions(&actions);
    assert_eq!(md.sound_trace.frozen().unwrap(), frozen);
    assert!(
        md.sound_trace_rejection()
            .unwrap()
            .contains("normal backend ownership")
    );
    md.retire_rollback(&lease).unwrap();
    md.release_rollback_checked(&lease, None).unwrap();
    assert!(!md.rollback_owned() && md.sound_trace.idle());
    assert!(md.sound_instruction_trace_stream() > active);
    same(md.core(), &before);
    actions.sound_trace.clear = false;
    md.apply_debug_actions(&actions);
    assert!(md.sound_instruction_trace().is_enabled());
    md.sound_trace.exhaust_for_test();
    let terminal = md.sound_instruction_trace_stream();
    assert!(!terminal.unwrap().is_live());
    assert!(!md.sound_instruction_trace().is_enabled());
    assert!(md.sound_trace_gap().unwrap().contains("exhausted"));
    md.apply_debug_actions(&actions);
    assert_eq!(md.sound_instruction_trace_stream(), terminal);
    assert!(!md.sound_instruction_trace().is_enabled());
    same(md.core(), &before);
    let mut control = before;
    md.step_frame();
    control.step_field().unwrap();
    same(md.core(), &control);
}
