use super::InstructionSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArmInstructionClass {
    BranchExchange,
    Branch,
    BlockDataTransfer,
    SingleDataTransfer,
    DataProcessing,
    Multiply,
    MultiplyLong,
    SingleDataSwap,
    SoftwareInterrupt,
    Coprocessor,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThumbInstructionClass {
    MoveShiftedRegister,
    AddSubtract,
    Immediate,
    Alu,
    HiRegisterBranchExchange,
    PcRelativeLoad,
    LoadStore,
    LoadStoreHalfword,
    SpRelativeLoad,
    LoadAddress,
    AddOffsetSp,
    PushPop,
    MultipleLoadStore,
    ConditionalBranchOrSwi,
    UnconditionalBranch,
    LongBranchWithLink,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodedInstruction {
    Arm {
        condition: u8,
        class: ArmInstructionClass,
    },
    Thumb {
        class: ThumbInstructionClass,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FetchedInstruction {
    pub pc: u32,
    pub raw: u32,
    pub instruction_set: InstructionSet,
    pub width_bytes: u8,
    pub fetch_cycles: u32,
    pub decoded: DecodedInstruction,
}

pub(super) fn decode_stub(raw: u32, instruction_set: InstructionSet) -> DecodedInstruction {
    match instruction_set {
        InstructionSet::Arm => DecodedInstruction::Arm {
            condition: ((raw >> 28) & 0xF) as u8,
            class: decode_arm_class(raw),
        },
        InstructionSet::Thumb => DecodedInstruction::Thumb {
            class: decode_thumb_class(raw as u16),
        },
    }
}

fn decode_arm_class(raw: u32) -> ArmInstructionClass {
    if raw & 0x0FFF_FFF0 == 0x012F_FF10 {
        return ArmInstructionClass::BranchExchange;
    }
    if raw & 0x0E00_0090 == 0x0000_0090 && raw & 0x60 != 0 {
        return ArmInstructionClass::SingleDataTransfer;
    }
    if raw & 0x0F00_0000 == 0x0F00_0000 {
        return ArmInstructionClass::SoftwareInterrupt;
    }
    if raw & 0x0FB0_0FF0 == 0x0100_0090 {
        return ArmInstructionClass::SingleDataSwap;
    }
    match (raw >> 25) & 0x7 {
        0b101 => ArmInstructionClass::Branch,
        0b100 => ArmInstructionClass::BlockDataTransfer,
        0b010 | 0b011 => ArmInstructionClass::SingleDataTransfer,
        0b000 | 0b001 => {
            if raw & 0x0F80_00F0 == 0x0080_0090 {
                ArmInstructionClass::MultiplyLong
            } else if raw & 0x0FC0_00F0 == 0x0000_0090 {
                ArmInstructionClass::Multiply
            } else {
                ArmInstructionClass::DataProcessing
            }
        }
        0b110 | 0b111 => ArmInstructionClass::Coprocessor,
        _ => ArmInstructionClass::Unknown,
    }
}

fn decode_thumb_class(raw: u16) -> ThumbInstructionClass {
    match raw >> 11 {
        0b00000..=0b00010 => ThumbInstructionClass::MoveShiftedRegister,
        0b00011 => ThumbInstructionClass::AddSubtract,
        0b00100..=0b00111 => ThumbInstructionClass::Immediate,
        0b01000 => {
            if raw & 0x0400 != 0 {
                ThumbInstructionClass::HiRegisterBranchExchange
            } else {
                ThumbInstructionClass::Alu
            }
        }
        0b01001 => ThumbInstructionClass::PcRelativeLoad,
        0b01010..=0b01111 => ThumbInstructionClass::LoadStore,
        0b10000..=0b10001 => ThumbInstructionClass::LoadStoreHalfword,
        0b10010..=0b10011 => ThumbInstructionClass::SpRelativeLoad,
        0b10100..=0b10101 => ThumbInstructionClass::LoadAddress,
        0b10110 => {
            if raw & 0x0F00 == 0 {
                ThumbInstructionClass::AddOffsetSp
            } else {
                ThumbInstructionClass::PushPop
            }
        }
        0b10111 => ThumbInstructionClass::PushPop,
        0b11000..=0b11001 => ThumbInstructionClass::MultipleLoadStore,
        0b11010..=0b11011 => ThumbInstructionClass::ConditionalBranchOrSwi,
        0b11100 => ThumbInstructionClass::UnconditionalBranch,
        0b11110..=0b11111 => ThumbInstructionClass::LongBranchWithLink,
        _ => ThumbInstructionClass::Unknown,
    }
}
