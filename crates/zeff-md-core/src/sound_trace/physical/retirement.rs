use super::{CpuSnapshot, HalfCapture};
use zeff_z80::*;

macro_rules! retirement {
    ($name:ident, $completion:ty, $result:ty, $ordinary:ident, $observed:ident) => {
        pub(crate) fn $name<C: HalfCapture>(
            cpu: &mut Cpu,
            state: &mut BusState,
            completion: $completion,
            capture: &mut C,
        ) -> Result<$result, CycleError> {
            if C::ENABLED {
                let before = CpuSnapshot::registers(cpu);
                let boundary = !state.pending();
                let (result, evidence) = cpu.$observed(state, completion)?;
                let starts = boundary
                    && match evidence.kind {
                        RetirementKind::Interrupt => true,
                        RetirementKind::Instruction => evidence
                            .consumed
                            .is_some_and(|byte| byte.class == InstructionByteClass::M1),
                        RetirementKind::HaltPoll => false,
                    };
                capture.cpu_snapshot(CpuSnapshot {
                    before,
                    after: CpuSnapshot::registers(cpu),
                    starts,
                });
                capture.retired(evidence);
                Ok(result)
            } else {
                cpu.$ordinary(state, completion)
            }
        }
    };
}
retirement!(
    physical,
    CycleCompletion,
    BoundedStep,
    retire_physical_cycle,
    retire_physical_cycle_with_evidence
);
retirement!(
    jr,
    JrCycleCompletion,
    JrRetirement,
    retire_jr_cycle,
    retire_jr_cycle_with_evidence
);
retirement!(
    word,
    WordAddCompletion,
    WordAddRetirement,
    retire_word_add_cycle,
    retire_word_add_cycle_with_evidence
);
retirement!(
    djnz,
    DjnzCompletion,
    DjnzRetirement,
    retire_djnz_cycle,
    retire_djnz_cycle_with_evidence
);
retirement!(
    indexed,
    IndexedMemoryCompletion,
    BoundedStep,
    retire_indexed_memory_cycle,
    retire_indexed_memory_cycle_with_evidence
);
retirement!(
    indexed_cb,
    IndexedCbCycleCompletion,
    BoundedStep,
    retire_indexed_cb_cycle,
    retire_indexed_cb_cycle_with_evidence
);
retirement!(
    ed_word,
    EdWordCompletion,
    BoundedStep,
    retire_ed_word_cycle,
    retire_ed_word_cycle_with_evidence
);
retirement!(
    copy,
    CopyCycleCompletion,
    BoundedStep,
    retire_copy_cycle,
    retire_copy_cycle_with_evidence
);
retirement!(
    im1,
    Im1CycleCompletion,
    BoundedStep,
    retire_im1_cycle,
    retire_im1_cycle_with_evidence
);
