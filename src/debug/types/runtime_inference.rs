use std::collections::HashMap;

use zeff_emu_common::debug::{InstructionTraceRecord, TraceExecMode};

use crate::symbols::{
    AddressSpaceId, Confidence, CpuLocation, ImageId, ProvenanceKind, RegionId, StorageLocation,
    SymbolLocation, SymbolSession,
};

#[derive(Clone, Debug)]
pub(crate) struct RuntimeSymbolCandidate {
    pub(crate) name: String,
    pub(crate) location: SymbolLocation,
    pub(crate) calls: u64,
    pub(crate) provenance: ProvenanceKind,
    pub(crate) confidence: Confidence,
}

#[derive(Default)]
pub(super) struct RuntimeInference {
    candidates: HashMap<u64, RuntimeSymbolCandidate>,
}

impl RuntimeInference {
    pub(super) fn observe(&mut self, entry: &InstructionTraceRecord, symbols: &SymbolSession) {
        let Some(target) = direct_call_target(entry) else {
            return;
        };
        let Some(location) = mapped_target(entry, target) else {
            return;
        };
        if symbols.has_code_symbol(location) {
            return;
        }
        let offset = location.storage.expect("mapped target").offset;
        let candidate = self
            .candidates
            .entry(offset)
            .or_insert_with(|| RuntimeSymbolCandidate {
                name: format!("sub_{offset:06X}"),
                location,
                calls: 0,
                provenance: ProvenanceKind::RuntimeInference,
                confidence: Confidence::Low,
            });
        candidate.calls = candidate.calls.saturating_add(1);
    }

    pub(super) fn prune(&mut self, symbols: &SymbolSession) {
        self.candidates
            .retain(|_, candidate| !symbols.has_code_symbol(candidate.location));
    }

    pub(super) fn clear(&mut self) {
        self.candidates.clear();
    }

    pub(super) fn candidates(&self) -> impl Iterator<Item = &RuntimeSymbolCandidate> {
        self.candidates.values()
    }
}

pub(super) fn direct_call_target(entry: &InstructionTraceRecord) -> Option<u32> {
    let bytes = entry
        .instruction
        .get(..usize::from(entry.instruction_len))?;
    let opcode = *bytes.first()?;
    match entry.mode {
        TraceExecMode::Sm83 => {
            let target = match opcode {
                0xC7 => 0x00,
                0xCF => 0x08,
                0xD7 => 0x10,
                0xDF => 0x18,
                0xE7 => 0x20,
                0xEF => 0x28,
                0xF7 => 0x30,
                0xFF => 0x38,
                0xC4 | 0xCC | 0xCD | 0xD4 | 0xDC => {
                    u32::from(u16::from_le_bytes([*bytes.get(1)?, *bytes.get(2)?]))
                }
                _ => return None,
            };
            call_was_taken(entry, 9, target).then_some(target)
        }
        TraceExecMode::Mos6502 => {
            if opcode != 0x20 {
                return None;
            }
            Some(u32::from(u16::from_le_bytes([
                *bytes.get(1)?,
                *bytes.get(2)?,
            ])))
        }
        TraceExecMode::HuC6280 => {
            let target = match opcode {
                0x20 => u32::from(u16::from_le_bytes([*bytes.get(1)?, *bytes.get(2)?])),
                0x44 => u32::from(
                    (entry.pc as u16)
                        .wrapping_add(2)
                        .wrapping_add_signed(i16::from(*bytes.get(1)? as i8)),
                ),
                _ => return None,
            };
            call_was_taken(entry, 4, target).then_some(target)
        }
        TraceExecMode::Z80 => {
            let target = match opcode {
                0xC7 => 0x00,
                0xCF => 0x08,
                0xD7 => 0x10,
                0xDF => 0x18,
                0xE7 => 0x20,
                0xEF => 0x28,
                0xF7 => 0x30,
                0xFF => 0x38,
                0xC4 | 0xCC | 0xCD | 0xD4 | 0xDC | 0xE4 | 0xEC | 0xF4 | 0xFC => {
                    u32::from(u16::from_le_bytes([*bytes.get(1)?, *bytes.get(2)?]))
                }
                _ => return None,
            };
            call_was_taken(entry, 11, target).then_some(target)
        }
        TraceExecMode::Arm => {
            let raw = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?);
            if raw & 0x0F00_0000 != 0x0B00_0000 {
                return None;
            }
            let displacement = ((raw & 0x00FF_FFFF) << 2) as i32;
            let displacement = (displacement << 6) >> 6;
            let target = entry.pc.wrapping_add(8).wrapping_add_signed(displacement);
            call_was_taken(entry, 15, target).then_some(target)
        }
        TraceExecMode::Thumb => {
            let raw = u16::from_le_bytes(bytes.get(..2)?.try_into().ok()?);
            if raw & 0xF800 != 0xF800 {
                return None;
            }
            changed_register(entry, 15)
        }
        TraceExecMode::V30 => v30_call_target(entry, bytes),
    }
}

fn changed_register(entry: &InstructionTraceRecord, register: u8) -> Option<u32> {
    entry
        .register_deltas()
        .iter()
        .find(|delta| delta.register == register)
        .map(|delta| delta.value)
}

fn call_was_taken(entry: &InstructionTraceRecord, pc_register: u8, target: u32) -> bool {
    changed_register(entry, pc_register) == Some(target)
}

fn v30_call_target(entry: &InstructionTraceRecord, bytes: &[u8]) -> Option<u32> {
    let mut prefix_len = 0;
    while bytes.get(prefix_len).is_some_and(|opcode| {
        matches!(
            opcode,
            0x26 | 0x2E | 0x36 | 0x3E | 0x64 | 0x65 | 0x66 | 0x67 | 0xF0 | 0xF2 | 0xF3
        )
    }) {
        prefix_len += 1;
    }
    match *bytes.get(prefix_len)? {
        0xE8 => {
            let displacement = i32::from(i16::from_le_bytes([
                *bytes.get(prefix_len + 1)?,
                *bytes.get(prefix_len + 2)?,
            ]));
            Some(
                entry
                    .pc
                    .wrapping_add((prefix_len + 3) as u32)
                    .wrapping_add_signed(displacement)
                    & 0x000F_FFFF,
            )
        }
        0x9A => {
            let offset = u32::from(u16::from_le_bytes([
                *bytes.get(prefix_len + 1)?,
                *bytes.get(prefix_len + 2)?,
            ]));
            let segment = u32::from(u16::from_le_bytes([
                *bytes.get(prefix_len + 3)?,
                *bytes.get(prefix_len + 4)?,
            ]));
            Some(((segment << 4).wrapping_add(offset)) & 0x000F_FFFF)
        }
        _ => None,
    }
}

pub(super) fn mapped_target(entry: &InstructionTraceRecord, target: u32) -> Option<SymbolLocation> {
    let source = entry.physical_rom_offset?;
    let offset = match entry.mode {
        TraceExecMode::Sm83 if target < 0x4000 => u64::from(target),
        TraceExecMode::Sm83 => same_window_offset(entry.pc, source, target, 0x4000)?,
        TraceExecMode::Mos6502 => same_window_offset(entry.pc, source, target, 0x2000)?,
        TraceExecMode::HuC6280 => same_window_offset(entry.pc, source, target, 0x2000)?,
        TraceExecMode::Z80 => same_window_offset(entry.pc, source, target, 0x2000)?,
        TraceExecMode::Arm | TraceExecMode::Thumb => gba_rom_offset(target)?,
        TraceExecMode::V30 => same_window_offset(entry.pc, source, target, 0x1_0000)?,
    };
    Some(SymbolLocation {
        cpu: Some(CpuLocation {
            space: AddressSpaceId(0),
            address: u64::from(target),
        }),
        storage: Some(StorageLocation {
            image: ImageId(0),
            region: RegionId(0),
            offset,
        }),
        bank: match entry.mode {
            TraceExecMode::Sm83 => Some((offset / 0x4000) as u32),
            TraceExecMode::HuC6280 => entry.bank,
            _ => None,
        },
        exec_mode: trace_exec_mode(entry.mode),
    })
}

fn same_window_offset(pc: u32, source: u64, target: u32, size: u32) -> Option<u64> {
    if pc / size != target / size {
        return None;
    }
    source.checked_add_signed(i64::from(target) - i64::from(pc))
}

fn gba_rom_offset(address: u32) -> Option<u64> {
    match address {
        0x0800_0000..=0x09FF_FFFF => Some(u64::from(address - 0x0800_0000)),
        0x0A00_0000..=0x0BFF_FFFF => Some(u64::from(address - 0x0A00_0000)),
        0x0C00_0000..=0x0DFF_FFFF => Some(u64::from(address - 0x0C00_0000)),
        _ => None,
    }
}

fn trace_exec_mode(mode: TraceExecMode) -> crate::symbols::ExecMode {
    match mode {
        TraceExecMode::Sm83 => crate::symbols::ExecMode::Sm83,
        TraceExecMode::Arm => crate::symbols::ExecMode::Arm,
        TraceExecMode::Thumb => crate::symbols::ExecMode::Thumb,
        TraceExecMode::Mos6502 => crate::symbols::ExecMode::Mos6502,
        TraceExecMode::Z80 => crate::symbols::ExecMode::Z80,
        TraceExecMode::V30 => crate::symbols::ExecMode::V30,
        TraceExecMode::HuC6280 => crate::symbols::ExecMode::HuC6280,
    }
}
