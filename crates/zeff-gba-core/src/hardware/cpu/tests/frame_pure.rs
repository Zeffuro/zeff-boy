use super::frame_direct::assert_cpu_equal;
use super::*;
use crate::hardware::cpu::decode::decode_stub;
use crate::hardware::cpu::frame_direct::classify_stateless_pure;

fn fetched(raw: u32, instruction_set: InstructionSet) -> FetchedInstruction {
    FetchedInstruction {
        pc: 0x0800_0200,
        raw,
        instruction_set,
        width_bytes: instruction_set.width_bytes(),
        fetch_cycles: 1,
        decoded: decode_stub(raw, instruction_set),
    }
}

fn seeded_cpu(flags: u32) -> Cpu {
    let mut cpu = Cpu::new();
    cpu.cpsr = (cpu.cpsr & 0x0FFF_FFFF) | flags << 28;
    cpu.spsr = 0xA000_0012;
    for (index, register) in cpu.regs[..15].iter_mut().enumerate() {
        *register = 0x1020_4081u32
            .wrapping_mul(index as u32 + 1)
            .rotate_left(index as u32);
    }
    cpu
}

fn assert_pure_matches(raw: u32, instruction_set: InstructionSet, flags: u32) {
    let fetched = fetched(raw, instruction_set);
    let mut generic = seeded_cpu(flags);
    let condition_passed = generic.fetched_condition_passed(fetched);
    let operation = classify_stateless_pure(fetched, condition_passed).unwrap();
    let mut direct = generic.clone();
    let mut bus = bus_with_rom(&[]);
    generic.execute_fetched(&mut bus, fetched);
    direct.execute_frame_pure(operation, fetched.pc, fetched.raw);
    assert_cpu_equal(&generic, &direct);
}

#[test]
fn direct_pure_matches_every_thumb_encoding() {
    for raw in 0..=u16::MAX {
        let fetched = fetched(u32::from(raw), InstructionSet::Thumb);
        if classify_stateless_pure(fetched, true).is_none() {
            continue;
        }
        for flags in [0, 2, 5, 10, 15] {
            assert_pure_matches(u32::from(raw), InstructionSet::Thumb, flags);
        }
    }
}

#[test]
fn direct_pure_matches_arm_data_processing_and_multiply() {
    for flags in [0, 2, 5, 10, 15] {
        for opcode in 0..16 {
            for set_flags in [false, true] {
                for operand in [
                    (1 << 25) | 0x5A,
                    1 | (3 << 5) | (17 << 7),
                    1 | (1 << 4) | (2 << 5) | (3 << 8),
                ] {
                    let raw = 0xE000_0000
                        | opcode << 21
                        | u32::from(set_flags) << 20
                        | 5 << 16
                        | 4 << 12
                        | operand;
                    assert_pure_matches(raw, InstructionSet::Arm, flags);
                }
            }
        }

        for accumulate in [false, true] {
            for set_flags in [false, true] {
                for [rd, rn, rs, rm] in [[4, 3, 2, 1], [1, 1, 1, 1], [2, 3, 2, 2]] {
                    let raw = 0xE000_0090
                        | u32::from(accumulate) << 21
                        | u32::from(set_flags) << 20
                        | rd << 16
                        | rn << 12
                        | rs << 8
                        | rm;
                    assert_pure_matches(raw, InstructionSet::Arm, flags);
                }
            }
        }
    }
}

#[test]
fn direct_pure_failed_arm_conditions_are_exact_noops() {
    for raw in [
        0x0280_0001,
        0x0000_0291,
        0x0590_1000,
        0x0800_0003,
        0x0A00_0001,
        0x012F_FF10,
        0x0F00_0001,
        0x0C00_0000,
        0xF280_0001,
    ] {
        assert_pure_matches(raw, InstructionSet::Arm, 0);
    }
}
