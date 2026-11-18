use super::*;
use std::cell::RefCell;
use zeff_emu_common::debug::{
    InstructionTraceRecord, TraceExecMode, TraceFrameKnowledge, TraceObservationStart, TraceWrite,
    TraceWriteKind, TraceWriteWidth,
};

pub(crate) trait AttemptCapture: Clone {
    fn begin(cpu: &Cpu, state: BusState) -> Self;
    fn fetch(&self, address: u16, value: u8);
    fn read(&self, address: u16, value: u8, io: bool);
    fn write(&mut self, address: u16, value: u8, io: bool);
    fn finish(&mut self, instruction: FetchedInstruction, complete: bool);
}

#[derive(Clone, Copy)]
pub(crate) struct NoAttempt;
impl AttemptCapture for NoAttempt {
    fn begin(_: &Cpu, _: BusState) -> Self {
        Self
    }
    fn fetch(&self, _: u16, _: u8) {}
    fn read(&self, _: u16, _: u8, _: bool) {}
    fn write(&mut self, _: u16, _: u8, _: bool) {}
    fn finish(&mut self, _: FetchedInstruction, _: bool) {}
}

#[derive(Clone)]
pub(crate) struct Attempt {
    pub(super) record: RefCell<InstructionTraceRecord>,
    pub(super) accesses: RefCell<super::watch::AccessLog>,
    pub(super) before: [u32; 14],
    pub(super) complete: bool,
    pub(super) halted: bool,
}

impl AttemptCapture for Attempt {
    fn begin(cpu: &Cpu, state: BusState) -> Self {
        let mut record = InstructionTraceRecord::new(
            TraceExecMode::Z80,
            u32::from(cpu.regs().pc),
            None,
            0,
            cpu.cycles(),
            &[],
        );
        record.observation.frame_knowledge = TraceFrameKnowledge::Unknown;
        if state.pending() {
            record.observation.start = TraceObservationStart::Suffix;
        }
        Self {
            record: RefCell::new(record),
            accesses: RefCell::default(),
            before: registers(cpu),
            complete: false,
            halted: cpu.is_halted(),
        }
    }
    fn fetch(&self, address: u16, value: u8) {
        append_byte(&mut self.record.borrow_mut(), value);
        self.accesses.borrow_mut().push(super::SoundAccess {
            space: super::SoundWatchSpace::Memory,
            kind: super::SoundAccessKind::Instruction,
            address,
            value,
        });
    }
    fn read(&self, address: u16, value: u8, io: bool) {
        self.accesses.borrow_mut().push(super::SoundAccess {
            space: if io {
                super::SoundWatchSpace::Io
            } else {
                super::SoundWatchSpace::Memory
            },
            kind: super::SoundAccessKind::Read,
            address,
            value,
        });
    }
    fn write(&mut self, address: u16, value: u8, io: bool) {
        self.accesses.borrow_mut().push(super::SoundAccess {
            space: if io {
                super::SoundWatchSpace::Io
            } else {
                super::SoundWatchSpace::Memory
            },
            kind: super::SoundAccessKind::Write,
            address,
            value,
        });
        self.record.get_mut().push_write(TraceWrite {
            address: u32::from(address),
            old_value: None,
            new_value: u32::from(value),
            width: TraceWriteWidth::Byte,
            kind: if io {
                TraceWriteKind::Io
            } else {
                TraceWriteKind::Memory
            },
        });
    }
    fn finish(&mut self, instruction: FetchedInstruction, complete: bool) {
        self.record.get_mut().pc = u32::from(instruction.pc);
        self.complete = complete;
    }
}

pub(super) fn append_byte(record: &mut InstructionTraceRecord, value: u8) {
    let index = usize::from(record.instruction_len);
    if let Some(byte) = record.instruction.get_mut(index) {
        *byte = value;
        record.instruction_len += 1;
    } else {
        record.instruction_overflow = record.instruction_overflow.saturating_add(1);
    }
}

pub(super) fn registers(cpu: &Cpu) -> [u32; 14] {
    let r = cpu.regs();
    [
        u32::from(r.a),
        u32::from(r.f),
        u32::from(r.b),
        u32::from(r.c),
        u32::from(r.d),
        u32::from(r.e),
        u32::from(r.h),
        u32::from(r.l),
        u32::from(r.ix),
        u32::from(r.iy),
        u32::from(r.sp),
        u32::from(r.pc),
        u32::from(r.i),
        u32::from(r.r),
    ]
}
