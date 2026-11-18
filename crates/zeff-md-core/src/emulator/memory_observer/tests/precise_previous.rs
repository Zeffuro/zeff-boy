use super::machine::{PROFILES, fixture, step};
use super::*;
use crate::emulator::{MachineFault, Region, TimingModel};

#[derive(Default)]
struct Accesses {
    memory: Vec<MemoryAccess>,
    writes: usize,
}

impl InstructionObserver for Accesses {
    fn memory_access(&mut self, access: MemoryAccess) {
        self.memory.push(access);
    }
    fn write(&mut self, _: BusAccess, _: u16) {
        self.writes += 1;
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_ram_tas_stays_unsupported_before_any_write_or_memory_callback() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut actual = fixture(
            &[0x13fc, 0x25, 0xff, 0x41, 0x4ad0, 0x4e71],
            region,
            TimingModel::Precise,
            true,
        );
        actual.cpu.registers.a[0] = 0xff0041;
        actual.step_instruction().unwrap();
        let mut expected = actual.clone();
        let mut capture = Accesses::default();
        let failure = actual.step_instruction_traced(&mut capture);
        assert!(matches!(
            failure,
            Err(MachineFault::Main(
                TimingFault::RmwOwnershipUnsupported { .. }
            ))
        ));
        assert_eq!(failure, expected.step_instruction());
        assert!(capture.memory.is_empty() && capture.writes == 0);
        assert_eq!(actual.peek(0xff0041), Some(0x25));
        assert_eq!(actual, expected);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_long_wrap_reports_only_the_ram_half_and_preserves_the_other_owner() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut actual = fixture(
            &[
                0x33fc, 0xaa55, 0xff, 0xfffe, 0x23fc, 0x1234, 0x5678, 0xff, 0xfffe, 0x4e71,
            ],
            region,
            TimingModel::Precise,
            true,
        );
        actual.step_instruction().unwrap();
        let mut expected = actual.clone();
        let events = step(&mut actual, &mut expected);
        assert_eq!(
            events
                .iter()
                .map(|e| (e.address, e.old_value, e.value))
                .collect::<Vec<_>>(),
            [
                (0xfffffe, Some(0xaa), Some(0x12)),
                (0xffffff, Some(0x55), Some(0x34)),
                (0, None, Some(0x56)),
                (1, None, Some(0x78))
            ]
        );
        assert_eq!(actual.peek(0), Some(0));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn previous_values_survive_raw_and_frontend_restore_with_exact_future_nonzero_pcm() {
    let program = [
        0x13fc, 0x85, 0xc0, 0x11, 0x13fc, 6, 0xc0, 0x11, 0x13fc, 0x90, 0xc0, 0x11, 0x23fc, 0x192a,
        0x3b4c, 0xff, 0x40, 0x23fc, 0x5566, 0x7788, 0xff, 0x40, 0x4e72, 0x2700,
    ];
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for (timing, concurrent) in PROFILES {
            let mut source = fixture(&program, region, timing, concurrent);
            source.set_sample_rate(44_100);
            for _ in 0..4 {
                source.step_instruction().unwrap();
            }
            let raw = source.encode_state().unwrap();
            let front = source.encode_frontend_state().unwrap();
            for frontend in [false, true] {
                let mut actual = source.clone();
                let mut expected = source.clone();
                if frontend {
                    actual.decode_frontend_state(&front).unwrap();
                } else {
                    actual.decode_state(&raw).unwrap();
                    // RAW restoration retains the existing frontend-output restart policy.
                    expected.set_sample_rate(expected.sample_rate());
                }
                assert_eq!(actual, expected);
                let events = step(&mut actual, &mut expected);
                assert_eq!(
                    events
                        .iter()
                        .map(|e| (e.address, e.old_value, e.value))
                        .collect::<Vec<_>>(),
                    [
                        (0xff0040, Some(0x19), Some(0x55)),
                        (0xff0041, Some(0x2a), Some(0x66)),
                        (0xff0042, Some(0x3b), Some(0x77)),
                        (0xff0043, Some(0x4c), Some(0x88))
                    ]
                );
                step(&mut actual, &mut expected);
                actual.step_field().unwrap();
                expected.step_field().unwrap();
                assert_eq!(actual, expected);
                assert_eq!(
                    actual.encode_state().unwrap(),
                    expected.encode_state().unwrap()
                );
                assert_eq!(
                    actual.encode_frontend_state().unwrap(),
                    expected.encode_frontend_state().unwrap()
                );
                assert_eq!(actual.framebuffer(), expected.framebuffer());
                let pcm = actual.drain_audio_samples();
                assert!(pcm.iter().any(|sample| *sample != 0.0));
                assert!(
                    pcm.iter().map(|sample| sample.to_bits()).eq(expected
                        .drain_audio_samples()
                        .iter()
                        .map(|sample| sample.to_bits()))
                );
            }
        }
    }
}
