use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_sound_trace_paired_fork_restores_pending_output_and_replays_without_child_rows() {
    let mut actual = machine(Region::NtscUs, &[0x60fe, 0x4e71], TONE);
    let mut continuation = SoundTraceContinuation::default();
    let mut output = store();
    for _ in 0..3 {
        actual
            .step_instruction_sound_traced(&mut continuation.attach(&mut output))
            .unwrap();
    }
    assert!(actual.sound_deadline().is_some());
    let control = actual.clone();
    let before = rows(&output);
    let gaps = continuation.gaps();
    let fork = continuation.fork(&mut actual, &mut output);
    for _ in 0..32 {
        actual
            .step_instruction_sound_traced(&mut continuation.attach(&mut output))
            .unwrap();
    }
    assert!(output.len() > before.len());
    fork.restore(&mut actual, &mut continuation, &mut output);
    exact(&actual, &control);
    assert_eq!(rows(&output), before);
    assert_eq!(continuation.gaps(), gaps);
    let mut control = control;
    for _ in 0..32 {
        actual
            .step_instruction_sound_traced(&mut continuation.attach(&mut output))
            .unwrap();
        control.step_instruction().unwrap();
    }
    exact(&actual, &control);
    assert_eq!(continuation.gaps(), gaps);
}
