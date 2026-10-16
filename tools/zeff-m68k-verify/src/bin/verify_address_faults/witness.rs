use std::collections::BTreeMap;

use super::profile::Prefix;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction};
use zeff_m68k::timing::physical::{
    EntryBus, EntryOutcome, EntryReport, PhysicalEntryState, PhysicalInspection,
};
use zeff_m68k::timing::physical_instruction::{
    InstructionBus, InstructionFault, InstructionOutcome, InstructionReport,
    PhysicalInstructionStep,
};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu};
use zeff_m68k_verify::reference::{Case, Phase, Transfer};

struct WitnessBus<'a> {
    memory: BTreeMap<u32, u8>,
    phases: &'a [Phase],
    profile: super::profile::Profile,
    cursor: usize,
    elapsed: u64,
    idle_remaining: u32,
    error: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum WitnessResult {
    Frame,
    CaptureGap,
}

pub(super) fn validate_profile(case: &Case) -> Result<(), String> {
    if !super::intake::selected_status_memory(case)
        && !super::intake::selected(case)
        && !super::intake::selected_tst(case)
        && !super::intake::selected_cmp(case)
        && !super::intake::selected_cmpi(case)
        && !super::intake::selected_cmpa(case)
        && !super::intake::selected_address_arithmetic(case)
        && !super::intake::selected_data_arithmetic(case)
    {
        return Err("unselected address-fault profile".into());
    }
    let p = super::profile::Profile::new(case)?;
    let m = p.marker;
    if p.operand & 1 == 0 || p.initial.pc & 1 != 0 || case.transactions.len() != m + 14 {
        return Err("selected profile requires first odd operand and complete entry suffix".into());
    }
    for (prefix, actual) in p.prefix.iter().zip(&case.transactions) {
        let valid = match (prefix, actual) {
            (Prefix::Idle(expected), Phase::Idle { duration_cycles }) => {
                expected == duration_cycles
            }
            (prefix, Phase::Read(t)) if prefix.read_access().is_some() => {
                let (address, _, program) = prefix.read_access().unwrap();
                t.duration_cycles == 4
                    && t.address_bus == address & 0x00ff_fffe
                    && t.function_code
                        == match (p.initial.supervisor(), program) {
                            (false, false) => 1,
                            (true, false) => 5,
                            (false, true) => 2,
                            (true, true) => 6,
                        }
                    && (t.uds, t.lds) == (1, 1)
            }
            _ => false,
        };
        if !valid {
            return Err("selected prefix order differs".into());
        }
    }
    let marker = match (&case.transactions[m], p.direction) {
        (Phase::ReadAddressError(value), Direction::Read)
        | (Phase::WriteAddressError(value), Direction::Write) => value,
        _ => return Err("selected marker direction differs".into()),
    };
    if marker.duration_cycles != 4
        || marker.address_bus != p.operand & 0x00ff_fffe
        || marker.function_code
            != match (p.initial.supervisor(), p.operand_program) {
                (false, false) => 1,
                (true, false) => 5,
                (false, true) => 2,
                (true, true) => 6,
            }
        || (marker.uds, marker.lds) != (1, 1)
        || !matches!(case.transactions[m + 1], Phase::Idle { duration_cycles: 8 })
        || !case.transactions[m + 2..m + 9]
            .iter()
            .all(|p| matches!(p, Phase::Write(_)))
        || !case.transactions[m + 9..m + 12]
            .iter()
            .all(|p| matches!(p, Phase::Read(_)))
        || !matches!(
            case.transactions[m + 12],
            Phase::Idle { duration_cycles: 2 }
        )
        || !matches!(case.transactions[m + 13], Phase::Read(_))
    {
        return Err("selected marker or complete entry phase order differs".into());
    }
    Ok(())
}

impl WitnessBus<'_> {
    fn fields(&self, access: BusAccess, transfer: &Transfer) -> bool {
        let lanes = match access.lanes {
            ByteLanes::Upper => (1, 0),
            ByteLanes::Lower => (0, 1),
            ByteLanes::Both => (1, 1),
        };
        access.bus_address == transfer.address_bus
            && access.function_code as u32 == transfer.function_code
            && lanes == (transfer.uds, transfer.lds)
    }

    fn transfer(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        write: bool,
    ) -> Result<Transfer, String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if start.0 != self.elapsed || self.idle_remaining != 0 {
            return Err("entry transfer start/idle boundary differs".into());
        }
        let transfer = match (self.phases.get(self.cursor), write) {
            (Some(Phase::Write(value)), true) | (Some(Phase::Read(value)), false) => value.clone(),
            _ => {
                return Err(format!(
                    "unexpected entry transfer at phase {}",
                    self.cursor
                ));
            }
        };
        let suffix = self
            .cursor
            .checked_sub(self.profile.marker)
            .ok_or("entry before marker")?;
        let permitted = match suffix {
            2..=8 => {
                access.kind == AccessKind::Stack
                    && write
                    && access.logical_address
                        == self
                            .profile
                            .phase
                            .ssp
                            .wrapping_sub([2, 6, 4, 8, 10, 14, 12][suffix - 2])
            }
            9 | 10 => {
                access.kind == AccessKind::Vector
                    && !write
                    && access.logical_address == if suffix == 9 { 12 } else { 14 }
            }
            11 | 13 => access.kind == AccessKind::Prefetch && !write,
            _ => false,
        };
        if !permitted || !self.fields(access, &transfer) || transfer.duration_cycles < 4 {
            return Err(format!(
                "entry transfer fields differ at phase {}",
                self.cursor
            ));
        }
        self.cursor += 1;
        self.elapsed += u64::from(transfer.duration_cycles);
        Ok(transfer)
    }

    fn read(&mut self, access: BusAccess, start: CpuClocks) -> Result<(u16, u32), String> {
        let transfer = self.transfer(access, start, false)?;
        self.sample(access, &transfer)
            .map(|value| (value, transfer.duration_cycles))
    }

    fn sample(&self, access: BusAccess, transfer: &Transfer) -> Result<u16, String> {
        let high = self
            .memory
            .get(&access.bus_address)
            .ok_or("unobserved read high lane")?;
        let low = self
            .memory
            .get(&(access.bus_address + 1))
            .ok_or("unobserved read low lane")?;
        let value = (u16::from(*high) << 8) | u16::from(*low);
        if u32::from(value) != transfer.data_bus {
            return Err("entry read data bus differs".into());
        }
        Ok(value)
    }

    fn write(&mut self, access: BusAccess, value: u16, start: CpuClocks) -> Result<u32, String> {
        let transfer = self.transfer(access, start, true)?;
        if u32::from(value) != transfer.data_bus {
            return Err("entry write data bus differs".into());
        }
        self.memory.insert(access.bus_address, (value >> 8) as u8);
        self.memory.insert(access.bus_address + 1, value as u8);
        Ok(transfer.duration_cycles)
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) -> Result<(), String> {
        if start.0 != self.elapsed {
            return Err("entry idle start differs".into());
        }
        if self.idle_remaining == 0 {
            match self.phases.get(self.cursor) {
                Some(Phase::Idle { duration_cycles }) => self.idle_remaining = *duration_cycles,
                _ => return Err("entry idle crosses transfer".into()),
            }
        }
        if clocks == 0 || clocks > self.idle_remaining {
            return Err("entry idle duration differs".into());
        }
        self.idle_remaining -= clocks;
        self.elapsed += u64::from(clocks);
        if self.idle_remaining == 0 {
            self.cursor += 1;
        }
        Ok(())
    }
}

impl EntryBus for WitnessBus<'_> {
    type Error = String;

    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, String> {
        match self.read(access, start) {
            Ok((value, elapsed)) => EntryReport {
                elapsed,
                outcome: EntryOutcome::Complete(value),
            },
            Err(error) => EntryReport {
                elapsed: 0,
                outcome: EntryOutcome::HostFailure(error),
            },
        }
    }

    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> EntryReport<(), String> {
        match self.write(access, value, start) {
            Ok(elapsed) => EntryReport {
                elapsed,
                outcome: EntryOutcome::Complete(()),
            },
            Err(error) => EntryReport {
                elapsed: 0,
                outcome: EntryOutcome::HostFailure(error),
            },
        }
    }

    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        if self.error.is_none()
            && let Err(error) = self.idle(start, clocks)
        {
            self.error = Some(error);
        }
    }
}

impl InstructionBus for WitnessBus<'_> {
    fn read_instruction(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> InstructionReport<u16, String> {
        let result = (|| {
            let (expected, kind, _) = self
                .profile
                .prefix
                .get(self.cursor)
                .and_then(Prefix::read_access)
                .ok_or("unexpected operand device read")?;
            if start.0 != self.elapsed
                || self.idle_remaining != 0
                || access.kind != kind
                || access.logical_address != expected
            {
                return Err("unexpected operand device read".into());
            }
            let transfer = match &self.phases[self.cursor] {
                Phase::Read(t) => t,
                _ => unreachable!(),
            };
            if !self.fields(access, transfer) {
                return Err("prefix refill fields differ".into());
            }
            let value = self.sample(access, transfer)?;
            let elapsed = transfer.duration_cycles;
            self.cursor += 1;
            self.elapsed += u64::from(elapsed);
            Ok((value, elapsed))
        })();
        match result {
            Ok((value, elapsed)) => InstructionReport {
                elapsed,
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
        _access: BusAccess,
        _value: u16,
        _start: CpuClocks,
    ) -> InstructionReport<(), String> {
        InstructionReport {
            elapsed: 0,
            outcome: InstructionOutcome::HostFailure("unexpected operand device write".into()),
        }
    }

    fn detect_instruction_address_error(
        &mut self,
        access: BusAccess,
        direction: Direction,
        start: CpuClocks,
    ) -> InstructionReport<(), String> {
        let marker = match &self.phases[self.profile.marker] {
            Phase::ReadAddressError(value) | Phase::WriteAddressError(value) => value,
            _ => unreachable!(),
        };
        if self.cursor != self.profile.marker
            || start.0 != self.profile.prefix_clocks
            || self.elapsed != self.profile.prefix_clocks
            || access.logical_address != self.profile.operand
            || access.kind != AccessKind::Data
            || direction != self.profile.direction
            || !self.fields(access, marker)
            || self
                .profile
                .marker_word
                .is_some_and(|word| u32::from(word) != marker.data_bus)
        {
            return InstructionReport {
                elapsed: 0,
                outcome: InstructionOutcome::HostFailure("address detector fields differ".into()),
            };
        }
        // Raw marker4 + idle8 becomes supplied detection8 + entry idle4.
        self.cursor = self.profile.marker + 1;
        self.idle_remaining = 4;
        self.elapsed += 8;
        InstructionReport {
            elapsed: 8,
            outcome: InstructionOutcome::AtGroup0Entry,
        }
    }
}

fn memory_map(bytes: &[(u32, u8)]) -> Result<BTreeMap<u32, u8>, String> {
    let mut map = BTreeMap::new();
    for &(address, value) in bytes {
        if address > 0x00ff_ffff || map.insert(address, value).is_some() {
            return Err("duplicate or out-of-range RAM witness".into());
        }
    }
    Ok(map)
}

pub(super) fn run(case: &Case) -> Result<WitnessResult, String> {
    validate_profile(case)?;
    let profile = super::profile::Profile::new(case)?;
    let registers = profile.initial;
    let memory = memory_map(&case.initial.ram)?;
    let expected = memory_map(&case.expected.ram)?;
    if memory.keys().any(|address| !expected.contains_key(address)) {
        return Err("final RAM omits initial witness".into());
    }
    let mut cpu = PrefetchCpu::from_prefetched(registers, case.initial.prefetch, CpuClocks(0))
        .ok_or("odd initial architectural PC")?;
    let mut bus = WitnessBus {
        memory,
        phases: &case.transactions,
        profile,
        cursor: 0,
        elapsed: 0,
        idle_remaining: 0,
        error: None,
    };
    let result = cpu.step_physical_instruction(&mut bus);
    if !bus.profile.capture_known {
        let elapsed = bus.profile.prefix_clocks + 8;
        if !matches!(result, Err(InstructionFault::CaptureUnavailable { .. }))
            || cpu.registers != bus.profile.phase
            || cpu.queued_words().is_some()
            || cpu.clock().0 != elapsed
            || bus.elapsed != elapsed
            || bus.cursor != bus.profile.marker + 1
            || bus.idle_remaining != 4
            || !matches!(
                cpu.physical_entry_state(),
                PhysicalEntryState::FaultUnqualified { .. }
            )
            || cpu.physical_inspection() != PhysicalInspection::Unavailable
            || bus.memory != memory_map(&case.initial.ram)?
            || bus.error.is_some()
        {
            return Err("unknown promoted class did not preserve the consumed capture gap".into());
        }
        let snapshot = cpu.clone();
        let device = (
            bus.elapsed,
            bus.cursor,
            bus.idle_remaining,
            bus.memory.clone(),
            bus.error.clone(),
        );
        if cpu.step_physical_instruction(&mut bus).is_ok()
            || cpu.enter_imported_group0(&mut bus).is_ok()
            || cpu != snapshot
            || device
                != (
                    bus.elapsed,
                    bus.cursor,
                    bus.idle_remaining,
                    bus.memory.clone(),
                    bus.error.clone(),
                )
        {
            return Err("capture gap was retried".into());
        }
        return Ok(WitnessResult::CaptureGap);
    }
    let step = result.map_err(|error| format!("physical step fault: {error:?}"))?;
    if let Some(error) = bus.error {
        return Err(error);
    }
    let clocks = match step {
        PhysicalInstructionStep::Group0 {
            opcode,
            capture,
            clocks,
        } if opcode == case.initial.prefetch[0] && capture.vector == 3 => clocks,
        _ => return Err("physical step did not enter address group0".into()),
    };
    if cpu.registers != case.expected.registers()?
        || cpu.queued_words() != Some(case.expected.prefetch)
    {
        return Err("final normalized registers or raw prefetch differ".into());
    }
    if bus.cursor != case.transactions.len() || bus.idle_remaining != 0 {
        return Err("full reference entry suffix remains unconsumed".into());
    }
    let total = u64::from(case.reference_total_cycles);
    if clocks != total || cpu.clock().0 != total || bus.elapsed != total {
        return Err("full address-fault clock total differs".into());
    }
    if bus.memory != expected || cpu.trace_pending() {
        return Err("final witnessed RAM or pending trace differs".into());
    }
    Ok(WitnessResult::Frame)
}
