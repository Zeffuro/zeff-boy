use super::probe::{Report, Stop, run};
use sha2::{Digest, Sha256};
use zeff_md_core::{Emulator, LoadConfig, Region};

fn fixture(concurrent_sound: bool, fault: bool) -> Emulator {
    let mut rom = vec![0; 1024];
    rom[..4].copy_from_slice(&0xfffffc_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    let words: &[u16] = if fault {
        &[0x4e71, 0x1039, 0x0040, 0x0000, 0x60fe]
    } else {
        &[0x4e71, 0x4e71, 0x4e71, 0x60f8]
    };
    for (index, word) in words.iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut machine = Emulator::new(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound,
        },
    )
    .unwrap();
    machine.set_sample_rate(48000);
    machine.set_apu_sample_generation_enabled(true);
    machine
}

fn hash_audio(samples: &[f32]) -> [u8; 32] {
    let mut hash = Sha256::new();
    for sample in samples {
        hash.update(sample.to_bits().to_le_bytes());
    }
    hash.finalize().into()
}

fn record(hash: &mut Sha256, machine: &Emulator, report: &Report) {
    hash.update(machine.encode_state().unwrap());
    hash.update(machine.framebuffer());
    hash.update(format!("{report:?}"));
}

fn emit(label: &str, hash: Sha256) {
    let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("{label} {digest}");
    #[cfg(not(target_arch = "wasm32"))]
    println!("{label} {digest}");
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn probe_stop_limit_and_fault_contract() {
    let mut digest = Sha256::new();
    for concurrent in [false, true] {
        for (bound, target, completed, stop) in [
            (0, 0x200, 0, Stop::Reached),
            (0, 0x206, 0, Stop::Limit),
            (2, 0x206, 2, Stop::Limit),
            (3, 0x206, 3, Stop::Reached),
            (10, 0x206, 3, Stop::Reached),
        ] {
            let initial = fixture(concurrent, false);
            let state = initial.encode_state().unwrap();
            let mut observed = fixture(concurrent, false);
            observed.decode_state(&state).unwrap();
            assert_eq!(observed, initial);
            let mut control = initial;
            let mut audio = Vec::new();
            for _ in 0..completed {
                control.step_instruction().unwrap();
                control.drain_audio_samples_into(&mut audio);
            }
            let report = run(&mut observed, bound, |m| m.registers().pc == target);
            assert_eq!(report.stop, stop);
            assert_eq!(report.attempted, completed);
            assert_eq!(report.completed, completed);
            assert_eq!(report.drained_samples, audio.len() as u64);
            assert_eq!(report.audio_sha256, hash_audio(&audio));
            assert_eq!(observed, control);
            record(&mut digest, &observed, &report);
        }
        let mut observed = fixture(concurrent, true);
        let mut control = observed.clone();
        control.step_instruction().unwrap();
        let expected_fault = control.step_instruction().unwrap_err();
        let mut audio = Vec::new();
        control.drain_audio_samples_into(&mut audio);
        let report = run(&mut observed, 10, |m| m.registers().pc == 0x208);
        assert_eq!(report.stop, Stop::Fault(expected_fault));
        assert_eq!(report.attempted, 2);
        assert_eq!(report.completed, 1);
        assert_eq!(report.audio_sha256, hash_audio(&audio));
        assert_eq!(report.drained_samples, audio.len() as u64);
        assert_eq!(observed, control);
        assert_eq!(observed.registers().pc, 0x202);
        digest.update(format!("{report:?}"));
    }
    emit("boundary-probe-stop-v1", digest);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn probe_field_and_restored_continuation_contract() {
    let mut digest = Sha256::new();
    for concurrent in [false, true] {
        let mut control = fixture(concurrent, false);
        let mut observed = control.clone();
        for _ in 0..2 {
            let field = observed.pacing_field_count();
            control.step_field().unwrap();
            let mut audio = Vec::new();
            control.drain_audio_samples_into(&mut audio);
            let report = run(&mut observed, 100000, |m| m.pacing_field_count() != field);
            assert_eq!(report.stop, Stop::Reached);
            assert!(report.completed > 0 && report.completed < 100000);
            assert_eq!(report.attempted, report.completed);
            assert_eq!(report.audio_sha256, hash_audio(&audio));
            assert_eq!(report.drained_samples, audio.len() as u64);
            assert!(report.drained_samples > 0);
            assert_eq!(observed, control);
            record(&mut digest, &observed, &report);
            let saved = observed.encode_state().unwrap();
            let mut restored = fixture(concurrent, false);
            restored.decode_state(&saved).unwrap();
            control.decode_state(&saved).unwrap();
            assert_eq!(restored.encode_state().unwrap(), saved);
            assert_eq!(restored, control);
            observed = restored;
        }
    }
    emit("boundary-probe-field-v1", digest);
}
