use serde_json::{Value, json};
use zeff_emu_common::debug::{DebugEvent, InstructionTraceRecord};

use super::super::nes_source::Nrom;
use super::State;

const MAX_CALLER_INSTRUCTIONS: usize = 64;
const MAX_LINKS: usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq)]
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

    fn parse(value: &Value) -> Option<Self> {
        match value.as_str()? {
            "a" => Some(Self::A),
            "x" => Some(Self::X),
            "y" => Some(Self::Y),
            _ => None,
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
struct RamRead {
    pc: u16,
    cycle: u64,
    instruction: Vec<u8>,
    address: u16,
    value: u8,
    writer: Option<Value>,
}

#[derive(Clone)]
struct Transform {
    pc: u16,
    cycle: u64,
    input: u8,
    output: u8,
    carry: bool,
}

#[derive(Clone)]
struct Tag {
    read: RamRead,
    transforms: Vec<Transform>,
}

impl Tag {
    fn value(&self) -> u8 {
        self.transforms
            .last()
            .map_or(self.read.value, |transform| transform.output)
    }
}

#[derive(Clone)]
struct Witness {
    pc: u16,
    cycle: u64,
    bytes: Vec<u8>,
    source_offset: Option<u64>,
}

impl Witness {
    fn json(&self) -> Value {
        json!({
            "pc": self.pc,
            "cpu_cycle": self.cycle,
            "bytes": const_hex::encode(&self.bytes),
            "source_offset": self.source_offset,
        })
    }
}

struct Window {
    entry: State,
    a: Option<Tag>,
    x: Option<Tag>,
    y: Option<Tag>,
    previous: State,
    witnesses: Vec<Witness>,
}

impl Window {
    fn start(entry: State) -> Self {
        Self {
            entry,
            a: None,
            x: None,
            y: None,
            previous: entry,
            witnesses: Vec::with_capacity(MAX_CALLER_INSTRUCTIONS),
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

    fn witness(&mut self, before: State, bytes: &[u8], source_offset: Option<u64>) {
        self.witnesses.push(Witness {
            pc: before.pc,
            cycle: before.cycle,
            bytes: bytes.to_vec(),
            source_offset,
        });
    }
}

#[derive(Clone)]
struct Binding {
    entry_pc: u16,
    entry_cycle: u64,
    register: Register,
    value: u8,
    caller_entry: State,
    call_pc: u16,
    call_source_offset: Option<u64>,
    tag: Tag,
    witnesses: Vec<Witness>,
}

pub(super) struct Tracker {
    nrom: Nrom,
    window: Option<Window>,
    incoming: Vec<Binding>,
    links: Vec<Value>,
    writers: super::ram_writer::Tracker,
}

impl Tracker {
    pub(super) fn new(source: &[u8]) -> Result<Self, &'static str> {
        Ok(Self {
            nrom: Nrom::parse(source).ok_or("unsupported_nrom_header")?,
            window: None,
            incoming: Vec::new(),
            links: Vec::new(),
            writers: super::ram_writer::Tracker::new(),
        })
    }

    pub(super) fn step(
        &mut self,
        before: State,
        after: State,
        record: Option<&InstructionTraceRecord>,
        new_argument_links: &[Value],
        first_link_index: usize,
    ) -> Result<(), &'static str> {
        self.writers.step(before, after, record);
        let Some(record) = record else {
            self.clear();
            return Ok(());
        };
        if self
            .window
            .as_ref()
            .is_some_and(|window| !states_match(window.previous, before))
        {
            return Err("caller_flow_noncontiguous_state");
        }
        if record.event == Some(DebugEvent::Interrupt) {
            self.clear();
            return Ok(());
        }
        let bytes = record.instruction_bytes();
        let Some(&opcode) = bytes.first() else {
            self.clear();
            return Ok(());
        };
        if opcode == 0x20 {
            self.start_child(before, after, bytes)?;
        } else if matches!(opcode, 0x00 | 0x40 | 0x60) {
            self.clear();
        } else {
            self.advance(before, after, bytes)?;
        }
        self.join(new_argument_links, first_link_index)?;
        if self.window.is_none() {
            self.incoming.clear();
        }
        Ok(())
    }

    pub(super) fn finish(self) -> Value {
        json!(self.links)
    }

    fn clear(&mut self) {
        self.window = None;
        self.incoming.clear();
    }

    fn start_child(
        &mut self,
        before: State,
        after: State,
        bytes: &[u8],
    ) -> Result<(), &'static str> {
        let [opcode, lo, hi] = bytes else {
            return Err("invalid_caller_flow_jsr");
        };
        if *opcode != 0x20
            || after.pc != u16::from_le_bytes([*lo, *hi])
            || after.sp != before.sp.wrapping_sub(2)
        {
            return Err("caller_flow_call_entry_mismatch");
        }
        let mut bindings = Vec::new();
        if let Some(mut window) = self.window.take()
            && window.witnesses.len() < MAX_CALLER_INSTRUCTIONS
        {
            window.witness(before, bytes, self.nrom.offset_for(before.pc));
            for register in [Register::A, Register::X, Register::Y] {
                if let Some(tag) = window.tag(register).clone() {
                    if tag.value() != register.value(before)
                        || register.value(after) != register.value(before)
                    {
                        return Err("caller_flow_tag_value_mismatch");
                    }
                    bindings.push(Binding {
                        entry_pc: after.pc,
                        entry_cycle: after.cycle,
                        register,
                        value: register.value(after),
                        caller_entry: window.entry,
                        call_pc: before.pc,
                        call_source_offset: self.nrom.offset_for(before.pc),
                        tag,
                        witnesses: window.witnesses.clone(),
                    });
                }
            }
        }
        self.incoming = bindings;
        self.window = Some(Window::start(after));
        Ok(())
    }

    fn advance(&mut self, before: State, after: State, bytes: &[u8]) -> Result<(), &'static str> {
        let Some(mut window) = self.window.take() else {
            return Ok(());
        };
        if window.witnesses.len() == MAX_CALLER_INSTRUCTIONS {
            return Ok(());
        }
        window.witness(before, bytes, self.nrom.offset_for(before.pc));
        if !self.apply(&mut window, before, after, bytes)? {
            self.incoming.clear();
            return Ok(());
        }
        if window.witnesses.len() < MAX_CALLER_INSTRUCTIONS {
            window.previous = after;
            self.window = Some(window);
        }
        Ok(())
    }

    fn apply(
        &self,
        window: &mut Window,
        before: State,
        after: State,
        bytes: &[u8],
    ) -> Result<bool, &'static str> {
        match bytes[0] {
            0xaa => self.transfer(window, Register::A, Register::X, before, after)?,
            0xa8 => self.transfer(window, Register::A, Register::Y, before, after)?,
            0x8a => self.transfer(window, Register::X, Register::A, before, after)?,
            0x98 => self.transfer(window, Register::Y, Register::A, before, after)?,
            0x0a => self.asl_a(window, before, after)?,
            0xa5 | 0xad | 0xa6 | 0xae | 0xa4 | 0xac => {
                self.direct_read(window, before, after, bytes)?
            }
            0xa9 | 0xb5 | 0xbd | 0xb9 | 0xa1 | 0xb1 | 0x68 => window.set_tag(Register::A, None),
            0xa2 | 0xb6 | 0xbe | 0xba => window.set_tag(Register::X, None),
            0xa0 | 0xb4 | 0xbc => window.set_tag(Register::Y, None),
            0xe8 | 0xca => window.set_tag(Register::X, None),
            0xc8 | 0x88 => window.set_tag(Register::Y, None),
            0x09 | 0x05 | 0x15 | 0x0d | 0x1d | 0x19 | 0x01 | 0x11 | 0x29 | 0x25 | 0x35 | 0x2d
            | 0x3d | 0x39 | 0x21 | 0x31 | 0x2a | 0x4a | 0x6a => window.set_tag(Register::A, None),
            0x84 | 0x94 | 0x8c | 0x85 | 0x95 | 0x8d | 0x9d | 0x99 | 0x81 | 0x91 | 0x86 | 0x96
            | 0x8e | 0x10 | 0x30 | 0x50 | 0x70 | 0x90 | 0xb0 | 0xd0 | 0xf0 | 0x4c | 0x6c | 0xea
            | 0x18 | 0x38 | 0x58 | 0x78 | 0xb8 | 0xd8 | 0xf8 | 0xc9 | 0xc5 | 0xd5 | 0xcd | 0xdd
            | 0xd9 | 0xc1 | 0xd1 | 0xe0 | 0xe4 | 0xec | 0xc0 | 0xc4 | 0xcc | 0x24 | 0x2c | 0xe6
            | 0xf6 | 0xee | 0xfe | 0xc6 | 0xd6 | 0xce | 0xde | 0x48 | 0x08 | 0x28 | 0x9a => {}
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
            return Err("caller_flow_transfer_mismatch");
        }
        let tag = window.tag(from).clone();
        if tag
            .as_ref()
            .is_some_and(|tag| tag.value() != from.value(before))
        {
            return Err("caller_flow_tag_value_mismatch");
        }
        window.set_tag(to, tag);
        Ok(())
    }

    fn asl_a(&self, window: &mut Window, before: State, after: State) -> Result<(), &'static str> {
        let Some(mut tag) = window.tag(Register::A).clone() else {
            return Ok(());
        };
        if tag.value() != before.a {
            return Err("caller_flow_tag_value_mismatch");
        }
        let output = before.a.wrapping_shl(1);
        let carry = before.a & 0x80 != 0;
        if after.a != output || (after.p & 1 != 0) != carry {
            return Err("caller_flow_asl_mismatch");
        }
        tag.transforms.push(Transform {
            pc: before.pc,
            cycle: before.cycle,
            input: before.a,
            output,
            carry,
        });
        window.set_tag(Register::A, Some(tag));
        Ok(())
    }

    fn direct_read(
        &self,
        window: &mut Window,
        before: State,
        after: State,
        bytes: &[u8],
    ) -> Result<(), &'static str> {
        let (register, address) = direct_load(bytes).ok_or("invalid_caller_flow_load")?;
        if address > 0x1fff {
            window.set_tag(register, None);
            return Ok(());
        }
        window.set_tag(
            register,
            Some(Tag {
                read: RamRead {
                    pc: before.pc,
                    cycle: before.cycle,
                    instruction: bytes.to_vec(),
                    address,
                    value: register.value(after),
                    writer: self.writers.snapshot(address, register.value(after)),
                },
                transforms: Vec::new(),
            }),
        );
        Ok(())
    }

    fn join(&mut self, links: &[Value], first_link_index: usize) -> Result<(), &'static str> {
        for (offset, link) in links.iter().enumerate() {
            let entry_pc = value_u16(&link["entry"]["pc"])?;
            let entry_cycle = value_u64(&link["entry"]["cpu_cycle"])?;
            let register = Register::parse(&link["argument"]["register"])
                .ok_or("invalid_entry_argument_link")?;
            let value = value_u8(&link["argument"]["value"])?;
            let event_index = value_u64(&link["event_index"])?;
            let entry_argument_read_index = first_link_index
                .checked_add(offset)
                .ok_or("caller_flow_link_index_overflow")?;
            for binding in &self.incoming {
                if binding.entry_pc != entry_pc
                    || binding.entry_cycle != entry_cycle
                    || binding.register != register
                    || binding.value != value
                {
                    continue;
                }
                if self.links.len() == MAX_LINKS {
                    return Err("caller_flow_link_limit");
                }
                self.links
                    .push(binding.json(entry_argument_read_index, event_index));
            }
        }
        Ok(())
    }
}

impl Binding {
    fn json(&self, entry_argument_read_index: usize, event_index: u64) -> Value {
        json!({
            "entry_argument_read_index": entry_argument_read_index,
            "event_index": event_index,
            "callee_entry": {"pc": self.entry_pc, "cpu_cycle": self.entry_cycle},
            "argument": {"register": self.register.name(), "value": self.value},
            "caller_entry": {"pc": self.caller_entry.pc, "cpu_cycle": self.caller_entry.cycle},
            "call": {"pc": self.call_pc, "source_offset": self.call_source_offset},
            "ram_read": {
                "pc": self.tag.read.pc,
                "cpu_cycle": self.tag.read.cycle,
                "instruction": const_hex::encode(&self.tag.read.instruction),
                "address": self.tag.read.address,
                "canonical_address": self.tag.read.address & 0x07ff,
                "value": self.tag.read.value,
            },
            "ram_writer": self.tag.read.writer,
            "transforms": self.tag.transforms.iter().map(|transform| json!({
                "operation": "asl_a",
                "pc": transform.pc,
                "cpu_cycle": transform.cycle,
                "input": transform.input,
                "output": transform.output,
                "carry": transform.carry,
            })).collect::<Vec<_>>(),
            "witnesses": self.witnesses.iter().map(Witness::json).collect::<Vec<_>>(),
        })
    }
}

fn direct_load(bytes: &[u8]) -> Option<(Register, u16)> {
    match bytes {
        [0xa5, address] => Some((Register::A, u16::from(*address))),
        [0xad, lo, hi] => Some((Register::A, u16::from_le_bytes([*lo, *hi]))),
        [0xa6, address] => Some((Register::X, u16::from(*address))),
        [0xae, lo, hi] => Some((Register::X, u16::from_le_bytes([*lo, *hi]))),
        [0xa4, address] => Some((Register::Y, u16::from(*address))),
        [0xac, lo, hi] => Some((Register::Y, u16::from_le_bytes([*lo, *hi]))),
        _ => None,
    }
}

fn value_u64(value: &Value) -> Result<u64, &'static str> {
    value.as_u64().ok_or("invalid_entry_argument_link")
}

fn value_u16(value: &Value) -> Result<u16, &'static str> {
    u16::try_from(value_u64(value)?).map_err(|_| "invalid_entry_argument_link")
}

fn value_u8(value: &Value) -> Result<u8, &'static str> {
    u8::try_from(value_u64(value)?).map_err(|_| "invalid_entry_argument_link")
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
#[path = "caller_flow/tests.rs"]
mod tests;
