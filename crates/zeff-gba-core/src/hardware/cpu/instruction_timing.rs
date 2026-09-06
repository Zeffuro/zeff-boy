use super::{
    ArmInstructionClass, DataAccessCursor, DecodedInstruction, FetchedInstruction,
    ThumbInstructionClass,
};

pub(super) struct DataAccessCharge {
    pub(super) cycles: u32,
    #[cfg(test)]
    pub(super) replaced_legacy_cycles: u32,
    #[cfg(test)]
    pub(super) non_data_cycles: u32,
    #[cfg(test)]
    pub(super) required_cycles: u32,
}

impl DataAccessCharge {
    pub(super) fn new(
        fetch_cycles: u32,
        total_cycles: u32,
        cursor: DataAccessCursor,
        hle: bool,
    ) -> Self {
        let incremental_cycles = total_cycles.saturating_sub(fetch_cycles);
        let replaced_legacy_cycles = if hle {
            0
        } else {
            cursor.access_count().min(incremental_cycles)
        };
        let non_data_cycles = incremental_cycles.saturating_sub(replaced_legacy_cycles);
        let required_cycles = fetch_cycles
            .saturating_add(non_data_cycles)
            .saturating_add(cursor.elapsed_cycles());
        Self {
            cycles: total_cycles.max(required_cycles),
            #[cfg(test)]
            replaced_legacy_cycles,
            #[cfg(test)]
            non_data_cycles,
            #[cfg(test)]
            required_cycles,
        }
    }
}

pub(super) fn instruction_base_cycles(fetched: FetchedInstruction, condition_passed: bool) -> u32 {
    if !condition_passed {
        return 0;
    }

    match fetched.decoded {
        DecodedInstruction::Arm {
            class: ArmInstructionClass::DataProcessing,
            ..
        } => {
            if fetched.raw & (1 << 25) == 0 && fetched.raw & (1 << 4) != 0 {
                1
            } else {
                0
            }
        }
        DecodedInstruction::Arm {
            class: ArmInstructionClass::Branch | ArmInstructionClass::BranchExchange,
            ..
        } => 0,
        DecodedInstruction::Arm {
            class: ArmInstructionClass::SingleDataTransfer,
            ..
        } => {
            if fetched.raw & (1 << 20) != 0 {
                2
            } else {
                1
            }
        }
        DecodedInstruction::Arm {
            class: ArmInstructionClass::BlockDataTransfer,
            ..
        } => {
            let register_count = block_transfer_register_count(fetched.raw);
            if fetched.raw & (1 << 20) != 0 {
                register_count + 1
            } else {
                register_count
            }
        }
        DecodedInstruction::Arm {
            class: ArmInstructionClass::SingleDataSwap,
            ..
        } => 3,
        DecodedInstruction::Thumb {
            class:
                ThumbInstructionClass::MoveShiftedRegister
                | ThumbInstructionClass::AddSubtract
                | ThumbInstructionClass::Immediate
                | ThumbInstructionClass::LoadAddress
                | ThumbInstructionClass::AddOffsetSp
                | ThumbInstructionClass::HiRegisterBranchExchange
                | ThumbInstructionClass::UnconditionalBranch
                | ThumbInstructionClass::LongBranchWithLink,
        } => 0,
        DecodedInstruction::Thumb {
            class: ThumbInstructionClass::Alu,
        } => match (fetched.raw >> 6) & 0xF {
            0x2 | 0x3 | 0x4 | 0x7 | 0xD => 1,
            _ => 0,
        },
        DecodedInstruction::Thumb {
            class: ThumbInstructionClass::ConditionalBranchOrSwi,
        } if fetched.raw as u16 & 0x0F00 != 0x0F00 => 0,
        _ => 1,
    }
}

fn block_transfer_register_count(raw: u32) -> u32 {
    let count = (raw & 0xFFFF).count_ones();
    if count == 0 { 16 } else { count }
}

pub(super) fn instruction_has_load_final_internal_cycle(
    fetched: FetchedInstruction,
    condition_passed: bool,
) -> bool {
    if !condition_passed {
        return false;
    }

    match fetched.decoded {
        DecodedInstruction::Thumb {
            class: ThumbInstructionClass::PcRelativeLoad,
        } => true,
        DecodedInstruction::Thumb {
            class: ThumbInstructionClass::LoadStore,
        } => {
            let raw = fetched.raw as u16;
            if raw & 0xF000 == 0x5000 {
                (raw >> 9) & 0x7 >= 0b011
            } else {
                raw & (1 << 11) != 0
            }
        }
        DecodedInstruction::Thumb {
            class: ThumbInstructionClass::LoadStoreHalfword | ThumbInstructionClass::SpRelativeLoad,
        } => fetched.raw & (1 << 11) != 0,
        _ => false,
    }
}
