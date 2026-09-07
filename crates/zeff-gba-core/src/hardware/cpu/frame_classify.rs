#[cfg(any(test, feature = "profiling"))]
use super::FetchedInstruction;
use super::frame_direct::DirectPure;
use super::{ArmInstructionClass, DecodedInstruction, ThumbInstructionClass};

#[cfg(any(test, feature = "profiling"))]
pub(super) fn stateless_candidate(
    fetched: FetchedInstruction,
    condition_passed: bool,
) -> Option<usize> {
    classify_stateless_pure(fetched, condition_passed).map(|operation| match operation {
        DirectPure::ArmConditionFailed => 0,
        DirectPure::ArmDataProcessing
            if fetched.raw & (1 << 25) == 0 && fetched.raw & (1 << 4) != 0 =>
        {
            2
        }
        DirectPure::ArmDataProcessing | DirectPure::ArmMultiply => 1,
        DirectPure::ThumbMoveShiftedRegister
        | DirectPure::ThumbAddSubtract
        | DirectPure::ThumbImmediate
        | DirectPure::ThumbAlu
        | DirectPure::ThumbLoadAddress
        | DirectPure::ThumbAddOffsetSp => 3,
    })
}

#[cfg(any(test, feature = "profiling"))]
pub(super) fn classify_stateless_pure(
    fetched: FetchedInstruction,
    condition_passed: bool,
) -> Option<DirectPure> {
    classify_stateless_pure_parts(fetched.raw, fetched.decoded, condition_passed)
}

pub(super) fn classify_stateless_pure_parts(
    raw: u32,
    decoded: DecodedInstruction,
    condition_passed: bool,
) -> Option<DirectPure> {
    match decoded {
        DecodedInstruction::Arm { .. } if !condition_passed => Some(DirectPure::ArmConditionFailed),
        DecodedInstruction::Arm {
            class: ArmInstructionClass::DataProcessing,
            ..
        } => {
            if raw & 0x0FBF_0FFF == 0x010F_0000
                || raw & 0x0FB0_FFF0 == 0x0120_F000
                || raw & 0x0FB0_F000 == 0x0320_F000
                || (raw >> 12) & 0xF == 15
            {
                None
            } else {
                Some(DirectPure::ArmDataProcessing)
            }
        }
        DecodedInstruction::Arm {
            class: ArmInstructionClass::Multiply,
            ..
        } => [raw >> 16, raw >> 12, raw >> 8, raw]
            .into_iter()
            .all(|register| register & 0xF != 15)
            .then_some(DirectPure::ArmMultiply),
        DecodedInstruction::Thumb { class } => classify_thumb_pure(class),
        _ => None,
    }
}

pub(super) const fn classify_thumb_pure(class: ThumbInstructionClass) -> Option<DirectPure> {
    match class {
        ThumbInstructionClass::MoveShiftedRegister => Some(DirectPure::ThumbMoveShiftedRegister),
        ThumbInstructionClass::AddSubtract => Some(DirectPure::ThumbAddSubtract),
        ThumbInstructionClass::Immediate => Some(DirectPure::ThumbImmediate),
        ThumbInstructionClass::Alu => Some(DirectPure::ThumbAlu),
        ThumbInstructionClass::LoadAddress => Some(DirectPure::ThumbLoadAddress),
        ThumbInstructionClass::AddOffsetSp => Some(DirectPure::ThumbAddOffsetSp),
        _ => None,
    }
}
