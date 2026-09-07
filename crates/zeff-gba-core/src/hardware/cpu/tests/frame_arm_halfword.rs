use super::frame_direct::assert_projected_equal;
use super::frame_mixed::{fixture, oracle_step};
use super::*;

fn instruction(kind: u32, controls: u32, destination: u32) -> u32 {
    let (load, mode) = match kind {
        0 => (0, 1),
        1 => (1, 1),
        2 => (1, 2),
        _ => (1, 3),
    };
    0xE000_0093
        | (controls & 1) << 24
        | ((controls >> 1) & 1) << 23
        | ((controls >> 2) & 1) << 22
        | ((controls >> 3) & 1) << 21
        | load << 20
        | destination << 12
        | mode << 5
}

#[test]
fn frame_arm_halfword_matches_addressing_aliases_alignment_and_plain_regions() {
    for kind in 0..4 {
        for controls in 0..16 {
            for destination in [0, 1, 3] {
                let raw = instruction(kind, controls, destination);
                for data in [
                    0x0200_1080,
                    0x0204_1080,
                    0x0300_1080,
                    0x0300_9080,
                    0x0800_3080,
                    0x0A00_3080,
                    0x0C00_3080,
                ] {
                    for odd in 0..2 {
                        let mut phase = fixture(0x0800_0200, &[raw]);
                        phase.cpu.regs[0] = data + odd;
                        phase.cpu.regs[3] = 3;
                        phase.cpu.cpsr |= CPSR_CARRY | CPSR_OVERFLOW;
                        let mut direct = phase.clone();
                        direct.bus.begin_frame_service();
                        assert_eq!(
                            oracle_step(&mut phase, &mut direct),
                            kind != 0 || data < 0x0800_0000,
                            "{raw:08X} data {data:08X}+{odd}",
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn frame_arm_halfword_matches_conditions_and_pc_or_peripheral_fallbacks() {
    for raw in [
        instruction(0, 7, 15), // Store PC retains the scalar ARM PC value.
        instruction(1, 7, 1) | 15 << 16,
        instruction(2, 7, 1) | 15 << 16,
        instruction(3, 7, 1) | 15 << 16,
    ] {
        let mut phase = fixture(0x0800_0200, &[raw]);
        let mut direct = phase.clone();
        direct.bus.begin_frame_service();
        assert!(oracle_step(&mut phase, &mut direct));
    }
    for condition in 0..16 {
        for flags in 0..16 {
            for kind in 0..4 {
                let raw = (instruction(kind, 7, 1) & 0x0FFF_FFFF) | condition << 28;
                let mut phase = fixture(0x0300_0200, &[raw]);
                phase.cpu.cpsr = (phase.cpu.cpsr & 0x0FFF_FFFF) | flags << 28;
                let mut direct = phase.clone();
                direct.bus.begin_frame_service();
                assert!(oracle_step(&mut phase, &mut direct));
            }
        }
    }
    for raw in [
        instruction(1, 7, 15),
        instruction(2, 7, 15),
        instruction(3, 7, 15),
        instruction(1, 15, 1) | 15 << 16,
        instruction(2, 15, 1) | 15 << 16,
        instruction(3, 15, 1) | 15 << 16,
        0xE1C0_10D0, // Unsupported signed-byte store.
        0xE1C0_10F0, // Unsupported signed-halfword store.
    ] {
        let mut phase = fixture(0x0800_0200, &[raw]);
        let mut direct = phase.clone();
        direct.bus.begin_frame_service();
        assert!(!oracle_step(&mut phase, &mut direct), "{raw:08X}");
    }
    for kind in 0..4 {
        for address in [
            0x0000_0100,
            0x0400_0000,
            0x0500_0000,
            0x0600_0000,
            0x0700_0000,
        ] {
            let mut phase = fixture(0x0800_0200, &[instruction(kind, 7, 1)]);
            phase.cpu.regs[0] = address;
            let mut direct = phase.clone();
            direct.bus.begin_frame_service();
            assert!(!oracle_step(&mut phase, &mut direct));
        }
    }
}

#[test]
fn frame_arm_halfword_matches_base_offset_aliases_and_restored_transfer_phases() {
    for kind in 0..4 {
        for controls in 0..16 {
            for (base_register, destination, offset_register) in
                [(3, 1, 3), (3, 3, 3), (0, 1, 15), (0, 15, 3)]
            {
                let raw = (instruction(kind, controls, destination) & !15)
                    | base_register << 16
                    | offset_register;
                let mut phase = fixture(0x0300_0200, &[raw]);
                phase.cpu.regs[base_register as usize] = 0x0200_1081;
                let mut direct = phase.clone();
                direct.bus.begin_frame_service();
                oracle_step(&mut phase, &mut direct);
            }
        }
        for phases in 0..8 {
            let mut phase = fixture(0x0300_0200, &[instruction(kind, 15, 1)]);
            for _ in 0..phases {
                phase.cpu.step_cpu_phase_for_test(&mut phase.bus);
            }
            let saved = phase.encode_state().unwrap();
            phase.load_state(&saved).unwrap();
            let mut direct = phase.clone();
            direct.bus.begin_frame_service();
            for _ in 0..4 {
                oracle_step(&mut phase, &mut direct);
            }
        }
    }
}

#[test]
fn frame_arm_halfword_preserves_fetch_before_store_and_crossing_horizons() {
    for kind in 0..4 {
        for base in [0x0200_0200, 0x0300_0200, 0x0800_0200] {
            let raw = instruction(kind, 7, 1);
            for cutoff in [0, 1, 2, 3, 4, 5, 8, 16] {
                let mut phase = fixture(base, &[raw]);
                if base < 0x0800_0000 {
                    phase.cpu.regs[0] = base + 12 - 3;
                }
                let mut direct = phase.clone();
                direct.bus.begin_frame_service();
                let guard = direct.cpu.cycles + cutoff;
                if let Some((actual, count)) =
                    direct
                        .cpu
                        .run_frame_direct(&mut direct.bus, guard, true, true)
                {
                    let mut expected = None;
                    for _ in 0..count {
                        expected = phase.cpu.step(&mut phase.bus);
                    }
                    assert_eq!(Some(actual), expected);
                }
                assert_projected_equal(&phase, &direct);
            }
        }
    }
}
