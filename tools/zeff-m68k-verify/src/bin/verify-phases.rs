use std::collections::BTreeMap;
use std::error::Error;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu};

#[path = "verify_phases/arithmetic.rs"]
mod arithmetic;
#[path = "verify_phases/branches.rs"]
mod branches;
#[path = "verify_phases/format.rs"]
mod format;
#[path = "verify_phases/held_control.rs"]
mod held_control;
#[path = "verify_phases/ordinary.rs"]
mod ordinary;
#[path = "verify_phases/stack_status.rs"]
mod stack_status;
#[path = "verify_phases/transfers.rs"]
mod transfers;
use format::{Case, Phase, Transfer, verify_reader, verify_receipt};

struct WitnessBus<'a> {
    memory: BTreeMap<u32, u8>,
    phases: &'a [Phase],
    cursor: usize,
    elapsed: u64,
    error: Option<String>,
    trace_stack: Option<[u32; 3]>,
    control_stack: Option<branches::StackContract>,
    status_contract: Option<stack_status::Contract>,
    short_vector: Option<u8>,
    transfer_contract: Option<transfers::Contract>,
    held_contract: Option<held_control::Contract>,
    ordinary_contract: Option<ordinary::Contract>,
    reset_owned: bool,
    reset_pairs: u8,
    idle_remaining: u32,
    operand_data: bool,
    pc_source: bool,
}

impl WitnessBus<'_> {
    fn transfer(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        write: bool,
    ) -> Result<Transfer, String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if self.reset_owned {
            return Err("transfer during RESET output ownership".into());
        }
        if self.idle_remaining != 0 {
            return Err("transfer interrupts reference idle interval".into());
        }
        if start.0 != self.elapsed {
            return Err("transfer start clock differs".into());
        }
        let transfer = match (self.phases.get(self.cursor), write) {
            (Some(Phase::Read(transfer)), false) | (Some(Phase::Write(transfer)), true) => {
                transfer.clone()
            }
            _ => return Err(format!("unexpected transfer at phase {}", self.cursor)),
        };
        let lanes = match access.lanes {
            ByteLanes::Both => (1, 1),
            ByteLanes::Upper => (1, 0),
            ByteLanes::Lower => (0, 1),
        };
        if access.bus_address != transfer.address_bus
            || access.function_code as u32 != transfer.function_code
            || lanes != (transfer.uds, transfer.lds)
            || !self.allowed_access(access, write)
        {
            return Err(format!("bus fields differ at phase {}", self.cursor));
        }
        if transfer.duration_cycles < 4 {
            return Err("reference transfer shorter than four clocks".into());
        }
        self.cursor += 1;
        self.elapsed += u64::from(transfer.duration_cycles);
        Ok(transfer)
    }

    fn allowed_access(&self, access: BusAccess, write: bool) -> bool {
        if let Some(contract) = &self.ordinary_contract {
            return contract.permits(access, write, self.cursor);
        }
        if let Some(contract) = &self.held_contract {
            return contract.permits(access, write, self.cursor);
        }
        if let Some(contract) = &self.transfer_contract {
            return contract.permits(access, write, self.cursor);
        }
        if let Some(contract) = &self.status_contract {
            return contract.permits(access, write);
        }
        match access.kind {
            AccessKind::Prefetch => !write,
            AccessKind::Data => {
                self.operand_data
                    && if self.pc_source && !write {
                        matches!(access.function_code as u32, 2 | 6)
                    } else {
                        matches!(access.function_code as u32, 1 | 5)
                    }
            }
            AccessKind::Vector => {
                !write
                    && self.trace_stack.is_some()
                    && access.function_code as u32 == 5
                    && self.short_vector.is_some_and(|vector| {
                        access.bus_address == u32::from(vector) * 4
                            || access.bus_address == u32::from(vector) * 4 + 2
                    })
            }
            AccessKind::Stack => {
                write
                    && ((access.function_code as u32 == 5
                        && self
                            .trace_stack
                            .is_some_and(|addresses| addresses.contains(&access.bus_address)))
                        || self.control_stack.as_ref().is_some_and(|contract| {
                            access.function_code as u32 == contract.function_code
                                && contract.addresses.contains(&access.bus_address)
                        }))
            }
            _ => false,
        }
    }

    fn write(&mut self, access: BusAccess, value: u16, start: CpuClocks) -> Result<u32, String> {
        let transfer = self.transfer(access, start, true)?;
        if u32::from(value) != transfer.data_bus {
            return Err("write data bus differs".into());
        }
        if access.lanes != ByteLanes::Lower {
            self.memory.insert(access.bus_address, (value >> 8) as u8);
        }
        if access.lanes != ByteLanes::Upper {
            self.memory.insert(access.bus_address + 1, value as u8);
        }
        Ok(transfer.duration_cycles - 4)
    }

    fn byte(&self, address: u32) -> Result<u8, String> {
        self.memory
            .get(&address)
            .copied()
            .ok_or_else(|| format!("unobserved read {address:06x}"))
    }

    fn read(&mut self, access: BusAccess, start: CpuClocks) -> Result<(u16, u32), String> {
        let transfer = self.transfer(access, start, false)?;
        let high = if access.lanes != ByteLanes::Lower {
            u16::from(self.byte(access.bus_address)?) << 8
        } else {
            0
        };
        let low = if access.lanes != ByteLanes::Upper {
            u16::from(self.byte(access.bus_address + 1)?)
        } else {
            0
        };
        let value = high | low;
        if u32::from(value) != transfer.data_bus {
            return Err("read data bus differs".into());
        }
        Ok((value, transfer.duration_cycles - 4))
    }
}

impl ClockedBus for WitnessBus<'_> {
    type Error = String;
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, String> {
        match self.read(access, start) {
            Ok((value, wait_clocks)) => CycleResult {
                result: Ok(value),
                wait_clocks,
            },
            Err(error) => CycleResult {
                result: Err(error),
                wait_clocks: 0,
            },
        }
    }
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), String> {
        match self.write(access, value, start) {
            Ok(wait_clocks) => CycleResult {
                result: Ok(()),
                wait_clocks,
            },
            Err(error) => CycleResult {
                result: Err(error),
                wait_clocks: 0,
            },
        }
    }
    fn begin_reset_output(
        &mut self,
        start: CpuClocks,
    ) -> Result<zeff_m68k::bus::BusResponse<()>, String> {
        if !self
            .held_contract
            .as_ref()
            .is_some_and(|contract| contract.reset_pulse)
            || self.reset_owned
            || self.reset_pairs != 0
            || start.0 != 4
            || self.elapsed != 4
        {
            return Err("unexpected RESET output assertion".into());
        }
        self.reset_owned = true;
        Ok(zeff_m68k::bus::BusResponse::Complete(()))
    }
    fn end_reset_output(&mut self, end: CpuClocks) {
        if !self.reset_owned || end.0 != 128 || self.elapsed != 128 {
            self.error = Some("unexpected RESET output release".into());
        }
        self.reset_owned = false;
        self.reset_pairs += 1;
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        if self.error.is_some() {
            return;
        }
        if start.0 != self.elapsed {
            self.error = Some("idle start clock differs".into());
            return;
        }
        let mut remaining = clocks;
        while remaining != 0 {
            if self.idle_remaining == 0 {
                match self.phases.get(self.cursor) {
                    Some(Phase::Idle { duration_cycles }) if *duration_cycles != 0 => {
                        self.idle_remaining = *duration_cycles;
                    }
                    _ => {
                        self.error =
                            Some(format!("idle crosses transfer at phase {}", self.cursor));
                        return;
                    }
                }
            }
            let used = remaining.min(self.idle_remaining);
            remaining -= used;
            self.idle_remaining -= used;
            self.elapsed += u64::from(used);
            if self.idle_remaining == 0 {
                self.cursor += 1;
            }
        }
    }
}

fn memory_map(bytes: &[(u32, u8)]) -> Result<BTreeMap<u32, u8>, String> {
    let mut result = BTreeMap::new();
    for &(address, byte) in bytes {
        if address > 0x00ff_ffff || result.insert(address, byte).is_some() {
            return Err("duplicate or out-of-range memory witness".into());
        }
    }
    Ok(result)
}

fn expected_memory(case: &Case, initial: &BTreeMap<u32, u8>) -> Result<BTreeMap<u32, u8>, String> {
    let mut expected = memory_map(&case.expected.ram)?;
    if initial
        .keys()
        .any(|address| !expected.contains_key(address))
    {
        return Err("final RAM snapshot omits initial witness".into());
    }
    let mut selected = std::collections::BTreeSet::new();
    let mut written = std::collections::BTreeSet::new();
    for phase in &case.transactions {
        let (transfer, write) = match phase {
            Phase::Read(value) => (value, false),
            Phase::Write(value) => (value, true),
            _ => continue,
        };
        for (offset, lane) in [(0, transfer.uds), (1, transfer.lds)] {
            if lane == 1 {
                selected.insert(transfer.address_bus + offset);
                if write {
                    written.insert(transfer.address_bus + offset);
                }
            }
        }
    }
    let padding: Vec<_> = expected
        .iter()
        .filter(|(address, _)| !initial.contains_key(*address) && !selected.contains(*address))
        .map(|(&address, &byte)| (address, byte))
        .collect();
    for (address, byte) in padding {
        // A sparse final word snapshot pads the unused lane beside a new byte write.
        if byte != 0 || !written.contains(&(address ^ 1)) {
            return Err("unknown final-only memory witness".into());
        }
        expected.remove(&address);
    }
    Ok(expected)
}

fn run(case: &Case) -> Result<(), String> {
    run_boundary(case, false)
}

fn run_boundary(case: &Case, aggregate_trace: bool) -> Result<(), String> {
    let registers = case.initial.registers()?;
    let trace = registers.status_register() & zeff_m68k::TRACE != 0;
    let privilege =
        matches!(case.initial.prefetch[0], 0x007c | 0x027c | 0x0a7c) && !registers.supervisor();
    let arithmetic = arithmetic::exception_contract(case)?;
    let control_stack = branches::contract(case)?;
    let status_contract = stack_status::derive(case)?;
    let transfer_contract = transfers::derive(case)?;
    let held_contract = held_control::derive(case)?;
    let ordinary_contract = ordinary::derive(case)?;
    if aggregate_trace && trace && ordinary_contract.is_some() {
        return Err("ordinary trace requires a separate service boundary fixture".into());
    }
    if aggregate_trace && trace && held_contract.is_some() {
        return Err("held-control trace requires a separate service boundary fixture".into());
    }
    if aggregate_trace && trace && status_contract.is_some() {
        return Err("stack/status trace requires a separate service boundary fixture".into());
    }
    if aggregate_trace && trace && arithmetic.is_some() {
        return Err("arithmetic trace chain requires a separate service boundary fixture".into());
    }
    let expected_exception = if let Some(contract) = &held_contract {
        contract.exception
    } else if let Some(contract) = &status_contract {
        contract.exception
    } else if let Some(contract) = &arithmetic {
        Some(contract.vector)
    } else if privilege {
        Some(8)
    } else {
        (trace && aggregate_trace).then_some(9)
    };
    let trace_stack = arithmetic
        .as_ref()
        .map(|contract| contract.stack)
        .or_else(|| {
            expected_exception
                .map(|_| [2, 6, 4].map(|offset| registers.ssp.wrapping_sub(offset) & 0x00ff_fffe))
        });
    let initial = memory_map(&case.initial.ram)?;
    let expected = expected_memory(case, &initial)?;
    let mut cpu = PrefetchCpu::from_prefetched(registers, case.initial.prefetch, CpuClocks(0))
        .ok_or("odd initial architectural PC")?;
    let mut bus = WitnessBus {
        memory: initial,
        phases: &case.transactions,
        cursor: 0,
        elapsed: 0,
        error: None,
        trace_stack,
        control_stack,
        status_contract,
        short_vector: expected_exception,
        transfer_contract,
        held_contract,
        ordinary_contract,
        reset_owned: false,
        reset_pairs: 0,
        idle_remaining: 0,
        pc_source: matches!(case.initial.prefetch[0] & 0x3f, 0x3a | 0x3b)
            && (matches!(case.initial.prefetch[0] >> 12, 1..=3)
                || matches!(case.initial.prefetch[0] >> 12, 8 | 9 | 11 | 12 | 13)
                    && matches!((case.initial.prefetch[0] >> 6) & 7, 0..=3 | 7)
                || matches!(
                    case.cohort.as_str(),
                    "BTST" | "MULU" | "MULS" | "DIVU" | "DIVS" | "CHK"
                )),
        operand_data: matches!(
            case.cohort.as_str(),
            "MOVE.b"
                | "Scc"
                | "MOVE.w"
                | "MOVE.l"
                | "MOVEA.w"
                | "MOVEA.l"
                | "TST.b"
                | "TST.w"
                | "TST.l"
                | "CLR.b"
                | "CLR.w"
                | "CLR.l"
                | "NEGX.l"
                | "MULU"
                | "MULS"
                | "DIVU"
                | "DIVS"
                | "CHK"
                | "ABCD"
                | "SBCD"
                | "NBCD"
                | "ADDX.b"
                | "ADDX.w"
                | "ADDX.l"
                | "SUBX.b"
                | "SUBX.w"
                | "SUBX.l"
                | "BTST"
                | "BCHG"
                | "BCLR"
                | "BSET"
                | "ASL.w"
                | "ASR.w"
                | "LSL.w"
                | "LSR.w"
                | "ROL.w"
                | "ROR.w"
                | "ROXL.w"
                | "ROXR.w"
                | "ADD.b"
                | "ADD.w"
                | "ADD.l"
                | "SUB.b"
                | "SUB.w"
                | "SUB.l"
                | "AND.b"
                | "AND.w"
                | "AND.l"
                | "OR.b"
                | "OR.w"
                | "OR.l"
                | "EOR.b"
                | "EOR.w"
                | "EOR.l"
                | "CMP.b"
                | "CMP.w"
                | "CMP.l"
                | "ADDA.w"
                | "ADDA.l"
                | "SUBA.w"
                | "SUBA.l"
                | "CMPA.w"
                | "CMPA.l"
        ),
    };
    let step = if aggregate_trace {
        cpu.step(&mut bus)
    } else {
        cpu.step_instruction(&mut bus)
    };
    if let Some(error) = bus.error {
        return Err(error);
    }
    let step = step.map_err(|error| format!("timed step fault: {error:?}"))?;
    if step.opcode != case.initial.prefetch[0] || step.exception != expected_exception {
        return Err("executed opcode differs".into());
    }
    let expected_trace = bus
        .status_contract
        .as_ref()
        .map_or(trace && !aggregate_trace && !privilege, |contract| {
            contract.pending_trace
        });
    let expected_trace = bus
        .held_contract
        .as_ref()
        .map_or(expected_trace, |contract| contract.pending_trace);
    if bus.reset_owned
        || bus.reset_pairs
            != u8::from(
                bus.held_contract
                    .as_ref()
                    .is_some_and(|contract| contract.reset_pulse),
            )
    {
        return Err("RESET output ownership completion differs".into());
    }
    if cpu.trace_pending() != expected_trace {
        return Err("pending trace boundary differs".into());
    }
    if cpu.registers != case.expected.registers()? {
        return Err("final registers differ".into());
    }
    if cpu.queued_words() != Some(case.expected.prefetch) {
        return Err("final prefetch order differs".into());
    }
    if bus.cursor != case.transactions.len() || bus.idle_remaining != 0 {
        return Err("reference phases remain unconsumed".into());
    }
    let total = u64::from(case.reference_total_cycles);
    if step.clocks != total || cpu.clock().0 != total || bus.elapsed != total {
        return Err("instruction clocks differ".into());
    }
    if bus.memory != expected {
        return Err("final witnessed memory differs".into());
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: verify-phases <phase-jsonl> [receipt-json]")?;
    let explicit_receipt = args.next();
    if args.next().is_some() {
        return Err("usage: verify-phases <phase-jsonl> [receipt-json]".into());
    }
    let report = verify_reader(BufReader::new(File::open(&path)?))?;
    let receipt_path = explicit_receipt.as_ref().map_or_else(
        || Path::new(&path).with_extension("receipt.json"),
        |path| Path::new(path).to_path_buf(),
    );
    let mut receipt_checked = false;
    if explicit_receipt.is_some() || receipt_path.exists() {
        let mut bytes = Vec::new();
        File::open(receipt_path)?
            .take(128 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 128 * 1024 {
            return Err("oversized phase receipt".into());
        }
        verify_receipt(serde_json::from_slice(&bytes)?, &report)?;
        receipt_checked = true;
    }
    for (cohort, row) in &report.ledger {
        println!(
            "{cohort}: raw={}, eligible={}, passed={}, unsupported_BSR={}, trace_prefix_pass={}, physical_unsupported={}, unsupported_routes={:?}, excluded={:?}",
            row.total,
            row.eligible,
            row.passed,
            row.unsupported,
            row.trace_prefix_pass,
            row.physical_unsupported,
            row.unsupported_routes,
            row.excluded
        );
    }
    println!(
        "total: {} records, {} failures; receipt_checked={receipt_checked}; sha256={}; raw_transaction_clocks={}",
        report.records, report.failures, report.hash, report.raw_phase_clocks
    );
    println!(
        "Instruction prefixes only: upstream trace-marked records stop before trace entry. RESET edges are directed adapter checks, not raw reference evidence. Full-transfer and idle boundaries only; no reference proof of trace entry, half-cycle pins or physical hardware."
    );
    if report.failures != 0 {
        return Err(format!("{} phase witness failures", report.failures).into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "verify_phases/tests.rs"]
mod tests;
