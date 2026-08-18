use std::collections::HashMap;

use zeff_emu_common::debug::{InstructionTraceRecord, TraceExecMode};

use crate::symbols::{SymbolId, SymbolSession};
use crate::ui::InstructionTraceBatch;

use super::runtime_inference::{RuntimeInference, RuntimeSymbolCandidate};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ExecutionLocation {
    Rom(u64),
    Cpu { mode: u8, address: u32 },
}

#[derive(Default)]
pub(crate) struct ExecutionCoverage {
    location_hits: HashMap<ExecutionLocation, u64>,
    location_symbols: HashMap<ExecutionLocation, Box<[SymbolId]>>,
    symbol_hits: HashMap<SymbolId, u64>,
    runtime_inference: RuntimeInference,
    symbol_generation: u64,
    last_sequence: Option<u64>,
    revision: u64,
    total: u64,
}

impl ExecutionCoverage {
    pub(crate) fn sync_symbols(&mut self, symbols: &SymbolSession) {
        if self.symbol_generation != symbols.store.generation() {
            self.rebuild(symbols);
        }
        self.runtime_inference.prune(symbols);
    }

    pub(crate) fn merge(&mut self, batch: &InstructionTraceBatch, symbols: &SymbolSession) {
        if batch.retained == 0 && batch.newest_sequence.is_none() {
            self.clear();
            return;
        }
        if self.symbol_generation != symbols.store.generation() {
            self.rebuild(symbols);
        }
        if let (Some(last), Some(newest)) = (self.last_sequence, batch.newest_sequence)
            && newest < last
        {
            self.clear();
            self.symbol_generation = symbols.store.generation();
        }

        let mut changed = false;
        for entry in &batch.entries {
            if self
                .last_sequence
                .is_some_and(|sequence| entry.sequence <= sequence)
            {
                continue;
            }
            self.last_sequence = Some(entry.sequence);
            self.record(entry, symbols, 1);
            self.runtime_inference.observe(entry, symbols);
            changed = true;
        }
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    fn record(&mut self, entry: &InstructionTraceRecord, symbols: &SymbolSession, hits: u64) {
        let location = execution_location(entry);
        let location_count = self.location_hits.entry(location).or_default();
        *location_count = location_count.saturating_add(hits);
        self.total = self.total.saturating_add(hits);

        let ids = self.location_symbols.entry(location).or_insert_with(|| {
            symbols
                .execution_symbol_ids(
                    entry.physical_rom_offset,
                    u64::from(entry.pc),
                    trace_exec_mode(entry.mode),
                )
                .into_boxed_slice()
        });
        for id in ids.iter().copied() {
            let count = self.symbol_hits.entry(id).or_default();
            *count = count.saturating_add(hits);
        }
    }

    fn rebuild(&mut self, symbols: &SymbolSession) {
        self.location_symbols.clear();
        self.symbol_hits.clear();
        self.symbol_generation = symbols.store.generation();
        for (&location, &hits) in &self.location_hits {
            let (physical, address, mode) = location_parts(location);
            let ids = symbols
                .execution_symbol_ids(physical, address, mode)
                .into_boxed_slice();
            for id in ids.iter().copied() {
                let count = self.symbol_hits.entry(id).or_default();
                *count = count.saturating_add(hits);
            }
            self.location_symbols.insert(location, ids);
        }
        self.revision = self.revision.wrapping_add(1);
    }

    pub(crate) fn clear(&mut self) {
        let changed = !self.location_hits.is_empty() || self.last_sequence.is_some();
        self.location_hits.clear();
        self.location_symbols.clear();
        self.symbol_hits.clear();
        self.runtime_inference.clear();
        self.last_sequence = None;
        self.total = 0;
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub(crate) fn hits(&self, id: SymbolId) -> u64 {
        self.symbol_hits.get(&id).copied().unwrap_or_default()
    }

    pub(crate) fn executed_ids(&self) -> impl Iterator<Item = SymbolId> + '_ {
        self.symbol_hits.keys().copied()
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn total(&self) -> u64 {
        self.total
    }

    pub(crate) fn runtime_candidates(&self) -> impl Iterator<Item = &RuntimeSymbolCandidate> {
        self.runtime_inference.candidates().filter(|candidate| {
            candidate.calls >= 2
                && candidate.location.storage.is_some_and(|storage| {
                    self.location_hits
                        .contains_key(&ExecutionLocation::Rom(storage.offset))
                })
        })
    }
}

fn execution_location(entry: &InstructionTraceRecord) -> ExecutionLocation {
    entry.physical_rom_offset.map_or(
        ExecutionLocation::Cpu {
            mode: entry.mode as u8,
            address: entry.pc,
        },
        ExecutionLocation::Rom,
    )
}

fn location_parts(location: ExecutionLocation) -> (Option<u64>, u64, crate::symbols::ExecMode) {
    match location {
        ExecutionLocation::Rom(offset) => (Some(offset), 0, crate::symbols::ExecMode::Unknown),
        ExecutionLocation::Cpu { mode, address } => {
            (None, u64::from(address), stored_exec_mode(mode))
        }
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
    }
}

fn stored_exec_mode(mode: u8) -> crate::symbols::ExecMode {
    match mode {
        value if value == TraceExecMode::Sm83 as u8 => crate::symbols::ExecMode::Sm83,
        value if value == TraceExecMode::Arm as u8 => crate::symbols::ExecMode::Arm,
        value if value == TraceExecMode::Thumb as u8 => crate::symbols::ExecMode::Thumb,
        value if value == TraceExecMode::Mos6502 as u8 => crate::symbols::ExecMode::Mos6502,
        value if value == TraceExecMode::Z80 as u8 => crate::symbols::ExecMode::Z80,
        value if value == TraceExecMode::V30 as u8 => crate::symbols::ExecMode::V30,
        _ => crate::symbols::ExecMode::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::types::runtime_inference::{direct_call_target, mapped_target};
    use crate::symbols::{
        AddressSpaceId, Confidence, CpuLocation, ExecMode, ImageId, Provenance, ProvenanceKind,
        RegionId, StorageLocation, SymbolKind, SymbolLocation, SymbolRecord, SymbolScope,
    };
    use zeff_emu_common::debug::RegisterDelta;

    fn symbol(name: &str, offset: u64, size: u64) -> SymbolRecord {
        SymbolRecord {
            id: SymbolId(0),
            name: name.to_owned(),
            location: SymbolLocation {
                cpu: Some(CpuLocation {
                    space: AddressSpaceId(0),
                    address: 0x4000 + offset,
                }),
                storage: Some(StorageLocation {
                    image: ImageId(0),
                    region: RegionId(0),
                    offset,
                }),
                bank: None,
                exec_mode: ExecMode::Sm83,
            },
            value: None,
            size: Some(size),
            kind: SymbolKind::Function,
            scope: SymbolScope::Global,
            provenance: Provenance {
                kind: ProvenanceKind::Build,
                source: None,
            },
            confidence: Confidence::Exact,
            comment: None,
        }
    }

    fn batch(entries: Vec<InstructionTraceRecord>) -> InstructionTraceBatch {
        InstructionTraceBatch {
            enabled: true,
            capacity: 1_000,
            retained: entries.len(),
            oldest_sequence: entries.first().map(|entry| entry.sequence),
            newest_sequence: entries.last().map(|entry| entry.sequence),
            entries,
        }
    }

    fn gb_call(
        sequence: u64,
        pc: u32,
        offset: u64,
        target: u16,
        taken: bool,
    ) -> InstructionTraceRecord {
        let mut entry = InstructionTraceRecord::new(
            TraceExecMode::Sm83,
            pc,
            Some(offset),
            0,
            sequence,
            &[0xCD, target as u8, (target >> 8) as u8],
        );
        entry.sequence = sequence;
        entry.push_register_delta(RegisterDelta {
            register: 9,
            value: if taken { u32::from(target) } else { pc + 3 },
        });
        entry
    }

    fn gb_instruction(sequence: u64, pc: u32, offset: u64) -> InstructionTraceRecord {
        let mut entry =
            InstructionTraceRecord::new(TraceExecMode::Sm83, pc, Some(offset), 0, sequence, &[0]);
        entry.sequence = sequence;
        entry
    }

    #[test]
    fn counts_containing_symbols_and_ignores_duplicate_batches() {
        let mut symbols = SymbolSession::default();
        let id = symbols.store.insert(symbol("Main", 0x8000, 4));
        let mut first =
            InstructionTraceRecord::new(TraceExecMode::Sm83, 0x4000, Some(0x8000), 0, 0, &[0]);
        first.sequence = 1;
        let mut second = first;
        second.sequence = 2;
        second.pc += 1;
        second.physical_rom_offset = Some(0x8001);
        let trace = batch(vec![first, second]);

        let mut coverage = ExecutionCoverage::default();
        coverage.merge(&trace, &symbols);
        coverage.merge(&trace, &symbols);

        assert_eq!(coverage.hits(id), 2);
        assert_eq!(coverage.total(), 2);
    }

    #[test]
    fn rebuilds_symbol_hits_after_the_store_changes() {
        let mut symbols = SymbolSession::default();
        let mut entry =
            InstructionTraceRecord::new(TraceExecMode::Sm83, 0x4000, Some(0x8000), 0, 0, &[0]);
        entry.sequence = 1;
        let mut coverage = ExecutionCoverage::default();
        coverage.merge(&batch(vec![entry]), &symbols);

        let id = symbols.store.insert(symbol("Late", 0x8000, 1));
        coverage.sync_symbols(&symbols);

        assert_eq!(coverage.hits(id), 1);
    }

    #[test]
    fn discovers_repeated_taken_calls_at_executed_physical_targets() {
        let symbols = SymbolSession::default();
        let trace = batch(vec![
            gb_call(1, 0x4560, 0x8560, 0x4660, true),
            gb_instruction(2, 0x4660, 0x8660),
            gb_call(3, 0x4560, 0x8560, 0x4660, true),
            gb_instruction(4, 0x4660, 0x8660),
        ]);
        let mut coverage = ExecutionCoverage::default();

        coverage.merge(&trace, &symbols);

        let candidates = coverage.runtime_candidates().collect::<Vec<_>>();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].name, "sub_008660");
        assert_eq!(candidates[0].calls, 2);
        assert_eq!(candidates[0].location.storage.unwrap().offset, 0x8660);
    }

    #[test]
    fn ignores_untaken_and_bank_ambiguous_calls() {
        let symbols = SymbolSession::default();
        let trace = batch(vec![
            gb_call(1, 0x4560, 0x8560, 0x4660, false),
            gb_call(2, 0x0200, 0x0200, 0x4660, true),
            gb_call(3, 0x0200, 0x0200, 0x4660, true),
            gb_instruction(4, 0x4660, 0x8660),
        ]);
        let mut coverage = ExecutionCoverage::default();

        coverage.merge(&trace, &symbols);

        assert_eq!(coverage.runtime_candidates().count(), 0);
    }

    #[test]
    fn known_symbols_suppress_runtime_candidates() {
        let mut symbols = SymbolSession::default();
        symbols.store.insert(symbol("Known", 0x8660, 4));
        let trace = batch(vec![
            gb_call(1, 0x4560, 0x8560, 0x4660, true),
            gb_instruction(2, 0x4660, 0x8660),
            gb_call(3, 0x4560, 0x8560, 0x4660, true),
        ]);
        let mut coverage = ExecutionCoverage::default();

        coverage.merge(&trace, &symbols);

        assert_eq!(coverage.runtime_candidates().count(), 0);
    }

    #[test]
    fn resolves_direct_call_targets_across_traced_cpu_modes() {
        let mut nes = InstructionTraceRecord::new(
            TraceExecMode::Mos6502,
            0x8100,
            Some(0x0100),
            0,
            0,
            &[0x20, 0x10, 0x81],
        );
        nes.sequence = 1;
        assert_eq!(direct_call_target(&nes), Some(0x8110));
        assert_eq!(
            mapped_target(&nes, 0x8110).unwrap().storage.unwrap().offset,
            0x0110
        );

        let mut z80 = InstructionTraceRecord::new(
            TraceExecMode::Z80,
            0x4100,
            Some(0x8100),
            0,
            0,
            &[0xCD, 0x00, 0x42],
        );
        z80.push_register_delta(RegisterDelta {
            register: 11,
            value: 0x4200,
        });
        assert_eq!(direct_call_target(&z80), Some(0x4200));
        assert_eq!(
            mapped_target(&z80, 0x4200).unwrap().storage.unwrap().offset,
            0x8200
        );

        let mut arm = InstructionTraceRecord::new(
            TraceExecMode::Arm,
            0x0800_0100,
            Some(0x0100),
            0,
            0,
            &0xEB00_0006_u32.to_le_bytes(),
        );
        arm.push_register_delta(RegisterDelta {
            register: 15,
            value: 0x0800_0120,
        });
        assert_eq!(direct_call_target(&arm), Some(0x0800_0120));
        assert_eq!(
            mapped_target(&arm, 0x0800_0120)
                .unwrap()
                .storage
                .unwrap()
                .offset,
            0x0120
        );

        let mut thumb = InstructionTraceRecord::new(
            TraceExecMode::Thumb,
            0x0800_0102,
            Some(0x0102),
            0,
            0,
            &0xF800_u16.to_le_bytes(),
        );
        thumb.push_register_delta(RegisterDelta {
            register: 15,
            value: 0x0800_0200,
        });
        assert_eq!(direct_call_target(&thumb), Some(0x0800_0200));

        let v30 = InstructionTraceRecord::new(
            TraceExecMode::V30,
            0x20010,
            Some(0x10010),
            0,
            0,
            &[0xE8, 0x0D, 0x00],
        );
        assert_eq!(direct_call_target(&v30), Some(0x20020));
        assert_eq!(
            mapped_target(&v30, 0x20020)
                .unwrap()
                .storage
                .unwrap()
                .offset,
            0x10020
        );
    }
}
