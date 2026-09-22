use serde_json::{Value, json};
use zeff_emu_common::debug::{DebugEvent, InstructionTraceRecord, TraceWriteKind, TraceWriteWidth};

use super::State;

const MAX_PATH: usize = 64;
const MAX_VALUES: usize = 16;

#[derive(Clone)]
struct Witness {
    pc: u16,
    cycle: u64,
    offset: u64,
    bytes: Vec<u8>,
}

impl Witness {
    fn json(&self) -> Value {
        json!({"pc": self.pc, "cpu_cycle": self.cycle,
            "source_offset": self.offset, "instruction": const_hex::encode(&self.bytes)})
    }
}

#[derive(Clone)]
struct Constraint {
    kind: &'static str,
    values: Vec<u8>,
    path: Vec<Witness>,
}

#[derive(Clone)]
struct Writer {
    witness: Witness,
    end_cycle: u64,
    address: u16,
    value: u8,
    constraint: Option<Constraint>,
}

pub(super) struct Tracker {
    cells: Vec<Option<Writer>>,
    registers: [Option<Constraint>; 3],
}

impl Tracker {
    pub(super) fn new() -> Self {
        Self {
            cells: vec![None; 2048],
            registers: [None, None, None],
        }
    }

    // Recorder authenticates each instruction and its complete write trace before this step.
    pub(super) fn step(
        &mut self,
        before: State,
        after: State,
        record: Option<&InstructionTraceRecord>,
    ) {
        let Some(record) = record else {
            self.cells.fill(None);
            self.registers.fill(None);
            return;
        };
        if record.write_overflow != 0 {
            self.cells.fill(None);
            self.registers.fill(None);
            return;
        }
        let bytes = record.instruction_bytes();
        let witness = record
            .physical_rom_offset
            .and_then(|offset| offset.checked_add(16))
            .map(|offset| Witness {
                pc: before.pc,
                cycle: before.cycle,
                offset,
                bytes: bytes.to_vec(),
            });
        let store = direct_store(bytes, before);
        for write in record.writes() {
            if write.width != TraceWriteWidth::Byte {
                self.cells.fill(None);
                self.registers.fill(None);
                return;
            }
            if write.address >= 0x2000 {
                continue;
            }
            let cell = &mut self.cells[(write.address & 0x7ff) as usize];
            *cell = None;
            if record.event.is_some() || write.kind != TraceWriteKind::Memory {
                continue;
            }
            if let Some((register, address, value)) = store
                && write.address == u32::from(address)
                && write.new_value == u32::from(value)
                && let Some(witness) = witness.clone()
            {
                let constraint = self.registers[register]
                    .clone()
                    .filter(|constraint| {
                        constraint.path.len() < MAX_PATH && constraint.values.contains(&value)
                    })
                    .map(|mut constraint| {
                        constraint.path.push(witness.clone());
                        constraint
                    });
                *cell = Some(Writer {
                    witness,
                    end_cycle: after.cycle,
                    address,
                    value,
                    constraint,
                });
            }
        }
        if record.event == Some(DebugEvent::Interrupt) || witness.is_none() {
            self.registers.fill(None);
            return;
        }
        self.advance(before, after, bytes, witness.unwrap());
    }

    pub(super) fn snapshot(&self, address: u16, value: u8) -> Option<Value> {
        let writer = self.cells.get(usize::from(address & 0x7ff))?.as_ref()?;
        if writer.value != value {
            return None;
        }
        let mut result = writer.witness.json();
        result["end_cpu_cycle"] = json!(writer.end_cycle);
        result["address"] = json!(writer.address);
        result["canonical_address"] = json!(writer.address & 0x7ff);
        result["value"] = json!(value);
        result["value_constraint"] = writer
            .constraint
            .as_ref()
            .map_or(Value::Null, |constraint| {
                json!({"scope": "writer_path_only", "kind": constraint.kind,
                "values": constraint.values, "max_instructions": MAX_PATH,
                "witnesses": constraint.path.iter().map(Witness::json).collect::<Vec<_>>()})
            });
        Some(result)
    }

    fn advance(&mut self, before: State, after: State, bytes: &[u8], witness: Witness) {
        for tag in &mut self.registers {
            if let Some(constraint) = tag {
                constraint.path.push(witness.clone());
                if constraint.path.len() >= MAX_PATH {
                    *tag = None;
                }
            }
        }
        match bytes {
            [0x29, mask] if after.a == before.a & mask => {
                let values: Vec<u8> = (0..=u8::MAX).filter(|value| value & !mask == 0).collect();
                self.registers[0] = (values.len() <= MAX_VALUES).then_some(Constraint {
                    kind: "immediate_and_mask",
                    values,
                    path: vec![witness],
                });
            }
            [opcode @ (0xa9 | 0xa2 | 0xa0), value] => {
                let register = match opcode {
                    0xa9 => 0,
                    0xa2 => 1,
                    _ => 2,
                };
                self.registers[register] =
                    (values(after)[register] == *value).then_some(Constraint {
                        kind: "immediate_constant",
                        values: vec![*value],
                        path: vec![witness],
                    });
            }
            [0xaa] => self.transfer(0, 1, before, after),
            [0xa8] => self.transfer(0, 2, before, after),
            [0x8a] => self.transfer(1, 0, before, after),
            [0x98] => self.transfer(2, 0, before, after),
            [0xa5 | 0xad | 0xb5 | 0xbd | 0xb9 | 0xa1 | 0xb1 | 0x68, ..] => self.registers[0] = None,
            [0xa6 | 0xae | 0xb6 | 0xbe | 0xba | 0xe8 | 0xca, ..] => self.registers[1] = None,
            [0xa4 | 0xac | 0xb4 | 0xbc | 0xc8 | 0x88, ..] => self.registers[2] = None,
            [
                0x84 | 0x94 | 0x8c | 0x85 | 0x95 | 0x8d | 0x9d | 0x99 | 0x81 | 0x91 | 0x86 | 0x96
                | 0x8e | 0x10 | 0x30 | 0x50 | 0x70 | 0x90 | 0xb0 | 0xd0 | 0xf0 | 0x4c | 0xea | 0x18
                | 0x38 | 0x58 | 0x78 | 0xb8 | 0xd8 | 0xf8 | 0xc9 | 0xe0 | 0xc0 | 0x20,
                ..,
            ] => {}
            _ => self.registers.fill(None),
        }
        for (index, tag) in self.registers.iter_mut().enumerate() {
            if tag
                .as_ref()
                .is_some_and(|constraint| !constraint.values.contains(&values(after)[index]))
            {
                *tag = None;
            }
        }
    }

    fn transfer(&mut self, from: usize, to: usize, before: State, after: State) {
        self.registers[to] = if values(before)[from] == values(after)[to] {
            self.registers[from].clone()
        } else {
            None
        };
    }
}

fn values(state: State) -> [u8; 3] {
    [state.a, state.x, state.y]
}

fn direct_store(bytes: &[u8], state: State) -> Option<(usize, u16, u8)> {
    let (opcode, address) = match bytes {
        [opcode @ 0x84..=0x86, address] => (*opcode, u16::from(*address)),
        [opcode @ 0x8c..=0x8e, lo, hi] => (*opcode, u16::from_le_bytes([*lo, *hi])),
        _ => return None,
    };
    let register = match opcode {
        0x85 | 0x8d => 0,
        0x86 | 0x8e => 1,
        _ => 2,
    };
    Some((register, address, values(state)[register]))
}

#[cfg(test)]
#[path = "ram_writer/tests.rs"]
mod tests;
