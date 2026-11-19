use super::*;
use crate::{LoadConfig, Region};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn instrumented_build_refuses_rollback_without_changing_machine() {
    let mut rom = vec![0; 0x400];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x200..0x204].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
    let mut core = Emulator::new(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: true,
        },
    )
    .unwrap();
    core.set_sample_rate(48_000);
    let before = core.clone();
    let error = MdRollbackConfig::capture(&core).unwrap_err();
    assert!(error.to_string().contains("instrumented builds"));
    assert_eq!(core, before);
    assert_eq!(core.encode_state().unwrap(), before.encode_state().unwrap());
    assert_eq!(
        core.cpu.queued_instruction_inputs(),
        before.cpu.queued_instruction_inputs()
    );
    assert_eq!(
        core.clone().drain_audio_samples(),
        before.clone().drain_audio_samples()
    );
}
