use super::frame_direct::{assert_cpu_equal, assert_projected_equal};
use super::frame_mixed::{CODE_BASES, fixture, oracle_step};
use super::*;

fn multiply(condition: u32, accumulate: bool, set_flags: bool, registers: [u32; 4]) -> u32 {
    let [rd, rn, rs, rm] = registers;
    (condition << 28)
        | (u32::from(accumulate) << 21)
        | (u32::from(set_flags) << 20)
        | (rd << 16)
        | (rn << 12)
        | (rs << 8)
        | 0x90
        | rm
}

#[test]
fn frame_multiply_matches_all_conditions_flags_and_control_bits() {
    for flags in 0..16u32 {
        for condition in 0..16u32 {
            for accumulate in [false, true] {
                for set_flags in [false, true] {
                    let raw = multiply(condition, accumulate, set_flags, [4, 3, 2, 1]);
                    let mut phase = fixture(0x0300_0200, &[raw]);
                    phase.cpu.cpsr = (phase.cpu.cpsr & 0x0FFF_FFFF) | (flags << 28);
                    phase.cpu.regs[1] = 0x8000_0001;
                    phase.cpu.regs[2] = 0x7FFF_FFFD;
                    phase.cpu.regs[3] = 0xA55A_3CC3;
                    let mut direct = phase.clone();
                    direct.bus.begin_frame_service();
                    assert!(
                        oracle_step(&mut phase, &mut direct),
                        "flags={flags:X} condition={condition:X} A={accumulate} S={set_flags}"
                    );
                }
            }
        }
    }
}

#[test]
fn frame_multiply_preserves_aliasing_values_waitcnt_and_fetch_regions() {
    let aliases = [
        (4, 3, 2, 1),
        (1, 3, 2, 1),
        (2, 3, 2, 1),
        (3, 3, 2, 1),
        (4, 3, 1, 1),
        (4, 1, 2, 1),
        (4, 2, 2, 1),
        (1, 1, 1, 1),
    ];
    let values = [
        0,
        1,
        u32::MAX,
        0x8000_0000,
        0x7FFF_FFFF,
        0xFFFF_0001,
        0x00FF_FFFF,
        0xA55A_3CC3,
    ];
    for (rd, rn, rs, rm) in aliases {
        for seed in values {
            for accumulate in [false, true] {
                for set_flags in [false, true] {
                    let raw = multiply(14, accumulate, set_flags, [rd, rn, rs, rm]);
                    let mut phase = fixture(0x0300_0200, &[raw]);
                    for register in 0..15 {
                        phase.cpu.regs[register] = seed
                            .rotate_left(register as u32)
                            .wrapping_add((register as u32).wrapping_mul(0x1020_4081));
                    }
                    let mut direct = phase.clone();
                    direct.bus.begin_frame_service();
                    assert!(oracle_step(&mut phase, &mut direct));
                }
            }
        }
    }

    for base in CODE_BASES {
        for waitcnt in [0, 0x0417, 0x03FF] {
            let raw = multiply(14, true, true, [4, 3, 2, 1]);
            let mut phase = fixture(base, &[raw]);
            phase.bus.write16(0x0400_0204, waitcnt);
            let mut direct = phase.clone();
            direct.bus.begin_frame_service();
            assert!(
                oracle_step(&mut phase, &mut direct),
                "{base:08X} {waitcnt:04X}"
            );
        }
    }
}

#[test]
fn frame_multiply_rejects_passed_r15_and_multiply_long_before_mutation() {
    for accumulate in [false, true] {
        for field in 0..4 {
            let mut registers = [4, 3, 2, 1];
            registers[field] = 15;
            let raw = multiply(14, accumulate, true, registers);
            let mut phase = fixture(0x0300_0200, &[raw]);
            let mut direct = phase.clone();
            direct.bus.begin_frame_service();
            assert!(
                direct
                    .cpu
                    .run_frame_direct(&mut direct.bus, u64::MAX, true, true)
                    .is_none()
            );
            assert_cpu_equal(&phase.cpu, &direct.cpu);
            assert_eq!(direct.bus.frame_service_stats_for_test().1, 0);
            assert!(!oracle_step(&mut phase, &mut direct));

            let failed_raw = raw & 0x0FFF_FFFF;
            let mut phase = fixture(0x0300_0200, &[failed_raw]);
            phase.cpu.cpsr &= !CPSR_ZERO;
            let mut direct = phase.clone();
            direct.bus.begin_frame_service();
            assert!(oracle_step(&mut phase, &mut direct));
        }
    }

    let mut phase = fixture(0x0300_0200, &[0xE0C0_2091]);
    let mut direct = phase.clone();
    direct.bus.begin_frame_service();
    assert!(
        direct
            .cpu
            .run_frame_direct(&mut direct.bus, u64::MAX, true, true)
            .is_none()
    );
    assert_cpu_equal(&phase.cpu, &direct.cpu);
    assert!(!oracle_step(&mut phase, &mut direct));
}

#[test]
fn frame_multiply_test_selector_only_disables_passed_short_multiply() {
    let passed = multiply(14, true, true, [4, 3, 2, 1]);
    let mut baseline = fixture(0x0300_0200, &[passed, 0xE281_1001]);
    baseline.bus.begin_frame_service();
    let before = baseline.cpu.clone();
    assert!(
        baseline
            .cpu
            .run_frame_direct(&mut baseline.bus, u64::MAX, false, true)
            .is_none()
    );
    assert_cpu_equal(&before, &baseline.cpu);
    baseline.cpu.step(&mut baseline.bus).unwrap();
    assert!(
        baseline
            .cpu
            .run_frame_direct(&mut baseline.bus, u64::MAX, false, true)
            .is_some()
    );

    let failed = multiply(0, true, true, [4, 3, 2, 1]);
    let mut baseline = fixture(0x0300_0200, &[failed]);
    baseline.cpu.cpsr &= !CPSR_ZERO;
    baseline.bus.begin_frame_service();
    assert!(
        baseline
            .cpu
            .run_frame_direct(&mut baseline.bus, u64::MAX, false, true)
            .is_some()
    );
}

#[test]
fn frame_multiply_fuses_mixed_loops_with_live_waitcnt() {
    let words = [
        multiply(14, false, true, [4, 0, 2, 1]),
        0xE281_1001,
        multiply(14, true, false, [5, 3, 2, 1]),
        0x1AFF_FFFB,
    ];
    for base in CODE_BASES {
        for waitcnt in [0, 0x0417, 0x03FF] {
            let mut phase = fixture(base, &words);
            phase.cpu.regs[1] = 3;
            phase.cpu.regs[2] = 5;
            phase.cpu.regs[3] = 7;
            phase.bus.write16(0x0400_0204, waitcnt);
            let mut direct = phase.clone();
            phase.bus.begin_frame_service();
            direct.bus.begin_frame_service();
            let (last, count) = direct
                .cpu
                .run_frame_direct(&mut direct.bus, u64::MAX, true, true)
                .unwrap();
            assert!(count >= 8, "{base:08X} {waitcnt:04X}: {count}");
            let mut expected = None;
            for _ in 0..count {
                expected = phase.cpu.step(&mut phase.bus);
            }
            assert_eq!(expected, Some(last));
            assert_eq!(direct.bus.frame_service_stats_for_test().2, 1);
            phase.bus.end_frame_service();
            assert_projected_equal(&phase, &direct);
        }
    }
}

#[test]
fn frame_multiply_horizon_and_guard_cutoffs_are_strict_and_nonmutating() {
    for base in [0x0300_0200, 0x0800_0200, 0x0C00_0200] {
        for raw in [
            multiply(14, false, false, [4, 0, 2, 1]),
            multiply(14, true, true, [4, 3, 2, 1]),
        ] {
            let mut saw_equal_horizon = false;
            for cut in 1..40u16 {
                let mut phase = fixture(base, &[raw]);
                phase.bus.write16(0x0400_0100, 0u16.wrapping_sub(cut));
                phase.bus.write16(0x0400_0102, 0xC0);
                let before = phase.cpu.clone();
                let mut direct = phase.clone();
                direct.bus.begin_frame_service();
                let budget = direct.bus.frame_cpu_cycle_budget();
                let expected = phase.cpu.step(&mut phase.bus).unwrap();
                let cycles = (phase.cpu.cycles - before.cycles) as u32;
                saw_equal_horizon |= cycles == budget;
                let result =
                    direct
                        .cpu
                        .run_frame_direct(&mut direct.bus, phase.cpu.cycles, true, true);
                assert_eq!(result.is_some(), cycles <= budget);
                let actual = if let Some((fetched, count)) = result {
                    assert_eq!(count, 1);
                    fetched
                } else {
                    assert_cpu_equal(&before, &direct.cpu);
                    direct.cpu.step(&mut direct.bus).unwrap()
                };
                assert_eq!(actual, expected);
                assert_projected_equal(&phase, &direct);
            }
            assert!(saw_equal_horizon, "{base:08X} {raw:08X}");

            let mut phase = fixture(base, &[raw]);
            let before = phase.cpu.clone();
            let mut exact = phase.clone();
            let expected = phase.cpu.step(&mut phase.bus).unwrap();
            let cycles = phase.cpu.cycles - before.cycles;
            exact.bus.begin_frame_service();
            let (actual, count) = exact
                .cpu
                .run_frame_direct(&mut exact.bus, before.cycles + cycles, true, true)
                .unwrap();
            assert_eq!((actual, count), (expected, 1));
            assert_projected_equal(&phase, &exact);

            let mut short = fixture(base, &[raw]);
            let before = short.cpu.clone();
            short.bus.begin_frame_service();
            assert!(
                short
                    .cpu
                    .run_frame_direct(&mut short.bus, before.cycles + cycles - 1, true, true)
                    .is_none()
            );
            assert_cpu_equal(&before, &short.cpu);
        }
    }
}
