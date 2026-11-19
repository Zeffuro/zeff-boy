use crate::reference::{Case, Phase, Transfer};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use zeff_m68k::bus::{AccessKind, BUS_ADDRESS_MASK, BusAccess, ByteLanes, Direction};
use zeff_m68k::timing::physical::{EntryBus, EntryOutcome, EntryReport};
use zeff_m68k::timing::physical_instruction::{
    InstructionBus, InstructionOutcome, InstructionReport, PhysicalInstructionStep,
};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu};

const EXPORTS: [&str; 2] = [
    "ddedf7c475aef47ba2eb1d753f89c140cc098392f2b15bf4f2e9a8d0a0c35a9d",
    "377a86ff9662cd37be872897e39d8aace5d299abfacb84165029c36afa7e1607",
];
const SOURCE_COMMIT: &str = "64b253116a3de04aaac4346c43680960dc9b67e5";

fn memory(bytes: &[(u32, u8)]) -> Result<BTreeMap<u32, u8>, String> {
    let mut result = BTreeMap::new();
    for &(address, value) in bytes {
        if address > BUS_ADDRESS_MASK || result.insert(address, value).is_some() {
            return Err("duplicate or invalid RAM address".into());
        }
    }
    Ok(result)
}

struct Device<'a> {
    memory: BTreeMap<u32, u8>,
    phases: &'a [Phase],
    cursor: usize,
    clock: u64,
    error: Option<String>,
}

impl Device<'_> {
    fn transfer(
        &mut self,
        access: BusAccess,
        direction: Direction,
        start: CpuClocks,
    ) -> Result<Transfer, String> {
        let expected = match (self.phases.get(self.cursor), direction) {
            (Some(Phase::Read(value)), Direction::Read)
            | (Some(Phase::Write(value)), Direction::Write) => value,
            _ => return Err("unexpected transfer order".into()),
        };
        let lanes = match access.lanes {
            ByteLanes::Upper => (1, 0),
            ByteLanes::Lower => (0, 1),
            ByteLanes::Both => (1, 1),
        };
        if start.0 != self.clock
            || self.error.is_some()
            || expected.duration_cycles != 4
            || access.bus_address != access.logical_address & BUS_ADDRESS_MASK & !1
            || access.bus_address != expected.address_bus
            || access.function_code as u32 != expected.function_code
            || lanes != (expected.uds, expected.lds)
            || access.kind
                != if expected.function_code & 1 == 0 {
                    AccessKind::Prefetch
                } else {
                    AccessKind::Data
                }
        {
            return Err("transfer fields or start clock differ".into());
        }
        let value = expected.clone();
        self.cursor += 1;
        self.clock += 4;
        Ok(value)
    }

    fn read(&mut self, access: BusAccess, start: CpuClocks) -> Result<u16, String> {
        let expected = self.transfer(access, Direction::Read, start)?;
        let mut value = 0;
        if expected.uds != 0 {
            value |= u16::from(
                *self
                    .memory
                    .get(&access.bus_address)
                    .ok_or("unobserved upper read")?,
            ) << 8;
        }
        if expected.lds != 0 {
            value |= u16::from(
                *self
                    .memory
                    .get(&(access.bus_address + 1))
                    .ok_or("unobserved lower read")?,
            );
        }
        if u32::from(value) != expected.data_bus {
            return Err("actual RAM read differs from original".into());
        }
        Ok(value)
    }

    fn write(&mut self, access: BusAccess, value: u16, start: CpuClocks) -> Result<(), String> {
        let expected = self.transfer(access, Direction::Write, start)?;
        if u32::from(value) != expected.data_bus {
            return Err("actual write payload differs from original".into());
        }
        if expected.uds != 0 {
            self.memory.insert(access.bus_address, (value >> 8) as u8);
        }
        if expected.lds != 0 {
            self.memory.insert(access.bus_address + 1, value as u8);
        }
        Ok(())
    }
}

impl EntryBus for Device<'_> {
    type Error = String;
    fn read_entry(&mut self, _: BusAccess, _: CpuClocks) -> EntryReport<u16, String> {
        self.error = Some("unexpected entry read".into());
        EntryReport {
            elapsed: 0,
            outcome: EntryOutcome::HostFailure("unexpected entry".into()),
        }
    }
    fn write_entry(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> EntryReport<(), String> {
        self.error = Some("unexpected entry write".into());
        EntryReport {
            elapsed: 0,
            outcome: EntryOutcome::HostFailure("unexpected entry".into()),
        }
    }
    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        if start.0 != self.clock
            || !matches!(self.phases.get(self.cursor),
            Some(Phase::Idle { duration_cycles }) if *duration_cycles == clocks)
        {
            self.error = Some("idle order, duration or start differs".into());
            return;
        }
        self.cursor += 1;
        self.clock += u64::from(clocks);
    }
}

impl InstructionBus for Device<'_> {
    fn read_instruction(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> InstructionReport<u16, String> {
        match self.read(access, start) {
            Ok(value) => InstructionReport {
                elapsed: 4,
                outcome: InstructionOutcome::Complete(value),
            },
            Err(error) => InstructionReport {
                elapsed: 0,
                outcome: InstructionOutcome::HostFailure(error),
            },
        }
    }
    fn write_instruction(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> InstructionReport<(), String> {
        match self.write(access, value, start) {
            Ok(()) => InstructionReport {
                elapsed: 4,
                outcome: InstructionOutcome::Complete(()),
            },
            Err(error) => InstructionReport {
                elapsed: 0,
                outcome: InstructionOutcome::HostFailure(error),
            },
        }
    }
}

fn selected(word: u16) -> bool {
    let (size, mode, reg) = (word >> 6 & 3, word >> 3 & 7, word & 7);
    matches!(word & 0xff00, 0x4200 | 0x4600)
        && size < 3
        && (size == 0 || mode == 0)
        && (mode == 0 || matches!(mode, 2..=6) || mode == 7 && reg <= 1)
}

pub fn verify(case: &Case) -> Result<(), String> {
    if !selected(case.initial.prefetch[0])
        || case.initial.sr & 0x8000 != 0
        || case.normalization_exclusion.is_some()
    {
        return Err("case outside CLR/NOT remainder completion domain".into());
    }
    let mut cpu = PrefetchCpu::from_prefetched(
        case.initial.registers()?,
        case.initial.prefetch,
        CpuClocks(0),
    )
    .ok_or("odd initial PC")?;
    let mut bus = Device {
        memory: memory(&case.initial.ram)?,
        phases: &case.transactions,
        cursor: 0,
        clock: 0,
        error: None,
    };
    let result = cpu
        .step_physical_instruction(&mut bus)
        .map_err(|error| format!("physical CLR/NOT: {error:?}"))?;
    let clocks = u64::from(case.reference_total_cycles);
    if case
        .transactions
        .iter()
        .map(|phase| u64::from(phase.duration()))
        .sum::<u64>()
        != clocks
    {
        return Err("original phase duration sum differs".into());
    }
    let expected = PrefetchCpu::from_prefetched(
        case.expected.registers()?,
        case.expected.prefetch,
        CpuClocks(clocks),
    )
    .ok_or("odd final PC")?;
    if !matches!(result, PhysicalInstructionStep::Instruction(step)
        if step.opcode == case.initial.prefetch[0] && step.exception.is_none() && step.clocks == clocks)
        || cpu != expected
        || bus.clock != clocks
        || bus.cursor != bus.phases.len()
        || bus.error.is_some()
        || bus.memory != memory(&case.expected.ram)?
    {
        return Err("whole CPU/RAM/ordered transfers/clocks/consumption differs".into());
    }
    Ok(())
}

pub fn verify_exports(texts: [&str; 2]) -> Result<u32, String> {
    let mut original = [0u32; 6];
    let mut completed = [0u32; 6];
    let mut excluded = [0u32; 6];
    let mut old = [0u32; 6];
    let mut keys = BTreeSet::new();
    let mut words = BTreeSet::new();
    for (index, text) in texts.into_iter().enumerate() {
        let digest: String = Sha256::digest(text.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if digest != EXPORTS[index] {
            return Err("original export digest differs".into());
        }
        let mut header = false;
        for line in text.lines() {
            let mut raw: serde_json::Value =
                serde_json::from_str(line).map_err(|e| e.to_string())?;
            if raw.get("type").and_then(|v| v.as_str()) == Some("header") {
                if header
                    || raw.get("source_commit").and_then(|v| v.as_str()) != Some(SOURCE_COMMIT)
                {
                    return Err("original header differs".into());
                }
                header = true;
                continue;
            }
            let cohort = match raw.get("cohort").and_then(|v| v.as_str()) {
                Some("CLR.b") => 0,
                Some("CLR.w") => 1,
                Some("CLR.l") => 2,
                Some("NOT.b") => 3,
                Some("NOT.w") => 4,
                Some("NOT.l") => 5,
                _ => continue,
            };
            if index != cohort / 3 || raw.get("type").and_then(|v| v.as_str()) != Some("case") {
                return Err("original cohort or record type differs".into());
            }
            raw.as_object_mut().ok_or("invalid case")?.remove("type");
            let case: Case = serde_json::from_value(raw).map_err(|e| e.to_string())?;
            if !keys.insert((case.cohort.clone(), case.name.clone())) {
                return Err("duplicate case".into());
            }
            original[cohort] += 1;
            if !selected(case.initial.prefetch[0]) {
                old[cohort] += 1;
                continue;
            }
            if case.initial.sr & 0x8000 != 0 {
                if case.normalization_exclusion.as_deref() != Some("trace") {
                    return Err("old trace tag differs".into());
                }
                excluded[cohort] += 1;
                continue;
            }
            verify(&case).map_err(|error| format!("{}/{}: {error}", case.cohort, case.name))?;
            completed[cohort] += 1;
            words.insert(case.initial.prefetch[0]);
        }
        if !header {
            return Err("missing original header".into());
        }
    }
    if original != [2500; 6]
        || completed != [1250, 202, 197, 1231, 192, 186]
        || excluded != [1250, 181, 202, 1269, 197, 219]
        || old != [0, 2117, 2101, 0, 2111, 2095]
        || words.len() != 132
    {
        return Err("original CLR/NOT partition differs".into());
    }
    Ok(completed.into_iter().sum())
}

#[cfg(test)]
#[path = "remainder/tests.rs"]
mod tests;
