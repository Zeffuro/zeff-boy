use super::frame_direct::assert_projected_equal;
use super::frame_mixed::{fixture, oracle_step};
use super::*;

#[test]
fn frame_stream_final_identity_uses_prefetched_raw_and_original_fetch_charge() {
    for base in [0x0200_0200, 0x0300_0200, 0x0800_0200] {
        for raw in [0xE280_1001, 0x0280_1001, 0x0580_1000, 0xF280_1001] {
            for pending_load in [false, true] {
                for cutoff in 0..16 {
                    let mut phase = fixture(base, &[raw]);
                    phase.cpu.pending_load_internal_cycle = pending_load;
                    if base < 0x0800_0000 {
                        // The queued instruction must survive writes to its backing memory.
                        phase.bus.write32(base + 4, 0xE3A0_10FF);
                    }
                    let mut direct = phase.clone();
                    direct.bus.begin_frame_service();
                    let guard = direct.cpu.cycles + cutoff;
                    if let Some((last, count)) =
                        direct
                            .cpu
                            .run_frame_direct(&mut direct.bus, guard, true, true)
                    {
                        let mut expected = None;
                        for _ in 0..count {
                            expected = phase.cpu.step(&mut phase.bus);
                        }
                        assert_eq!(Some(last), expected);
                        assert_eq!(direct.cpu.last_fetch, expected);
                    }
                    assert_projected_equal(&phase, &direct);
                }
            }
        }
    }
}

#[test]
fn frame_stream_preserves_load_then_pure_and_timer_crossing_completion() {
    for cutoff in 1..32u16 {
        let mut phase = fixture(0x0300_0200, &[0xE590_1000, 0xE281_2001, 0xE282_3001]);
        phase.bus.write16(0x0400_0100, 0u16.wrapping_sub(cutoff));
        phase.bus.write16(0x0400_0102, 0xC0);
        let mut direct = phase.clone();
        direct.bus.begin_frame_service();
        for _ in 0..4 {
            oracle_step(&mut phase, &mut direct);
        }
    }
}

#[test]
fn frame_stream_arm_parts_preserve_full_decoding_bits_and_register_lists() {
    use crate::hardware::cpu::decode::decode_stub;
    use crate::hardware::cpu::frame_classify::classify_stateless_pure_parts;
    use crate::hardware::cpu::frame_direct::classify_stateless_pure;
    use crate::hardware::cpu::instruction_timing::instruction_base_cycles_parts;
    for base in [
        0xE12F_FF10u32,
        0xE100_0090,
        0xE10F_0000,
        0xE120_F000,
        0xE320_F000,
        0xE890_0000,
        0xE880_0000,
        0xE000_0090,
        0xE080_0090,
    ] {
        for omitted in [
            0,
            1,
            0xF,
            0x80,
            0x100,
            0xF00,
            0x1000,
            0xF000,
            0xFFFF,
            0x10000,
            0xF0000,
            0x0040_0000,
        ] {
            for raw in [base ^ omitted, base | omitted] {
                for condition_passed in [false, true] {
                    let decoded = decode_stub(raw, InstructionSet::Arm);
                    let fetched = FetchedInstruction {
                        pc: 0x0300_0200,
                        raw,
                        instruction_set: InstructionSet::Arm,
                        width_bytes: 4,
                        fetch_cycles: 3,
                        decoded,
                    };
                    assert_eq!(
                        instruction_base_cycles_parts(raw, decoded, condition_passed),
                        instruction_base_cycles(fetched, condition_passed)
                    );
                    assert_eq!(
                        classify_stateless_pure_parts(raw, decoded, condition_passed)
                            .map(|op| op as u8),
                        classify_stateless_pure(fetched, condition_passed).map(|op| op as u8)
                    );
                    if condition_passed && raw & 0x0E00_0000 == 0x0800_0000 {
                        let registers = (raw & 0xFFFF).count_ones();
                        let registers = if registers == 0 { 16 } else { registers };
                        assert_eq!(
                            instruction_base_cycles_parts(raw, decoded, true),
                            registers + u32::from(raw & (1 << 20) != 0)
                        );
                    }
                }
            }
        }
    }
}
