use super::decode::decode_arm_class;
use super::instruction_timing::instruction_base_cycles_parts;
use super::{ArmInstructionClass, DecodedInstruction};

const FULL_DECODE: u8 = 0x40;
const DYNAMIC_CYCLES: u8 = 0x80;

// The low nibble stores the canonical class discriminant. Keeping the class
// conversion separate leaves each large-table entry exactly one byte.
const CLASSES: [ArmInstructionClass; 16] = [
    ArmInstructionClass::BranchExchange,
    ArmInstructionClass::Branch,
    ArmInstructionClass::BlockDataTransfer,
    ArmInstructionClass::SingleDataTransfer,
    ArmInstructionClass::DataProcessing,
    ArmInstructionClass::Multiply,
    ArmInstructionClass::MultiplyLong,
    ArmInstructionClass::SingleDataSwap,
    ArmInstructionClass::SoftwareInterrupt,
    ArmInstructionClass::Coprocessor,
    ArmInstructionClass::Unknown,
    ArmInstructionClass::Unknown,
    ArmInstructionClass::Unknown,
    ArmInstructionClass::Unknown,
    ArmInstructionClass::Unknown,
    ArmInstructionClass::Unknown,
];

const fn index(raw: u32) -> usize {
    (((raw >> 16) & 0xFF0) | ((raw >> 4) & 0xF)) as usize
}

const fn build_table() -> [u8; 4096] {
    // Unproved entries must fall back, never inherit a representative class.
    let mut table = [FULL_DECODE; 4096];
    let mut slot = 0;
    while slot < table.len() {
        // BX inspects bits 19:8; SWP also inspects bits 11:8. All candidate
        // buckets escape, including encodings whose representative is not BX.
        if slot != 0x121 && slot != 0x109 && slot != 0x149 {
            let raw = (((slot as u32) & 0xFF0) << 16) | (((slot as u32) & 0xF) << 4);
            let class = decode_arm_class(raw);
            let decoded = DecodedInstruction::Arm {
                condition: 0,
                class,
            };
            table[slot] = if matches!(class, ArmInstructionClass::BlockDataTransfer) {
                class as u8 | DYNAMIC_CYCLES
            } else {
                let cycles = instruction_base_cycles_parts(raw, decoded, true);
                assert!(cycles <= 3);
                class as u8 | ((cycles as u8) << 4)
            };
        }
        slot += 1;
    }
    table
}

static TABLE: [u8; 4096] = build_table();

#[inline]
pub(super) fn lookup(raw: u32) -> (DecodedInstruction, u32) {
    let packed = TABLE[index(raw)];
    let class = if packed & FULL_DECODE != 0 {
        decode_arm_class(raw)
    } else {
        CLASSES[usize::from(packed & 0xF)]
    };
    let decoded = DecodedInstruction::Arm {
        condition: (raw >> 28) as u8,
        class,
    };
    let cycles = if packed & (FULL_DECODE | DYNAMIC_CYCLES) != 0 {
        instruction_base_cycles_parts(raw, decoded, true)
    } else {
        u32::from((packed >> 4) & 3)
    };
    (decoded, cycles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::cpu::InstructionSet;
    use crate::hardware::cpu::decode::decode_stub;
    use crate::hardware::cpu::frame_classify::classify_stateless_pure_parts;

    fn assert_metadata(raw: u32) {
        let expected = decode_stub(raw, InstructionSet::Arm);
        let (decoded, cycles) = lookup(raw);
        assert_eq!(decoded, expected, "{raw:08X}");
        assert_eq!(
            cycles,
            instruction_base_cycles_parts(raw, expected, true),
            "{raw:08X}"
        );
        for passed in [false, true] {
            assert_eq!(
                if passed { cycles } else { 0 },
                instruction_base_cycles_parts(raw, expected, passed),
                "{raw:08X}"
            );
            assert_eq!(
                classify_stateless_pure_parts(raw, decoded, passed).map(|kind| kind as u8),
                classify_stateless_pure_parts(raw, expected, passed).map(|kind| kind as u8),
                "{raw:08X}"
            );
        }
    }

    #[test]
    fn arm_metadata_matches_all_indices_and_each_omitted_register_field() {
        for slot in 0..4096 {
            let raw = ((slot & 0xFF0) << 16) | ((slot & 0xF) << 4);
            for shift in [0, 8, 12, 16, 28] {
                for field in 0..16 {
                    assert_metadata(raw | (field << shift));
                    // Simultaneous all-ones in the other omitted fields also
                    // exercises register-PC and full-encoding mask boundaries.
                    assert_metadata(raw | (0xF00F_FF0F & !(0xF << shift)) | (field << shift));
                }
            }
        }
    }

    #[test]
    fn arm_metadata_escapes_full_encoding_exceptions_and_live_block_lists() {
        for (index, class) in CLASSES[..=ArmInstructionClass::Unknown as usize]
            .iter()
            .enumerate()
        {
            assert_eq!(*class as usize, index, "class-map discriminant changed");
        }
        for raw in [
            0x012F_FF10,
            0x0100_0090,
            0x0140_0090,
            0x010F_0000,
            0x0120_F000,
            0x0320_F000,
            0x0000_0090,
            0x0080_0090,
        ] {
            for bit in 0..28 {
                assert_metadata(raw ^ (1 << bit));
            }
            for condition in 0..16 {
                assert_metadata(raw | (condition << 28));
            }
        }
        for list in 0..=u16::MAX {
            assert_metadata(0xE890_0000 | u32::from(list));
            assert_metadata(0xE880_0000 | u32::from(list));
        }
        for slot in [0x121, 0x109, 0x149] {
            assert_ne!(TABLE[slot] & FULL_DECODE, 0);
        }
        assert_eq!(size_of_val(&TABLE), 4096);
    }
}
