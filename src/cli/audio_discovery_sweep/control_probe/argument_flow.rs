use serde_json::{Value, json};
use zeff_emu_common::audio_trace::{AudioTraceEvent, NesTraceWrite};
use zeff_emu_common::debug::{DebugEvent, InstructionTraceRecord};

use super::super::nes_source::Nrom;
use super::State;

const MAX_CALLEE_INSTRUCTIONS: usize = 64;
const MAX_LINKS: usize = 4096;

#[derive(Clone, Copy)]
enum Register {
    A,
    X,
    Y,
}

impl Register {
    fn name(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::X => "x",
            Self::Y => "y",
        }
    }

    fn value(self, state: State) -> u8 {
        match self {
            Self::A => state.a,
            Self::X => state.x,
            Self::Y => state.y,
        }
    }
}

#[derive(Clone)]
struct EntryArgument {
    register: Register,
    value: u8,
}

#[derive(Clone)]
struct RomRead {
    argument: EntryArgument,
    pc: u16,
    cycle: u64,
    instruction: Vec<u8>,
    index_register: Register,
    index_value: u8,
    address: u16,
    source_offset: u64,
    value: u8,
}

#[derive(Clone)]
enum Tag {
    Entry(EntryArgument),
    RomRead(RomRead),
}

impl Tag {
    fn value(&self) -> u8 {
        match self {
            Self::Entry(argument) => argument.value,
            Self::RomRead(read) => read.value,
        }
    }
}

#[derive(Clone)]
struct Witness {
    pc: u16,
    cycle: u64,
    bytes: Vec<u8>,
    source_offset: Option<u64>,
}

struct Window {
    call_pc: u16,
    call_source_offset: Option<u64>,
    entry_pc: u16,
    entry_cycle: u64,
    a: Option<Tag>,
    x: Option<Tag>,
    y: Option<Tag>,
    previous: State,
    witnesses: Vec<Witness>,
}

impl Window {
    fn start(call: State, entry: State, source_offset: Option<u64>) -> Self {
        Self {
            call_pc: call.pc,
            call_source_offset: source_offset,
            entry_pc: entry.pc,
            entry_cycle: entry.cycle,
            a: Some(Tag::Entry(EntryArgument {
                register: Register::A,
                value: entry.a,
            })),
            x: Some(Tag::Entry(EntryArgument {
                register: Register::X,
                value: entry.x,
            })),
            y: Some(Tag::Entry(EntryArgument {
                register: Register::Y,
                value: entry.y,
            })),
            previous: entry,
            witnesses: Vec::with_capacity(MAX_CALLEE_INSTRUCTIONS),
        }
    }

    fn tag(&self, register: Register) -> &Option<Tag> {
        match register {
            Register::A => &self.a,
            Register::X => &self.x,
            Register::Y => &self.y,
        }
    }

    fn set_tag(&mut self, register: Register, tag: Option<Tag>) {
        match register {
            Register::A => self.a = tag,
            Register::X => self.x = tag,
            Register::Y => self.y = tag,
        }
    }

    fn has_tags(&self) -> bool {
        self.a.is_some() || self.x.is_some() || self.y.is_some()
    }

    fn witness(&mut self, before: State, bytes: &[u8], source_offset: Option<u64>) {
        self.witnesses.push(Witness {
            pc: before.pc,
            cycle: before.cycle,
            bytes: bytes.to_vec(),
            source_offset,
        });
    }
}

pub(super) struct Tracker<'a> {
    source: &'a [u8],
    nrom: Nrom,
    window: Option<Window>,
    links: Vec<Value>,
}

impl<'a> Tracker<'a> {
    pub(super) fn new(source: &'a [u8]) -> Result<Self, &'static str> {
        Ok(Self {
            source,
            nrom: Nrom::parse(source).ok_or("unsupported_nrom_header")?,
            window: None,
            links: Vec::new(),
        })
    }

    pub(super) fn step(
        &mut self,
        before: State,
        after: State,
        record: Option<&InstructionTraceRecord>,
        writers: &[(usize, AudioTraceEvent<NesTraceWrite>)],
    ) -> Result<(), &'static str> {
        let Some(record) = record else {
            self.window = None;
            return Ok(());
        };
        if self
            .window
            .as_ref()
            .is_some_and(|window| !states_match(window.previous, before))
        {
            return Err("argument_flow_noncontiguous_state");
        }
        if record.event == Some(DebugEvent::Interrupt) {
            self.window = None;
            return Ok(());
        }
        let bytes = record.instruction_bytes();
        let Some(&opcode) = bytes.first() else {
            self.window = None;
            return Ok(());
        };
        if opcode == 0x20 {
            self.start_call(before, after, bytes)?;
            return Ok(());
        }
        if matches!(opcode, 0x00 | 0x40 | 0x60) {
            self.window = None;
            return Ok(());
        }
        let Some(mut window) = self.window.take() else {
            return Ok(());
        };
        if window.witnesses.len() == MAX_CALLEE_INSTRUCTIONS {
            return Ok(());
        }
        let supported = self.apply(&mut window, before, after, bytes, writers)?;
        if !supported || !window.has_tags() {
            return Ok(());
        }
        window.previous = after;
        if window.witnesses.len() == MAX_CALLEE_INSTRUCTIONS {
            return Ok(());
        }
        self.window = Some(window);
        Ok(())
    }

    pub(super) fn finish(self) -> Value {
        json!(self.links)
    }

    pub(super) fn links(&self) -> &[Value] {
        &self.links
    }

    fn start_call(
        &mut self,
        before: State,
        after: State,
        bytes: &[u8],
    ) -> Result<(), &'static str> {
        let [_, lo, hi] = *bytes else {
            return Err("invalid_jsr_instruction");
        };
        let target = u16::from_le_bytes([lo, hi]);
        if after.pc != target || after.sp != before.sp.wrapping_sub(2) {
            return Err("argument_flow_call_entry_mismatch");
        }
        self.window = Some(Window::start(
            before,
            after,
            self.nrom.offset_for(before.pc),
        ));
        Ok(())
    }

    fn apply(
        &mut self,
        window: &mut Window,
        before: State,
        after: State,
        bytes: &[u8],
        writers: &[(usize, AudioTraceEvent<NesTraceWrite>)],
    ) -> Result<bool, &'static str> {
        window.witness(before, bytes, self.nrom.offset_for(before.pc));
        let opcode = bytes[0];
        match opcode {
            0xaa => self.transfer(window, Register::A, Register::X, before, after)?,
            0xa8 => self.transfer(window, Register::A, Register::Y, before, after)?,
            0x8a => self.transfer(window, Register::X, Register::A, before, after)?,
            0x98 => self.transfer(window, Register::Y, Register::A, before, after)?,
            0xbd | 0xb9 => self.indexed_read(window, before, after, bytes)?,
            0xa9 | 0xa5 | 0xb5 | 0xad | 0xa1 | 0xb1 | 0x68 => window.set_tag(Register::A, None),
            0xa2 | 0xa6 | 0xb6 | 0xae | 0xbe | 0xba => window.set_tag(Register::X, None),
            0xa0 | 0xa4 | 0xb4 | 0xac | 0xbc => window.set_tag(Register::Y, None),
            0xe8 | 0xca => window.set_tag(Register::X, None),
            0xc8 | 0x88 => window.set_tag(Register::Y, None),
            0x09 | 0x05 | 0x15 | 0x0d | 0x1d | 0x19 | 0x01 | 0x11 | 0x29 | 0x25 | 0x35 | 0x2d
            | 0x3d | 0x39 | 0x21 | 0x31 => window.set_tag(Register::A, None),
            0x0a | 0x2a | 0x4a | 0x6a => window.set_tag(Register::A, None),
            0x84 | 0x94 | 0x8c | 0x85 | 0x95 | 0x8d | 0x9d | 0x99 | 0x81 | 0x91 | 0x86 | 0x96
            | 0x8e => self.store(window, before, bytes, writers)?,
            0x10 | 0x30 | 0x50 | 0x70 | 0x90 | 0xb0 | 0xd0 | 0xf0 | 0x4c | 0x6c | 0xea | 0x18
            | 0x38 | 0x58 | 0x78 | 0xb8 | 0xd8 | 0xf8 | 0xc9 | 0xc5 | 0xd5 | 0xcd | 0xdd | 0xd9
            | 0xc1 | 0xd1 | 0xe0 | 0xe4 | 0xec | 0xc0 | 0xc4 | 0xcc | 0x24 | 0x2c | 0xe6 | 0xf6
            | 0xee | 0xfe | 0xc6 | 0xd6 | 0xce | 0xde | 0x48 | 0x08 | 0x28 | 0x9a => {}
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn transfer(
        &self,
        window: &mut Window,
        from: Register,
        to: Register,
        before: State,
        after: State,
    ) -> Result<(), &'static str> {
        if to.value(after) != from.value(before) {
            return Err("argument_flow_transfer_mismatch");
        }
        if window
            .tag(from)
            .as_ref()
            .is_some_and(|tag| tag.value() != from.value(before))
        {
            return Err("argument_flow_tag_value_mismatch");
        }
        let tag = window.tag(from).clone();
        window.set_tag(to, tag);
        Ok(())
    }

    fn indexed_read(
        &self,
        window: &mut Window,
        before: State,
        after: State,
        bytes: &[u8],
    ) -> Result<(), &'static str> {
        let [opcode, lo, hi] = *bytes else {
            return Err("invalid_indexed_load_instruction");
        };
        let index_register = if opcode == 0xbd {
            Register::X
        } else {
            Register::Y
        };
        let index_value = index_register.value(before);
        let Some(Tag::Entry(argument)) = window.tag(index_register).clone() else {
            window.set_tag(Register::A, None);
            return Ok(());
        };
        if argument.value != index_value {
            return Err("argument_flow_tag_value_mismatch");
        }
        let address = u16::from_le_bytes([lo, hi]).wrapping_add(u16::from(index_value));
        let Some(source_offset) = self.nrom.offset_for(address) else {
            window.set_tag(Register::A, None);
            return Ok(());
        };
        let value = *self
            .source
            .get(source_offset as usize)
            .ok_or("argument_flow_read_source_mismatch")?;
        if after.a != value {
            return Err("argument_flow_indexed_load_mismatch");
        }
        window.set_tag(
            Register::A,
            Some(Tag::RomRead(RomRead {
                argument,
                pc: before.pc,
                cycle: before.cycle,
                instruction: bytes.to_vec(),
                index_register,
                index_value,
                address,
                source_offset,
                value,
            })),
        );
        Ok(())
    }

    fn store(
        &mut self,
        window: &Window,
        before: State,
        bytes: &[u8],
        writers: &[(usize, AudioTraceEvent<NesTraceWrite>)],
    ) -> Result<(), &'static str> {
        let Some((register, address)) = store_destination(before, bytes) else {
            return Ok(());
        };
        let Some(Tag::RomRead(read)) = window.tag(register) else {
            return Ok(());
        };
        if read.value != register.value(before) {
            return Err("argument_flow_tag_value_mismatch");
        }
        for (event_index, event) in writers {
            let NesTraceWrite::Register {
                address: written_address,
                value,
                ..
            } = event.write
            else {
                return Err("argument_flow_non_register_writer");
            };
            if written_address != address || value != read.value {
                return Err("argument_flow_store_mismatch");
            }
            if self.links.len() == MAX_LINKS {
                return Err("argument_flow_link_limit");
            }
            self.links.push(json!({
                "call": {"pc": window.call_pc, "source_offset": window.call_source_offset},
                "entry": {"pc": window.entry_pc, "cpu_cycle": window.entry_cycle},
                "argument": {"register": read.argument.register.name(), "value": read.argument.value},
                "rom_read": {
                    "pc": read.pc, "cpu_cycle": read.cycle,
                    "instruction": const_hex::encode(&read.instruction),
                    "index_register": read.index_register.name(), "index_value": read.index_value,
                    "address": read.address, "source_offset": read.source_offset, "value": read.value,
                },
                "witnesses": window.witnesses.iter().map(Witness::json).collect::<Vec<_>>(),
                "event_index": event_index, "write_trace_cycle": event.cycle,
                "writer_pc": event.pc, "register": written_address,
            }));
        }
        Ok(())
    }
}

impl Witness {
    fn json(&self) -> Value {
        json!({
            "pc": self.pc, "cpu_cycle": self.cycle,
            "bytes": const_hex::encode(&self.bytes), "source_offset": self.source_offset,
        })
    }
}

fn store_destination(before: State, bytes: &[u8]) -> Option<(Register, u16)> {
    let [opcode, lo, hi] = *bytes else {
        return None;
    };
    let base = u16::from_le_bytes([lo, hi]);
    match opcode {
        0x8c => Some((Register::Y, base)),
        0x8d => Some((Register::A, base)),
        0x8e => Some((Register::X, base)),
        0x9d => Some((Register::A, base.wrapping_add(u16::from(before.x)))),
        0x99 => Some((Register::A, base.wrapping_add(u16::from(before.y)))),
        _ => None,
    }
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
#[path = "argument_flow/tests.rs"]
mod tests;
