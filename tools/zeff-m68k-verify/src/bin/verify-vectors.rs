use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io::Read;

use serde::Deserialize;
use zeff_m68k::bus::{BusAccess, BusResponse, ByteLanes, M68kBus};
use zeff_m68k::{Cpu, Registers, StepOutcome};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Corpus {
    schema: u32,
    source_commit: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    cohort: String,
    initial: State,
    #[serde(rename = "final")]
    expected: State,
    memory: Vec<(u32, u8)>,
    expected_memory: Vec<(u32, u8)>,
    #[serde(default)]
    expected_nominal_cycles: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    d: [u32; 8],
    a: [u32; 7],
    usp: u32,
    ssp: u32,
    sr: u16,
    pc: u32,
}

impl State {
    fn registers(&self) -> Registers {
        let mut registers = Registers::new();
        registers.d = self.d;
        registers.a = self.a;
        registers.usp = self.usp;
        registers.ssp = self.ssp;
        registers.pc = self.pc;
        registers.set_status_register(self.sr);
        registers
    }
}

struct WitnessBus {
    memory: BTreeMap<u32, u8>,
    written: BTreeSet<u32>,
    reset_pulses: Vec<u32>,
}

impl WitnessBus {
    fn read_byte(&self, address: u32) -> Result<u8, String> {
        self.memory
            .get(&address)
            .copied()
            .ok_or_else(|| format!("unobserved read at {address:06x}"))
    }

    fn write_byte(&mut self, address: u32, value: u8) {
        self.memory.insert(address, value);
        self.written.insert(address);
    }
}

impl M68kBus for WitnessBus {
    type Error = String;

    fn read(&mut self, access: BusAccess) -> Result<u16, Self::Error> {
        let address = access.bus_address;
        let high = if access.lanes != ByteLanes::Lower {
            u16::from(self.read_byte(address)?) << 8
        } else {
            0
        };
        let low = if access.lanes != ByteLanes::Upper {
            u16::from(self.read_byte(address + 1)?)
        } else {
            0
        };
        Ok(high | low)
    }

    fn write(&mut self, access: BusAccess, value: u16) -> Result<(), Self::Error> {
        let address = access.bus_address;
        if access.lanes != ByteLanes::Lower {
            self.write_byte(address, (value >> 8) as u8);
        }
        if access.lanes != ByteLanes::Upper {
            self.write_byte(address + 1, value as u8);
        }
        Ok(())
    }

    fn reset_output(&mut self, asserted_clocks: u32) -> Result<BusResponse<()>, Self::Error> {
        self.reset_pulses.push(asserted_clocks);
        Ok(BusResponse::Complete(()))
    }
}

fn run(case: &Case) -> Result<(), String> {
    let initial_memory = memory_map(&case.memory)?;
    let expected_memory = memory_map(&case.expected_memory)?;
    if initial_memory
        .keys()
        .any(|address| !expected_memory.contains_key(address))
    {
        return Err("expected memory omits initial bytes".into());
    }
    let mut cpu = Cpu::new();
    cpu.registers = case.initial.registers();
    if cpu.registers.status_register() != case.initial.sr {
        return Err("initial SR contains unsupported status bits".into());
    }
    let mut bus = WitnessBus {
        memory: initial_memory,
        written: BTreeSet::new(),
        reset_pulses: Vec::new(),
    };
    let (opcode, cycles) = match cpu.step(&mut bus) {
        StepOutcome::Fault(fault) => return Err(format!("step fault: {fault:?}")),
        StepOutcome::Stopped => return Err("unexpected stopped CPU".into()),
        StepOutcome::Executed {
            opcode,
            nominal_cycles,
        }
        | StepOutcome::Exception {
            opcode,
            nominal_cycles,
            ..
        } => (opcode, nominal_cycles),
    };
    let expected_pulses: &[u32] =
        if opcode == 0x4e70 && case.initial.sr & zeff_m68k::SUPERVISOR != 0 {
            &[124]
        } else {
            &[]
        };
    if bus.reset_pulses != expected_pulses {
        return Err("unexpected external reset pulse".into());
    }
    if let Some(expected) = case.expected_nominal_cycles
        && cycles != expected
    {
        return Err(format!(
            "nominal cycles: actual {cycles}; expected {expected}"
        ));
    }
    let expected = case.expected.registers();
    if expected.status_register() != case.expected.sr {
        return Err("expected SR contains unsupported status bits".into());
    }
    if cpu.registers != expected {
        return Err(format!(
            "registers: actual {:?}; expected {:?}",
            cpu.registers, expected
        ));
    }
    for address in &bus.written {
        if !expected_memory.contains_key(address) {
            return Err(format!("unobserved write at {address:06x}"));
        }
    }
    for (address, expected_byte) in expected_memory {
        let actual = bus.memory.get(&address).copied();
        if actual != Some(expected_byte) {
            return Err(format!(
                "memory {address:06x}: actual {actual:?}; expected {expected_byte:02x}"
            ));
        }
    }
    Ok(())
}

fn memory_map(bytes: &[(u32, u8)]) -> Result<BTreeMap<u32, u8>, String> {
    let mut memory = BTreeMap::new();
    for &(address, byte) in bytes {
        if address > 0x00ff_ffff || memory.insert(address, byte).is_some() {
            return Err(format!(
                "duplicate or out-of-range memory address {address:08x}"
            ));
        }
    }
    Ok(memory)
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: verify-vectors <normalized-json-path>")?;
    let limit = 128 * 1024 * 1024;
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err("oversized witness corpus".into());
    }
    let corpus: Corpus = serde_json::from_slice(&bytes)?;
    if corpus.schema != 1 || corpus.source_commit != "64b253116a3de04aaac4346c43680960dc9b67e5" {
        return Err("unsupported normalized corpus schema or source commit".into());
    }
    if corpus.cases.is_empty() {
        return Err("empty witness corpus".into());
    }
    let mut ledger: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut failures = 0;
    for case in &corpus.cases {
        let entry = ledger.entry(&case.cohort).or_default();
        entry.0 += 1;
        match run(case) {
            Ok(()) => entry.1 += 1,
            Err(reason) => {
                failures += 1;
                if failures <= 20 {
                    eprintln!("{} / {}: {reason}", case.cohort, case.name);
                }
            }
        }
    }
    for (cohort, (total, passed)) in ledger {
        println!("{cohort}: {passed}/{total} architectural witnesses passed");
    }
    println!("total: {} cases; {failures} failures", corpus.cases.len());
    if failures != 0 {
        return Err(format!("{failures} architectural witness failures").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nop() -> Case {
        let state = |pc| State {
            d: [0; 8],
            a: [0; 7],
            usp: 0,
            ssp: 0,
            sr: 0x2700,
            pc,
        };
        Case {
            name: "NOP".into(),
            cohort: "NOP".into(),
            initial: state(0x1000),
            expected: state(0x1002),
            memory: vec![(0x1000, 0x4e), (0x1001, 0x71)],
            expected_memory: vec![(0x1000, 0x4e), (0x1001, 0x71)],
            expected_nominal_cycles: None,
        }
    }

    #[test]
    fn valid_witness_passes_and_missing_memory_or_register_evidence_fails() {
        assert!(run(&nop()).is_ok());
        let mut case = nop();
        case.expected_memory.clear();
        assert!(run(&case).unwrap_err().contains("omits initial"));
        let mut case = nop();
        case.memory.pop();
        assert!(run(&case).unwrap_err().contains("unobserved read"));
        let mut case = nop();
        case.expected.d[0] = 1;
        assert!(run(&case).unwrap_err().contains("registers"));
        let mut case = nop();
        case.expected_memory[0].1 = 0;
        assert!(run(&case).unwrap_err().contains("memory"));
    }

    #[test]
    fn duplicate_out_of_range_and_reserved_status_evidence_is_rejected() {
        for expected in [false, true] {
            for byte in [(0x1000, 0x4e), (0x0100_0000, 0)] {
                let mut case = nop();
                if expected {
                    case.expected_memory.push(byte);
                } else {
                    case.memory.push(byte);
                }
                assert!(run(&case).unwrap_err().contains("memory address"));
            }
        }
        let mut case = nop();
        case.initial.sr |= 0x0040;
        assert!(run(&case).unwrap_err().contains("initial SR"));
        let mut case = nop();
        case.expected.sr |= 0x0040;
        assert!(run(&case).unwrap_err().contains("expected SR"));
    }

    #[test]
    fn optional_nominal_cycle_evidence_is_checked() {
        let mut case = nop();
        case.expected_nominal_cycles = Some(4);
        assert!(run(&case).is_ok());
        case.expected_nominal_cycles = Some(6);
        assert!(run(&case).unwrap_err().contains("actual 4; expected 6"));
    }

    #[test]
    fn reset_reference_checks_the_external_pulse_and_architectural_state() {
        let mut case = nop();
        case.cohort = "RESET".into();
        case.memory[1].1 = 0x70;
        case.expected_memory = case.memory.clone();
        case.expected_nominal_cycles = Some(132);
        assert!(run(&case).is_ok());
    }

    #[test]
    fn writes_without_final_evidence_are_rejected() {
        let mut case = nop();
        case.memory[0].1 = 0x20;
        case.memory[1].1 = 0x80;
        case.expected_memory = case.memory.clone();
        case.initial.a[0] = 0x2000;
        case.expected.a[0] = 0x2000;
        case.expected.sr |= 4;
        assert!(run(&case).unwrap_err().contains("unobserved write"));
    }
}
