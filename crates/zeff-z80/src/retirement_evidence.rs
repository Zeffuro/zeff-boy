use super::*;
use std::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstructionByteClass {
    M1,
    Operand,
    Displacement,
    Terminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsumedInstructionByte {
    pub address: u16,
    pub value: u8,
    pub class: InstructionByteClass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetirementKind {
    Instruction,
    HaltPoll,
    Interrupt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetirementEvidence {
    pub complete: bool,
    pub consumed: Option<ConsumedInstructionByte>,
    pub input_overflowed: bool,
    pub cpu_before: u64,
    pub cpu_after: u64,
    pub kind: RetirementKind,
}

impl RetirementEvidence {
    pub(crate) fn new(
        complete: bool,
        consumed: Option<ConsumedInstructionByte>,
        cpu_before: u64,
        cpu_after: u64,
        kind: RetirementKind,
    ) -> Self {
        Self {
            complete,
            consumed,
            input_overflowed: false,
            cpu_before,
            cpu_after,
            kind,
        }
    }
}

pub(crate) trait Capture {
    fn record(&self, byte: ConsumedInstructionByte);
}

pub(crate) struct NoCapture;
impl Capture for NoCapture {
    #[inline(always)]
    fn record(&self, _: ConsumedInstructionByte) {}
}

#[derive(Default)]
pub(crate) struct InputCapture {
    byte: Cell<Option<ConsumedInstructionByte>>,
    overflowed: Cell<bool>,
}
impl InputCapture {
    pub(crate) fn get(&self) -> Option<ConsumedInstructionByte> {
        if self.overflowed.get() {
            None
        } else {
            self.byte.get()
        }
    }
    pub(crate) fn overflowed(&self) -> bool {
        self.overflowed.get()
    }
}
impl Capture for InputCapture {
    fn record(&self, byte: ConsumedInstructionByte) {
        self.overflowed
            .set(self.overflowed.get() || self.byte.replace(Some(byte)).is_some());
    }
}

impl Cpu {
    pub(crate) fn accept_instruction_input<C: Capture>(
        &mut self,
        value: u8,
        class: InstructionByteClass,
        capture: &C,
    ) {
        capture.record(ConsumedInstructionByte {
            address: self.regs.pc,
            value,
            class,
        });
        self.accept_fetched_byte(value);
    }
}
