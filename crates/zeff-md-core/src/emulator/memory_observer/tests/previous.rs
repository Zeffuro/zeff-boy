use super::machine::{PROFILES, fixture, step};
use super::*;
use crate::emulator::{Region, TimingModel};

type WriteSample = (u32, Option<u8>, Option<u8>);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn plain_previous_receipts_preserve_the_same_real_cycle_and_interrupt_samples() {
    for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
        for observed in [false, true] {
            let mut actual = fixture(&[0x4e71; 4], Region::NtscUs, TimingModel::Standard, false);
            actual.bus.ram[0x40..0x42].copy_from_slice(&[0x19, 0x2a]);
            let mut expected = actual.clone();
            let access = BusAccess {
                logical_address: 0xff0040 + u32::from(lanes == ByteLanes::Lower),
                bus_address: 0xff0040,
                kind: AccessKind::Data,
                lanes,
                function_code: FunctionCode::SupervisorData,
            };
            let start = actual.bus.cpu_clock;
            let ordinary = expected.bus.write_cycle_observed(access, 0xa55a, start);
            let (cycle, previous) = if observed {
                let (receipt, previous) = actual
                    .bus
                    .write_cycle_observed_with_previous(access, 0xa55a, start);
                assert_eq!(receipt.samples, ordinary.samples);
                (receipt.cycle, previous)
            } else {
                actual.bus.write_cycle_with_previous(access, 0xa55a, start)
            };
            assert_eq!(cycle, ordinary.cycle);
            assert_eq!(
                previous,
                match lanes {
                    ByteLanes::Upper => [Some(0x19), None],
                    ByteLanes::Lower => [None, Some(0x2a)],
                    ByteLanes::Both => [Some(0x19), Some(0x2a)],
                }
            );
            assert_eq!(actual, expected);
            let before = actual.clone();
            let (failed, previous) = actual.bus.write_cycle_with_previous(access, 0xffff, start);
            assert!(failed.result.is_err());
            assert_eq!(failed.wait_clocks, 0);
            assert_eq!(previous, [None, None]);
            assert_eq!(actual, before);
        }
    }
}

fn values(events: &[MemoryAccess]) -> Vec<WriteSample> {
    events
        .iter()
        .map(|event| (event.address, event.old_value, event.value))
        .collect()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn previous_ram_bytes_cover_lanes_mirrors_repeats_long_and_stack_in_all_profiles() {
    let program = [
        0x13fc, 0x91, 0xe0, 0x40, 0x13fc, 0x82, 0xff, 0x41, 0x33fc, 0x7364, 0xfe, 0x40, 0x33fc,
        0x7364, 0xff, 0x40, 0x23fc, 0x1234, 0x5678, 0xff, 0x40, 0x2100, 0x6104, 0x60fe, 0x4e71,
        0x4e75, 0x4e71,
    ];
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for (timing, concurrent) in PROFILES {
            let mut actual = fixture(&program, region, timing, concurrent);
            actual.cpu.registers.d[0] = 0xa1b2c3d4;
            actual.cpu.registers.a[0] = 0xff0044;
            actual.bus.ram[0x40..0x44].copy_from_slice(&[0x19, 0x2a, 0x3b, 0x4c]);
            actual.bus.ram[0xfff8..0xfffc].copy_from_slice(&[0x5d, 0x6e, 0x7f, 0x80]);
            let mut expected = actual.clone();
            let expected_steps: &[&[WriteSample]] = &[
                &[(0xe00040, Some(0x19), Some(0x91))],
                &[(0xff0041, Some(0x2a), Some(0x82))],
                &[
                    (0xfe0040, Some(0x91), Some(0x73)),
                    (0xfe0041, Some(0x82), Some(0x64)),
                ],
                &[
                    (0xff0040, Some(0x73), Some(0x73)),
                    (0xff0041, Some(0x64), Some(0x64)),
                ],
                &[
                    (0xff0040, Some(0x73), Some(0x12)),
                    (0xff0041, Some(0x64), Some(0x34)),
                    (0xff0042, Some(0x3b), Some(0x56)),
                    (0xff0043, Some(0x4c), Some(0x78)),
                ],
                &[
                    (0xff0042, Some(0x56), Some(0xc3)),
                    (0xff0043, Some(0x78), Some(0xd4)),
                    (0xff0040, Some(0x12), Some(0xa1)),
                    (0xff0041, Some(0x34), Some(0xb2)),
                ],
                &[
                    (0xfffff8, Some(0x5d), Some(0)),
                    (0xfffff9, Some(0x6e), Some(0)),
                    (0xfffffa, Some(0x7f), Some(2)),
                    (0xfffffb, Some(0x80), Some(0x2e)),
                ],
            ];
            for wanted in expected_steps {
                let events = step(&mut actual, &mut expected);
                let wanted = wanted.to_vec();
                assert_eq!(
                    values(&events),
                    wanted,
                    "{region:?}/{timing:?}/{concurrent}"
                );
                assert!(
                    events
                        .iter()
                        .all(|event| event.direction == Direction::Write)
                );
            }
            let returned = step(&mut actual, &mut expected);
            assert_eq!(returned.len(), 4);
            assert!(
                returned
                    .iter()
                    .all(|event| event.direction == Direction::Read && event.old_value.is_none())
            );
            assert_eq!(actual.registers().pc, 0x22e);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn ignored_rom_tas_wrap_and_fault_prefix_keep_storage_claims_honest() {
    for concurrent in [false, true] {
        let timing = TimingModel::Standard;
        let mut actual = fixture(
            &[
                0x33fc, 0x1234, 0, 0x380, 0x4ad0, 0x23fc, 0x5678, 0x9abc, 0xff, 0xfffe, 0x4e71,
            ],
            Region::NtscUs,
            timing,
            concurrent,
        );
        actual.cpu.registers.a[0] = 0xff0041;
        actual.bus.ram[0x41] = 0x25;
        actual.bus.ram[0xfffe..].copy_from_slice(&[0x36, 0x47]);
        let mut expected = actual.clone();
        assert_eq!(
            values(&step(&mut actual, &mut expected)),
            [(0x380, None, Some(0x12)), (0x381, None, Some(0x34))]
        );
        assert_eq!(
            values(&step(&mut actual, &mut expected)),
            [(0xff0041, None, Some(0x25)), (0xff0041, None, Some(0xa5))]
        );
        assert_eq!(actual.peek(0xff0041), Some(0x25));
        assert!(actual.bus.rmw.is_none());
        let events = step(&mut actual, &mut expected);
        assert_eq!(
            values(&events),
            [
                (0xfffffe, Some(0x36), Some(0x56)),
                (0xffffff, Some(0x47), Some(0x78)),
                (0, None, Some(0x9a)),
                (1, None, Some(0xbc))
            ]
        );

        let mut actual = fixture(
            &[0x23fc, 0x1234, 0x5678, 0x7f, 0xfffe, 0x4e71],
            Region::NtscUs,
            timing,
            concurrent,
        );
        let mut expected = actual.clone();
        let mut events = Vec::new();
        let failure = actual.step_instruction_observed(&mut |event| events.push(event));
        assert!(failure.is_err());
        assert_eq!(failure, expected.step_instruction());
        assert_eq!(actual, expected);
        assert_eq!(
            values(&events),
            [(0x7ffffe, None, Some(0x12)), (0x7fffff, None, Some(0x34))]
        );
        assert!(actual.encode_state().is_err() && expected.encode_state().is_err());
        assert_eq!(actual.step_instruction(), expected.step_instruction());
        assert_eq!(actual, expected);
    }
}
