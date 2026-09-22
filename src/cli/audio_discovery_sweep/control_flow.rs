use serde_json::{Value, json};
use zeff_emu_common::audio_trace::{AudioTraceEvent, AudioTraceSource, NesTraceWrite};
use zeff_emu_common::debug::{
    DebugEvent, InstructionTraceRecord, TraceExecMode, TraceWriteKind, TraceWriteWidth,
};

use super::nes_source::Nrom;

const MAX_DEPTH: usize = 32;
const MAX_OBSERVATIONS: usize = 4096;
const AUDIO_TRACE_CPU_ORIGIN: u64 = 7;

#[derive(Clone, Copy)]
pub(super) struct State {
    pub(super) pc: u16,
    pub(super) a: u8,
    pub(super) x: u8,
    pub(super) y: u8,
    pub(super) sp: u8,
    pub(super) p: u8,
    pub(super) cycle: u64,
}

pub(super) struct Recorder<'a> {
    source: &'a [u8],
    nrom: Nrom,
    frames: Vec<Frame>,
    observations: Vec<Observation>,
    last_after: Option<State>,
}

impl<'a> Recorder<'a> {
    pub(super) fn new(source: &'a [u8]) -> Result<Self, &'static str> {
        let nrom = Nrom::parse(source).ok_or("unsupported NROM source")?;
        Ok(Self {
            source,
            nrom,
            frames: Vec::new(),
            observations: Vec::new(),
            last_after: None,
        })
    }

    pub(super) fn step(
        &mut self,
        before: State,
        after: State,
        record: Option<&InstructionTraceRecord>,
        writers: &[(usize, AudioTraceEvent<NesTraceWrite>)],
    ) -> Result<(), &'static str> {
        if self
            .last_after
            .is_some_and(|previous| !states_match(previous, before))
        {
            return Err("non_contiguous_cpu_state");
        }
        let Some(record) = record else {
            if writers.is_empty() {
                self.last_after = Some(after);
                return Ok(());
            }
            return Err("idle step has audio writers");
        };
        self.authenticate_record(before, after, record)?;
        let bytes = record.instruction_bytes();
        let result = if record.event == Some(DebugEvent::Interrupt) {
            if !writers.is_empty() || !bytes.is_empty() {
                return Err("interrupt record has instruction or writers");
            }
            self.check_active_slot_writes(record)?;
            self.push_interrupt(before, after, record, InterruptTrigger::External)
        } else {
            if bytes.is_empty() {
                return Err("non-interrupt record has no instruction");
            }
            self.record_writers(before, after, record, writers)?;
            self.apply_instruction(before, after, record)
        };
        result?;
        self.last_after = Some(after);
        Ok(())
    }

    pub(super) fn finish(self) -> Value {
        json!({
            "observed_writes": self.observations.len(),
            "observations": self.observations.into_iter().map(Observation::json).collect::<Vec<_>>(),
        })
    }

    fn authenticate_record(
        &self,
        before: State,
        after: State,
        record: &InstructionTraceRecord,
    ) -> Result<(), &'static str> {
        if record.mode != TraceExecMode::Mos6502
            || record.pc != u32::from(before.pc)
            || record.cycle != before.cycle
            || record.register_delta_overflow != 0
            || record.write_overflow != 0
        {
            return Err("invalid instruction record metadata");
        }
        let offset = self
            .nrom
            .offset_for(before.pc)
            .ok_or("executed outside NROM")?;
        if record.physical_rom_offset != offset.checked_sub(16) {
            return Err("instruction record ROM offset mismatch");
        }
        self.authenticate_deltas(before, after, record)?;
        let bytes = record.instruction_bytes();
        if record.event == Some(DebugEvent::Interrupt) {
            return bytes
                .is_empty()
                .then_some(())
                .ok_or("interrupt has instruction bytes");
        }
        if bytes.is_empty() || bytes.len() > 3 {
            return Err("unsupported instruction length");
        }
        for (delta, byte) in bytes.iter().copied().enumerate() {
            let address = before
                .pc
                .checked_add(delta as u16)
                .ok_or("instruction wraps CPU address")?;
            let offset = self
                .nrom
                .offset_for(address)
                .ok_or("instruction leaves NROM")?;
            if self.source.get(offset as usize) != Some(&byte) {
                return Err("instruction bytes do not match source");
            }
        }
        Ok(())
    }

    fn authenticate_deltas(
        &self,
        before: State,
        after: State,
        record: &InstructionTraceRecord,
    ) -> Result<(), &'static str> {
        let before_values = [
            u32::from(before.a),
            u32::from(before.x),
            u32::from(before.y),
            u32::from(before.sp),
            u32::from(before.p),
            u32::from(before.pc),
        ];
        let after_values = [
            u32::from(after.a),
            u32::from(after.x),
            u32::from(after.y),
            u32::from(after.sp),
            u32::from(after.p),
            u32::from(after.pc),
        ];
        let mut delta_index = 0;
        for (register, (&before, &after)) in
            before_values.iter().zip(after_values.iter()).enumerate()
        {
            if before == after {
                continue;
            }
            let Some(delta) = record.register_deltas().get(delta_index) else {
                return Err("register_deltas_mismatch_state");
            };
            if delta.register != register as u8 || delta.value != after {
                return Err("register_deltas_mismatch_state");
            }
            delta_index += 1;
        }
        (delta_index == record.register_deltas().len())
            .then_some(())
            .ok_or("register_deltas_mismatch_state")
    }

    fn record_writers(
        &mut self,
        before: State,
        after: State,
        record: &InstructionTraceRecord,
        writers: &[(usize, AudioTraceEvent<NesTraceWrite>)],
    ) -> Result<(), &'static str> {
        let bytes = record.instruction_bytes();
        for (event_index, writer) in writers {
            self.authenticate_writer(before, after, record, bytes, writer)?;
            if self.observations.len() == MAX_OBSERVATIONS {
                return Err("control-flow observation limit reached");
            }
            let NesTraceWrite::Register { address, value, .. } = writer.write else {
                return Err("writer is not a register write");
            };
            self.observations.push(Observation {
                event_index: *event_index,
                cycle: writer.cycle,
                pc: before.pc,
                register: address,
                value,
                writer_before: before,
                call_path: self.frames.clone(),
            });
        }
        Ok(())
    }

    fn authenticate_writer(
        &self,
        before: State,
        after: State,
        record: &InstructionTraceRecord,
        bytes: &[u8],
        writer: &AudioTraceEvent<NesTraceWrite>,
    ) -> Result<(), &'static str> {
        let start = before
            .cycle
            .checked_sub(AUDIO_TRACE_CPU_ORIGIN)
            .ok_or("invalid CPU cycle")?;
        let end = after
            .cycle
            .checked_sub(AUDIO_TRACE_CPU_ORIGIN)
            .ok_or("invalid CPU cycle")?;
        if !(start..end).contains(&writer.cycle) || writer.pc != record.pc {
            return Err("writer cycle or PC does not match instruction");
        }
        let expected_offset = self
            .nrom
            .offset_for(before.pc)
            .ok_or("writer outside NROM")?;
        if writer.instruction_source
            != (AudioTraceSource::CartridgeRom {
                offset: expected_offset,
                bit_reversed: false,
            })
        {
            return Err("writer source does not match instruction");
        }
        let NesTraceWrite::Register { address, value, .. } = writer.write else {
            return Err("writer is not a register write");
        };
        if !matches!(address, 0x4000..=0x4013 | 0x4015 | 0x4017) || bytes.len() != 3 {
            return Err("writer is not an authenticated APU store");
        }
        let base = u16::from_le_bytes([bytes[1], bytes[2]]);
        let (destination, expected_value) = match bytes[0] {
            0x8c => (base, before.y),
            0x8d => (base, before.a),
            0x8e => (base, before.x),
            0x9d => (base.wrapping_add(u16::from(before.x)), before.a),
            0x99 => (base.wrapping_add(u16::from(before.y)), before.a),
            _ => return Err("writer opcode is not an authenticated store"),
        };
        (address == destination && value == expected_value)
            .then_some(())
            .ok_or("writer destination or value mismatch")
    }

    fn apply_instruction(
        &mut self,
        before: State,
        after: State,
        record: &InstructionTraceRecord,
    ) -> Result<(), &'static str> {
        let bytes = record.instruction_bytes();
        match bytes[0] {
            0x20 => self.push_call(before, after, record),
            0x00 => self.push_interrupt(before, after, record, InterruptTrigger::Brk),
            0x60 => self.pop_frame(before, after, record, FrameKind::Call, 2),
            0x40 => self.pop_frame(before, after, record, FrameKind::Interrupt, 3),
            0x9a if !self.frames.is_empty() => Err("TXS invalidates active return frames"),
            _ => {
                self.check_active_slot_writes(record)?;
                self.check_active_stack_pointer(before, after, bytes[0])
            }
        }
    }

    fn push_call(
        &mut self,
        before: State,
        after: State,
        record: &InstructionTraceRecord,
    ) -> Result<(), &'static str> {
        let bytes = record.instruction_bytes();
        if bytes.len() != 3 || after.sp != before.sp.wrapping_sub(2) {
            return Err("JSR state mismatch");
        }
        let return_pc = before.pc.checked_add(3).ok_or("JSR return address wraps")?;
        let target = u16::from_le_bytes([bytes[1], bytes[2]]);
        if after.pc != target {
            return Err("JSR target mismatch");
        }
        self.check_active_slot_writes(record)?;
        let pushed = return_pc.wrapping_sub(1);
        self.require_pushes(record, before.sp, &[pushed.to_be_bytes()[0], pushed as u8])?;
        self.push_frame(Frame::call(
            before,
            after,
            target,
            return_pc,
            self.nrom.offset_for(before.pc).unwrap(),
            bytes,
        ))
    }

    fn push_interrupt(
        &mut self,
        before: State,
        after: State,
        record: &InstructionTraceRecord,
        trigger: InterruptTrigger,
    ) -> Result<(), &'static str> {
        let return_pc = match trigger {
            InterruptTrigger::External => before.pc,
            InterruptTrigger::Brk => {
                let bytes = record.instruction_bytes();
                if bytes.len() != 2 || bytes[0] != 0 {
                    return Err("BRK instruction mismatch");
                }
                before.pc.checked_add(2).ok_or("BRK return address wraps")?
            }
        };
        if after.sp != before.sp.wrapping_sub(3) {
            return Err("interrupt stack state mismatch");
        }
        self.check_active_slot_writes(record)?;
        let pushed_status = match trigger {
            InterruptTrigger::External => (before.p | 0x20) & !0x10,
            InterruptTrigger::Brk => before.p | 0x30,
        };
        let [high, low] = return_pc.to_be_bytes();
        self.require_pushes(record, before.sp, &[high, low, pushed_status])?;
        self.push_frame(Frame::interrupt(
            before,
            after,
            return_pc,
            trigger,
            record,
            self.nrom.offset_for(before.pc).unwrap(),
        ))
    }

    fn pop_frame(
        &mut self,
        before: State,
        after: State,
        record: &InstructionTraceRecord,
        kind: FrameKind,
        stack_bytes: u8,
    ) -> Result<(), &'static str> {
        if record.instruction_bytes().len() != 1 || !record.writes().is_empty() {
            return Err("return instruction has invalid trace writes");
        }
        let frame = self.frames.last().ok_or("return without active frame")?;
        if frame.kind != kind
            || after.sp != before.sp.wrapping_add(stack_bytes)
            || after.sp != frame.return_sp
            || after.pc != frame.return_pc
        {
            return Err("return does not match active frame");
        }
        self.frames.pop();
        Ok(())
    }

    fn push_frame(&mut self, frame: Frame) -> Result<(), &'static str> {
        if self.frames.len() == MAX_DEPTH {
            return Err("control-flow depth limit reached");
        }
        self.frames.push(frame);
        Ok(())
    }

    fn check_active_slot_writes(
        &self,
        record: &InstructionTraceRecord,
    ) -> Result<(), &'static str> {
        for write in record.writes() {
            let Some(address) = u16::try_from(write.address)
                .ok()
                .filter(|address| *address < 0x2000)
            else {
                continue;
            };
            let address = address & 0x07ff;
            if self
                .frames
                .iter()
                .any(|frame| frame.slots.contains(&address))
            {
                return Err("active return slot was overwritten");
            }
        }
        Ok(())
    }

    fn check_active_stack_pointer(
        &self,
        before: State,
        after: State,
        opcode: u8,
    ) -> Result<(), &'static str> {
        if self.frames.is_empty() || before.sp == after.sp {
            return Ok(());
        }
        match opcode {
            0x48 | 0x08 if after.sp == before.sp.wrapping_sub(1) => Ok(()),
            0x68 | 0x28 if after.sp == before.sp.wrapping_add(1) => {
                let popped = 0x0100 | u16::from(after.sp);
                (!self
                    .frames
                    .iter()
                    .any(|frame| frame.slots.contains(&popped)))
                .then_some(())
                .ok_or("active_return_slot_was_consumed")
            }
            _ => Err("active_stack_pointer_changed"),
        }
    }

    fn require_pushes(
        &self,
        record: &InstructionTraceRecord,
        sp: u8,
        values: &[u8],
    ) -> Result<(), &'static str> {
        let writes = record.writes();
        if writes.len() != values.len() {
            return Err("control stack write count mismatch");
        }
        for (index, (write, value)) in writes.iter().zip(values).enumerate() {
            let address = 0x0100 | u16::from(sp.wrapping_sub(index as u8));
            if write.address != u32::from(address)
                || write.new_value != u32::from(*value)
                || write.width != TraceWriteWidth::Byte
                || write.kind != TraceWriteKind::Memory
            {
                return Err("control stack write mismatch");
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameKind {
    Call,
    Interrupt,
}

#[derive(Clone, Copy)]
enum InterruptTrigger {
    External,
    Brk,
}

#[derive(Clone)]
struct Frame {
    kind: FrameKind,
    trigger: Option<InterruptTrigger>,
    call_pc: Option<u16>,
    source_offset: Option<u64>,
    instruction: Vec<u8>,
    target: u16,
    return_pc: u16,
    caller_before: State,
    entry: State,
    return_sp: u8,
    slots: Vec<u16>,
}

impl Frame {
    fn call(
        caller_before: State,
        entry: State,
        target: u16,
        return_pc: u16,
        source_offset: u64,
        instruction: &[u8],
    ) -> Self {
        Self {
            kind: FrameKind::Call,
            trigger: None,
            call_pc: Some(caller_before.pc),
            source_offset: Some(source_offset),
            instruction: instruction.to_vec(),
            target,
            return_pc,
            caller_before,
            entry,
            return_sp: caller_before.sp,
            slots: vec![
                0x0100 | u16::from(caller_before.sp),
                0x0100 | u16::from(caller_before.sp.wrapping_sub(1)),
            ],
        }
    }

    fn interrupt(
        caller_before: State,
        entry: State,
        return_pc: u16,
        trigger: InterruptTrigger,
        record: &InstructionTraceRecord,
        source_offset: u64,
    ) -> Self {
        let (call_pc, source_offset, instruction) = match trigger {
            InterruptTrigger::External => (None, None, Vec::new()),
            InterruptTrigger::Brk => (
                Some(caller_before.pc),
                Some(source_offset),
                record.instruction_bytes().to_vec(),
            ),
        };
        Self {
            kind: FrameKind::Interrupt,
            trigger: Some(trigger),
            call_pc,
            source_offset,
            instruction,
            target: entry.pc,
            return_pc,
            caller_before,
            entry,
            return_sp: caller_before.sp,
            slots: vec![
                0x0100 | u16::from(caller_before.sp),
                0x0100 | u16::from(caller_before.sp.wrapping_sub(1)),
                0x0100 | u16::from(caller_before.sp.wrapping_sub(2)),
            ],
        }
    }

    fn json(&self) -> Value {
        let kind = match self.kind {
            FrameKind::Call => "call",
            FrameKind::Interrupt => "interrupt",
        };
        let trigger = self.trigger.map(|trigger| match trigger {
            InterruptTrigger::External => "external",
            InterruptTrigger::Brk => "brk",
        });
        json!({
            "kind": kind,
            "trigger": trigger,
            "call_pc": self.call_pc,
            "instruction_source_offset": self.source_offset,
            "instruction": self.call_pc.map(|_| json!({
                "bytes": const_hex::encode(&self.instruction),
                "sha256": zeff_firmware::sha256_hex(&self.instruction),
            })),
            "target": self.target,
            "return_pc": self.return_pc,
            "caller_before": state_json(self.caller_before),
            "entry": state_json(self.entry),
        })
    }
}

struct Observation {
    event_index: usize,
    cycle: u64,
    pc: u16,
    register: u16,
    value: u8,
    writer_before: State,
    call_path: Vec<Frame>,
}

impl Observation {
    fn json(self) -> Value {
        json!({
            "event_index": self.event_index,
            "cycle": self.cycle,
            "pc": self.pc,
            "register": self.register,
            "value": self.value,
            "writer_before": state_json(self.writer_before),
            "call_path": self.call_path.iter().map(Frame::json).collect::<Vec<_>>(),
        })
    }
}

fn state_json(state: State) -> Value {
    json!({
        "pc": state.pc,
        "a": state.a,
        "x": state.x,
        "y": state.y,
        "sp": state.sp,
        "p": state.p,
        "cycle": state.cycle,
    })
}

fn states_match(left: State, right: State) -> bool {
    left.pc == right.pc
        && left.a == right.a
        && left.x == right.x
        && left.y == right.y
        && left.sp == right.sp
        && left.p == right.p
        && left.cycle == right.cycle
}

#[cfg(test)]
#[path = "control_flow/tests.rs"]
mod tests;
