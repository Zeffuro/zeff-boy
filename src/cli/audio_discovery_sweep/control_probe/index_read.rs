use serde_json::{Value, json};
use zeff_emu_common::audio_trace::{AudioTraceEvent, NesTraceWrite};
use zeff_emu_common::debug::InstructionTraceRecord;

use super::super::nes_source::Nrom;
use super::State;

enum Pending {
    Call { pc: u16, entry: State },
    Load { link: Value, value: u8, next: State },
}

pub(super) struct Tracker<'a> {
    source: &'a [u8],
    nrom: Nrom,
    pending: Option<Pending>,
    links: Vec<Value>,
}

impl<'a> Tracker<'a> {
    pub(super) fn new(source: &'a [u8]) -> Result<Self, &'static str> {
        Ok(Self {
            source,
            nrom: Nrom::parse(source).ok_or("unsupported_nrom_header")?,
            pending: None,
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
        let previous = self.pending.take();
        let Some(record) = record else {
            return Ok(());
        };
        let instruction = record.instruction_bytes();
        if matches!(instruction, [0x20, _, _]) {
            self.pending = Some(Pending::Call {
                pc: before.pc,
                entry: after,
            });
            return Ok(());
        }
        match previous {
            Some(Pending::Call { pc, entry })
                if before.pc == entry.pc && before.cycle == entry.cycle =>
            {
                let [opcode @ (0xbd | 0xb9), lo, hi] = instruction else {
                    return Ok(());
                };
                let (register, index) = if *opcode == 0xbd {
                    ("x", entry.x)
                } else {
                    ("y", entry.y)
                };
                let base = u16::from_le_bytes([*lo, *hi]);
                let address = base.wrapping_add(u16::from(index));
                let Some(offset) = self.nrom.offset_for(address) else {
                    return Ok(());
                };
                let value = self.source[offset as usize];
                if after.a != value || before.sp != entry.sp || after.sp != entry.sp {
                    return Err("indexed_load_mismatch");
                }
                self.pending = Some(Pending::Load {
                    value,
                    next: after,
                    link: json!({
                        "call_pc": pc, "call_source_offset": self.nrom.offset_for(pc),
                        "entry_pc": entry.pc, "index_register": register, "entry_index": index,
                        "load_pc": before.pc, "load_cpu_cycle": before.cycle,
                        "instruction": const_hex::encode(instruction),
                        "table_base": base, "read_address": address,
                        "read_source_offset": offset, "value": value,
                    }),
                });
            }
            Some(Pending::Load { link, value, next })
                if before.pc == next.pc && before.cycle == next.cycle =>
            {
                if !matches!(instruction, [0x8d | 0x9d | 0x99, _, _]) || before.a != value {
                    return Ok(());
                }
                for (index, event) in writers {
                    let NesTraceWrite::Register {
                        address,
                        value: written,
                        ..
                    } = event.write
                    else {
                        continue;
                    };
                    if written != value {
                        return Err("indexed_store_mismatch");
                    }
                    if self.links.len() == 4096 {
                        return Err("observation_limit");
                    }
                    let mut observed = link.clone();
                    observed["event_index"] = json!(index);
                    observed["write_trace_cycle"] = json!(event.cycle);
                    observed["writer_pc"] = json!(event.pc);
                    observed["register"] = json!(address);
                    self.links.push(observed);
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn finish(self) -> Value {
        json!(self.links)
    }
}
