use super::frame_direct::assert_projected_equal;
use super::frame_mixed::{CODE_BASES, fixture as arm_fixture, oracle_step};
use super::*;
use crate::emulator::Emulator;

fn fixture(base: u32, raw: u16) -> Emulator {
    let mut emu = arm_fixture(base, &[u32::from(raw) << 16 | 0x2000, 0x2000_2000]);
    emu.cpu.cpsr |= CPSR_THUMB;
    emu.cpu.set_pc(base + 4);
    emu.cpu.step(&mut emu.bus).unwrap();
    emu.cpu.regs[13] = 0x0200_1080;
    emu
}

#[test]
fn frame_thumb_sp_matches_registers_offsets_alignment_and_plain_regions() {
    for load in [false, true] {
        for rd in 0..8 {
            for offset in [0, 1, 127, 255] {
                let raw = 0x9000 | u16::from(load) << 11 | rd << 8 | offset;
                for address in [
                    0x0200_1080u32,
                    0x0204_1080,
                    0x0300_1080,
                    0x0300_9080,
                    0x0800_3080,
                    0x0A00_3080,
                    0x0C00_3080,
                ] {
                    for alignment in 0..4 {
                        let mut phase = fixture(0x0800_0200, raw);
                        phase.cpu.regs[13] =
                            (address + alignment).wrapping_sub(u32::from(offset) * 4);
                        phase.cpu.regs[usize::from(rd)] = 0x81F2_7354;
                        phase.cpu.cpsr |= CPSR_CARRY | CPSR_OVERFLOW;
                        let mut direct = phase.clone();
                        direct.bus.begin_frame_service();
                        assert_eq!(
                            oracle_step(&mut phase, &mut direct),
                            load || address < 0x0800_0000,
                            "{raw:04X} {address:08X}+{alignment}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn frame_thumb_sp_preserves_wrapping_and_peripheral_fallbacks() {
    for load in [false, true] {
        for address in [
            0x0000_0000u32,
            0x0100_0000,
            0x01FF_FFFF,
            0x02FF_FFFF,
            0x03FF_FFFF,
            0x0400_0100,
            0x0500_0000,
            0x0600_0000,
            0x0700_0000,
            0x0800_4000,
            0x0DFF_FF00,
            0x0E00_0000,
            0xFFFF_FFFF,
        ] {
            let mut phase = fixture(0x0300_0200, 0x91FF | u16::from(load) << 11);
            phase.cpu.regs[13] = address.wrapping_sub(1020);
            let mut direct = phase.clone();
            direct.bus.begin_frame_service();
            assert_eq!(
                oracle_step(&mut phase, &mut direct),
                matches!(address >> 24, 2 | 3),
                "{address:08X}"
            );
        }
    }
}

#[test]
fn frame_thumb_sp_preserves_live_fetch_and_crossing_horizons() {
    for base in CODE_BASES {
        for load in [false, true] {
            for cutoff in [0, 1, 2, 3, 4, 5, 8, 16] {
                let mut phase = fixture(base, 0x9100 | u16::from(load) << 11);
                if base < 0x0800_0000 {
                    phase.cpu.regs[13] = base + 8;
                    phase.cpu.regs[1] = 0x2101_2102;
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

#[test]
fn frame_thumb_sp_restored_phases_and_load_followup_match_scalar() {
    for raw in [0x9100, 0x9900] {
        for phases in 0..10 {
            let mut phase = fixture(0x0300_0200, raw);
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
fn frame_thumb_sp_timer_deadline_preserves_crossing_phase_execution() {
    for base in [0x0300_0200, 0x0800_0200, 0x0C00_0200] {
        for raw in [0x9100, 0x9900] {
            for cut in 1..64u16 {
                let mut phase = fixture(base, raw);
                phase.bus.write16(0x0400_0100, 0u16.wrapping_sub(cut));
                phase.bus.write16(0x0400_0102, 0xC0);
                let before = phase.cpu.cycles;
                let mut direct = phase.clone();
                direct.bus.begin_frame_service();
                let budget = direct.bus.frame_cpu_cycle_budget();
                let expected = phase.cpu.step(&mut phase.bus).unwrap();
                let cycles = (phase.cpu.cycles - before) as u32;
                let result =
                    direct
                        .cpu
                        .run_frame_direct(&mut direct.bus, phase.cpu.cycles, true, true);
                assert_eq!(
                    result.is_some(),
                    cycles <= budget,
                    "{base:08X} {raw:04X} cut={cut}"
                );
                let actual = if let Some((fetched, count)) = result {
                    assert_eq!(count, 1);
                    fetched
                } else {
                    assert_eq!(direct.cpu.cycles, before);
                    direct.cpu.step(&mut direct.bus).unwrap()
                };
                assert_eq!(actual, expected);
                assert_projected_equal(&phase, &direct);
            }
        }
    }
}
