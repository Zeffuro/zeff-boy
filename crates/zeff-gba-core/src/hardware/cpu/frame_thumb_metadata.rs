use super::decode::decode_thumb_class;
use super::frame_direct::{DirectPure, classify_thumb_pure};
use super::{
    DecodedInstruction, FetchedInstruction, InstructionSet, ThumbInstructionClass,
    instruction_base_cycles,
};

#[derive(Clone, Copy)]
pub(super) struct ThumbMetadata {
    pub class: ThumbInstructionClass,
    pub pure: Option<DirectPure>,
    pub base_cycles: u8,
}

const fn build_table() -> [ThumbMetadata; 1024] {
    let mut table = [ThumbMetadata {
        class: ThumbInstructionClass::Unknown,
        pure: None,
        base_cycles: 1,
    }; 1024];
    let mut index = 0;
    while index < table.len() {
        let raw = (index as u16) << 6;
        let class = decode_thumb_class(raw);
        let fetched = FetchedInstruction {
            pc: 0,
            raw: raw as u32,
            instruction_set: InstructionSet::Thumb,
            width_bytes: 2,
            fetch_cycles: 0,
            decoded: DecodedInstruction::Thumb { class },
        };
        table[index] = ThumbMetadata {
            class,
            pure: classify_thumb_pure(class),
            base_cycles: instruction_base_cycles(fetched, true) as u8,
        };
        index += 1;
    }
    table
}

static TABLE: [ThumbMetadata; 1024] = build_table();
const _: () = assert!(size_of::<ThumbMetadata>() <= 4);

#[inline]
pub(super) fn lookup(raw: u16) -> ThumbMetadata {
    TABLE[usize::from(raw >> 6)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::cpu::decode::decode_stub;
    use crate::hardware::cpu::frame_direct::classify_stateless_pure;

    #[test]
    fn thumb_metadata_matches_every_opcode_against_scalar_rules() {
        for raw in 0..=u16::MAX {
            let metadata = lookup(raw);
            let fetched = FetchedInstruction {
                pc: 0x0300_0102,
                raw: u32::from(raw),
                instruction_set: InstructionSet::Thumb,
                width_bytes: 2,
                fetch_cycles: 3,
                decoded: decode_stub(u32::from(raw), InstructionSet::Thumb),
            };
            assert_eq!(
                DecodedInstruction::Thumb {
                    class: metadata.class
                },
                fetched.decoded,
                "{raw:04X}"
            );
            assert_eq!(
                u32::from(metadata.base_cycles),
                instruction_base_cycles(fetched, true),
                "{raw:04X}"
            );
            for condition in [false, true] {
                assert_eq!(
                    metadata.pure.map(|operation| operation as u8),
                    classify_stateless_pure(fetched, condition).map(|operation| operation as u8),
                    "{raw:04X}"
                );
            }
        }
    }
}
