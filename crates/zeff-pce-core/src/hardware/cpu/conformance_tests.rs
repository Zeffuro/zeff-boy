use super::{Cpu, CpuBus, CpuStep, Registers, StatusFlags};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const PHYSICAL_MEMORY_LEN: usize = 1 << 21;
const EXPECTED_CASES_PER_FILE: usize = 2_500;
const MAX_RECORDED_CYCLES: usize = 500;
const MIN_STATE_RECORD_LEN: usize = 4 + 5 + 2 + 8 + 4;
const MIN_CASE_RECORD_LEN: usize = 4 + 50 + 4 + 2 * MIN_STATE_RECORD_LEN + 4 + 4;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProcessorState {
    registers: Registers,
    mpr: [u8; 8],
    ram: Vec<(u32, u8)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RecordedCycle {
    Read { addr: u32, value: u8, dummy: bool },
    Write { addr: u32, value: u8, dummy: bool },
    Idle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProcessorCase {
    name: String,
    opcode: u8,
    initial: ProcessorState,
    final_state: ProcessorState,
    num_cycles: u32,
    cycles: Vec<RecordedCycle>,
}

struct Decoder<'a> {
    source: &'a [u8],
    cursor: usize,
}

impl<'a> Decoder<'a> {
    fn new(source: &'a [u8]) -> Self {
        Self { source, cursor: 0 }
    }

    fn position(&self) -> usize {
        self.cursor
    }

    fn remaining(&self) -> usize {
        self.source.len() - self.cursor
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let end = self
            .cursor
            .checked_add(N)
            .ok_or_else(|| "binary vector offset overflow".to_owned())?;
        let bytes = self.source.get(self.cursor..end).ok_or_else(|| {
            format!(
                "truncated binary vector at offset {:#X}: need {N} bytes",
                self.cursor
            )
        })?;
        self.cursor = end;
        Ok(bytes.try_into().expect("fixed-size vector field"))
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        Ok(self.read_array::<1>()?[0])
    }

    fn read_u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.read_array()?))
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.read_array()?))
    }

    fn read_len(&mut self, what: &str) -> Result<usize, String> {
        let value = i32::from_le_bytes(self.read_array()?);
        usize::try_from(value).map_err(|_| format!("negative {what} length {value}"))
    }
}

fn decode_file(source: &[u8]) -> Result<Vec<ProcessorCase>, String> {
    let mut decoder = Decoder::new(source);
    let case_count = usize::try_from(decoder.read_u32()?)
        .map_err(|_| "case count does not fit usize".to_owned())?;
    let maximum_cases = decoder.remaining() / MIN_CASE_RECORD_LEN;
    if case_count > maximum_cases {
        return Err(format!(
            "case count {case_count} exceeds the file-size limit of {maximum_cases}"
        ));
    }
    let mut cases = Vec::with_capacity(case_count);
    for index in 0..case_count {
        cases.push(decode_case(&mut decoder).map_err(|error| format!("case {index}: {error}"))?);
    }
    if decoder.position() != source.len() {
        return Err(format!(
            "binary vector has {} trailing bytes",
            source.len() - decoder.position()
        ));
    }
    Ok(cases)
}

fn decode_case(decoder: &mut Decoder<'_>) -> Result<ProcessorCase, String> {
    let start = decoder.position();
    let record_len = decoder.read_len("case record")?;
    if record_len < MIN_CASE_RECORD_LEN {
        return Err(format!(
            "case record is {record_len} bytes, minimum is {MIN_CASE_RECORD_LEN}"
        ));
    }
    let end = start
        .checked_add(record_len)
        .ok_or_else(|| "case record length overflow".to_owned())?;
    if end > decoder.source.len() {
        return Err("case record extends beyond file".to_owned());
    }

    let body_start = decoder.position();
    let mut record = Decoder::new(&decoder.source[body_start..end]);
    let raw_name = record.read_array::<50>()?;
    let name = String::from_utf8(raw_name.to_vec())
        .map_err(|error| format!("case name is not UTF-8: {error}"))?
        .trim_end_matches(['\0', ' '])
        .to_owned();
    let opcode = u8::try_from(record.read_u32()?)
        .map_err(|_| format!("{name:?}: opcode does not fit in one byte"))?;
    let initial = decode_state(&mut record, &name, "initial")?;
    let final_state = decode_state(&mut record, &name, "final")?;
    let num_cycles = record.read_u32()?;
    let recorded_cycles = usize::try_from(record.read_u32()?)
        .map_err(|_| format!("{name:?}: recorded cycle count does not fit usize"))?;
    if recorded_cycles > MAX_RECORDED_CYCLES {
        return Err(format!(
            "{name:?}: records {recorded_cycles} cycles, maximum is {MAX_RECORDED_CYCLES}"
        ));
    }
    if u32::try_from(recorded_cycles).expect("maximum recorded cycles") > num_cycles {
        return Err(format!(
            "{name:?}: records {recorded_cycles} cycles but declares only {num_cycles}"
        ));
    }

    let mut cycles = Vec::with_capacity(recorded_cycles);
    for cycle in 0..recorded_cycles {
        cycles.push(
            decode_cycle(&mut record)
                .map_err(|error| format!("{name:?}: cycle {cycle}: {error}"))?,
        );
    }
    if record.remaining() != 0 {
        return Err(format!(
            "{name:?}: case record has {} trailing bytes",
            record.remaining()
        ));
    }
    decoder.cursor = end;

    Ok(ProcessorCase {
        name,
        opcode,
        initial,
        final_state,
        num_cycles,
        cycles,
    })
}

fn decode_state(
    decoder: &mut Decoder<'_>,
    case_name: &str,
    phase: &str,
) -> Result<ProcessorState, String> {
    let start = decoder.position();
    let state_len = decoder.read_len("processor state")?;
    if state_len < MIN_STATE_RECORD_LEN {
        return Err(format!(
            "{case_name:?}: {phase} state is {state_len} bytes, minimum is {MIN_STATE_RECORD_LEN}"
        ));
    }
    let end = start
        .checked_add(state_len)
        .ok_or_else(|| "processor state length overflow".to_owned())?;
    if end > decoder.source.len() {
        return Err(format!(
            "{case_name:?}: {phase} state extends beyond its case record"
        ));
    }
    let body_start = decoder.position();
    let mut state = Decoder::new(&decoder.source[body_start..end]);
    let registers = Registers {
        a: state.read_u8()?,
        x: state.read_u8()?,
        y: state.read_u8()?,
        sp: state.read_u8()?,
        status: StatusFlags::from_bits_retain(state.read_u8()?),
        pc: state.read_u16()?,
    };
    let mut mpr = [0; 8];
    for value in &mut mpr {
        *value = state.read_u8()?;
    }
    let ram_count = state.read_len("RAM pair list")?;
    let maximum_ram_pairs = state.remaining() / 5;
    if ram_count > maximum_ram_pairs {
        return Err(format!(
            "{case_name:?}: {phase} RAM count {ram_count} exceeds the state-size limit of {maximum_ram_pairs}"
        ));
    }
    let mut ram = Vec::with_capacity(ram_count);
    let mut addresses = HashSet::with_capacity(ram_count);
    for _ in 0..ram_count {
        let addr = state.read_u32()?;
        if addr >= PHYSICAL_MEMORY_LEN as u32 {
            return Err(format!(
                "{case_name:?}: {phase} RAM address {addr:#08X} exceeds 21 bits"
            ));
        }
        if !addresses.insert(addr) {
            return Err(format!(
                "{case_name:?}: {phase} RAM repeats address {addr:#08X}"
            ));
        }
        ram.push((addr, state.read_u8()?));
    }
    if state.remaining() != 0 {
        return Err(format!(
            "{case_name:?}: {phase} state has {} trailing bytes",
            state.remaining()
        ));
    }
    decoder.cursor = end;
    Ok(ProcessorState {
        registers,
        mpr,
        ram,
    })
}

fn decode_cycle(decoder: &mut Decoder<'_>) -> Result<RecordedCycle, String> {
    let packed = decoder.read_u32()?;
    let read = packed & 1 != 0;
    let write = packed & 2 != 0;
    let dummy = packed & 4 != 0;
    if read && write {
        return Err("read and write pins are both active".to_owned());
    }
    if !read && !write {
        if packed != 0 {
            return Err(format!("idle cycle has unexpected pin data {packed:#010X}"));
        }
        return Ok(RecordedCycle::Idle);
    }
    let addr = (packed >> 3) & 0x001F_FFFF;
    let value = (packed >> 24) as u8;
    if read {
        Ok(RecordedCycle::Read { addr, value, dummy })
    } else {
        Ok(RecordedCycle::Write { addr, value, dummy })
    }
}

struct FlatRam {
    memory: Box<[u8]>,
    dirty: Vec<u32>,
    dirty_marked: Box<[bool]>,
    cycle_count: u32,
    trace_limit: usize,
    cycles: Vec<RecordedCycle>,
}

impl FlatRam {
    fn new() -> Self {
        Self {
            memory: vec![0xFF; PHYSICAL_MEMORY_LEN].into_boxed_slice(),
            dirty: Vec::new(),
            dirty_marked: vec![false; PHYSICAL_MEMORY_LEN].into_boxed_slice(),
            cycle_count: 0,
            trace_limit: 0,
            cycles: Vec::new(),
        }
    }

    fn reset(&mut self, initial_ram: &[(u32, u8)], trace_limit: usize) {
        for addr in self.dirty.drain(..) {
            let index = addr as usize;
            self.memory[index] = 0xFF;
            self.dirty_marked[index] = false;
        }
        self.cycle_count = 0;
        self.trace_limit = trace_limit;
        self.cycles.clear();
        for &(addr, value) in initial_ram {
            self.write_untraced(addr, value);
        }
    }

    fn write_untraced(&mut self, addr: u32, value: u8) {
        let index = addr as usize;
        if !self.dirty_marked[index] {
            self.dirty_marked[index] = true;
            self.dirty.push(addr);
        }
        self.memory[index] = value;
    }

    fn record(&mut self, cycle: RecordedCycle) {
        if self.cycles.len() < self.trace_limit {
            self.cycles.push(cycle);
        }
        self.cycle_count = self.cycle_count.wrapping_add(1);
    }

    fn peek(&self, addr: u32) -> u8 {
        self.memory[addr as usize]
    }
}

impl CpuBus for FlatRam {
    fn read(&mut self, physical_addr: u32) -> u8 {
        let value = self.peek(physical_addr);
        self.record(RecordedCycle::Read {
            addr: physical_addr,
            value,
            dummy: false,
        });
        value
    }

    fn write(&mut self, physical_addr: u32, value: u8) {
        self.write_untraced(physical_addr, value);
        self.record(RecordedCycle::Write {
            addr: physical_addr,
            value,
            dummy: false,
        });
    }

    fn dummy_read(&mut self, physical_addr: u32) -> u8 {
        let value = self.peek(physical_addr);
        self.record(RecordedCycle::Read {
            addr: physical_addr,
            value,
            dummy: true,
        });
        value
    }

    fn dummy_write(&mut self, physical_addr: u32, value: u8) {
        self.write_untraced(physical_addr, value);
        self.record(RecordedCycle::Write {
            addr: physical_addr,
            value,
            dummy: true,
        });
    }

    fn idle(&mut self) {
        self.record(RecordedCycle::Idle);
    }
}

struct CorpusRunner {
    bus: FlatRam,
}

impl CorpusRunner {
    fn new() -> Self {
        Self {
            bus: FlatRam::new(),
        }
    }

    fn run_case(
        &mut self,
        expected_opcode: u8,
        case: &ProcessorCase,
        has_complete_trace: bool,
    ) -> Result<(), String> {
        if case.opcode != expected_opcode {
            return Err(format!(
                "case opcode {:02X} does not match file opcode {expected_opcode:02X}",
                case.opcode
            ));
        }
        self.bus.reset(&case.initial.ram, case.cycles.len());
        let mut cpu = Cpu::new();
        *cpu.registers_mut() = case.initial.registers;
        for (index, value) in case.initial.mpr.into_iter().enumerate() {
            cpu.set_mapping_register(index, value);
        }
        let mapped_pc = cpu.logical_to_physical(case.initial.registers.pc);
        if self.bus.peek(mapped_pc) != expected_opcode {
            return Err(format!(
                "mapped PC {mapped_pc:#08X} contains {:02X}, expected {expected_opcode:02X}",
                self.bus.peek(mapped_pc)
            ));
        }

        let step = cpu
            .step(&mut self.bus)
            .map_err(|trap| format!("{trap:?}"))?;
        self.compare_step(step, expected_opcode, case)?;
        let actual_state = ProcessorState {
            registers: cpu.registers(),
            mpr: cpu.mapping_registers(),
            ram: Vec::new(),
        };
        let registers_match = if has_complete_trace {
            actual_state.registers == case.final_state.registers
        } else {
            actual_state.registers.sp == case.final_state.registers.sp
                && actual_state.registers.pc == case.final_state.registers.pc
                && actual_state.registers.status == case.final_state.registers.status
        };
        if !registers_match {
            return Err(format!(
                "register mismatch: actual {:?}, expected {:?}",
                actual_state.registers, case.final_state.registers
            ));
        }
        if actual_state.mpr != case.final_state.mpr {
            return Err(format!(
                "MPR mismatch: actual {:02X?}, expected {:02X?}",
                actual_state.mpr, case.final_state.mpr
            ));
        }
        if has_complete_trace {
            for &(addr, expected) in &case.final_state.ram {
                let actual = self.bus.peek(addr);
                if actual != expected {
                    return Err(format!(
                        "RAM {addr:#08X}: actual {actual:#04X}, expected {expected:#04X}"
                    ));
                }
            }
        }
        Ok(())
    }

    fn compare_step(&self, step: CpuStep, opcode: u8, case: &ProcessorCase) -> Result<(), String> {
        if step.opcode != opcode || step.pc != case.initial.registers.pc {
            return Err(format!("step identity mismatch: {step:?}"));
        }
        if step.cycles != case.num_cycles {
            return Err(format!(
                "cycle count: actual {}, expected {}",
                step.cycles, case.num_cycles
            ));
        }
        if self.bus.cycle_count != case.num_cycles {
            return Err(format!(
                "observed bus cycles: actual {}, expected {}",
                self.bus.cycle_count, case.num_cycles
            ));
        }
        if self.bus.cycles != case.cycles {
            let mismatch = self
                .bus
                .cycles
                .iter()
                .zip(&case.cycles)
                .position(|(actual, expected)| actual != expected)
                .unwrap_or(self.bus.cycles.len().min(case.cycles.len()));
            return Err(format!(
                "cycle {mismatch}: actual {:?}, expected {:?}",
                self.bus.cycles.get(mismatch),
                case.cycles.get(mismatch)
            ));
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
struct CorpusOptions {
    directory: PathBuf,
    opcode: Option<u8>,
    case_limit: Option<usize>,
    runs: usize,
    allow_long_cases: bool,
}

fn corpus_options_from<F>(get: F) -> Result<CorpusOptions, String>
where
    F: Fn(&str) -> Option<String>,
{
    let directory = get("ZEFF_HUC6280_V1_DIR")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "ZEFF_HUC6280_V1_DIR must name a local SingleStepTests huc6280/v1 directory".to_owned()
        })?;
    let opcode = get("ZEFF_HUC6280_OPCODE")
        .map(|value| parse_opcode(&value))
        .transpose()?;
    let case_limit = get("ZEFF_HUC6280_CASE_LIMIT")
        .map(|value| parse_case_limit(&value))
        .transpose()?;
    let runs = get("ZEFF_HUC6280_RUNS")
        .map(|value| parse_runs(&value))
        .transpose()?
        .unwrap_or(1);
    let allow_long_cases = match get("ZEFF_HUC6280_ALLOW_LONG_CASES").as_deref() {
        None | Some("0") => false,
        Some("1") => true,
        Some(_) => return Err("ZEFF_HUC6280_ALLOW_LONG_CASES must be 0 or 1".to_owned()),
    };
    Ok(CorpusOptions {
        directory: PathBuf::from(directory),
        opcode,
        case_limit,
        runs,
        allow_long_cases,
    })
}

fn parse_opcode(value: &str) -> Result<u8, String> {
    if value.len() != 2 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("ZEFF_HUC6280_OPCODE must be exactly two hexadecimal digits".to_owned());
    }
    u8::from_str_radix(value, 16)
        .map_err(|_| "ZEFF_HUC6280_OPCODE must be exactly two hexadecimal digits".to_owned())
}

fn parse_case_limit(value: &str) -> Result<usize, String> {
    match value.parse() {
        Ok(value @ 1..=EXPECTED_CASES_PER_FILE) => Ok(value),
        _ => Err(format!(
            "ZEFF_HUC6280_CASE_LIMIT must be between 1 and {EXPECTED_CASES_PER_FILE}"
        )),
    }
}

fn parse_runs(value: &str) -> Result<usize, String> {
    match value {
        "1" => Ok(1),
        "2" => Ok(2),
        _ => Err("ZEFF_HUC6280_RUNS must be 1 or 2".to_owned()),
    }
}

fn requested_opcodes(opcode: Option<u8>) -> Vec<u8> {
    opcode.map_or_else(|| (0..=u8::MAX).collect(), |opcode| vec![opcode])
}

fn opcode_path(directory: &Path, opcode: u8) -> PathBuf {
    directory.join(format!("{opcode:02x}.json.bin"))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RunStats {
    executed: usize,
    skipped_undefined: usize,
    truncated_cases: usize,
}

fn run_opcode_file(
    runner: &mut CorpusRunner,
    opcode: u8,
    path: &Path,
    options: &CorpusOptions,
) -> Result<RunStats, String> {
    let source = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let cases = decode_file(&source).map_err(|error| format!("{}: {error}", path.display()))?;
    if cases.len() != EXPECTED_CASES_PER_FILE {
        return Err(format!(
            "{}: expected {EXPECTED_CASES_PER_FILE} cases, found {}",
            path.display(),
            cases.len()
        ));
    }
    let mut stats = RunStats::default();
    for (index, case) in cases
        .iter()
        .take(options.case_limit.unwrap_or(usize::MAX))
        .enumerate()
    {
        // The manual requires a one-hot TMA selector. Multiple driven MPRs have
        // stable OR behavior in the corpus, but selector zero leaves the bus
        // undriven and produces non-architectural values in the oracle.
        if is_undefined_tma_zero(case) {
            stats.skipped_undefined += 1;
            continue;
        }
        let has_complete_trace = case.cycles.len() == case.num_cycles as usize;
        if !options.allow_long_cases && !has_complete_trace {
            return Err(format!(
                "{}: case {index} {:?} has a truncated long trace; set ZEFF_HUC6280_ALLOW_LONG_CASES=1 to run it",
                path.display(),
                case.name
            ));
        }
        if !has_complete_trace {
            stats.truncated_cases += 1;
        }
        for run in 0..options.runs {
            runner
                .run_case(opcode, case, has_complete_trace)
                .map_err(|error| {
                    format!(
                        "{}: case {index} {:?}, run {}: {error}",
                        path.display(),
                        case.name,
                        run + 1
                    )
                })?;
            stats.executed += 1;
        }
    }
    Ok(stats)
}

fn is_undefined_tma_zero(case: &ProcessorCase) -> bool {
    case.opcode == 0x43
        && matches!(
            case.cycles.get(1),
            Some(RecordedCycle::Read {
                value: 0,
                dummy: false,
                ..
            })
        )
}

#[test]
#[ignore = "requires a local SingleStepTests huc6280/v1 binary corpus"]
fn huc6280_v1_diagnostic_corpus() {
    let options =
        corpus_options_from(|name| std::env::var(name).ok()).expect("valid corpus options");
    let mut runner = CorpusRunner::new();
    let mut totals = RunStats::default();
    for opcode in requested_opcodes(options.opcode) {
        let path = opcode_path(&options.directory, opcode);
        assert!(path.is_file(), "missing corpus file {}", path.display());
        let stats = run_opcode_file(&mut runner, opcode, &path, &options)
            .unwrap_or_else(|error| panic!("{error}"));
        totals.executed += stats.executed;
        totals.skipped_undefined += stats.skipped_undefined;
        totals.truncated_cases += stats.truncated_cases;
    }
    eprintln!(
        "HuC6280 corpus: {} executions, {} truncated cases, {} undefined cases skipped",
        totals.executed, totals.truncated_cases, totals.skipped_undefined
    );
}

#[test]
fn binary_decoder_and_options_are_strict() {
    let case = synthetic_case();
    let decoded = decode_file(&case).expect("synthetic binary vector");
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].name, "ea #synthetic");
    assert_eq!(decoded[0].opcode, 0xEA);
    assert_eq!(decoded[0].num_cycles, 2);
    assert_eq!(decoded[0].cycles.len(), 2);

    assert!(decode_file(&case[..case.len() - 1]).is_err());
    let mut trailing = case.clone();
    trailing.push(0);
    assert!(decode_file(&trailing).is_err());
    assert!(decode_file(&u32::MAX.to_le_bytes()).is_err());

    let mut short_record = case.clone();
    short_record[4..8].copy_from_slice(&4_u32.to_le_bytes());
    assert!(decode_file(&short_record).is_err());

    let mut crossing_state = case.clone();
    crossing_state[62..66].copy_from_slice(&1_000_u32.to_le_bytes());
    assert!(decode_file(&crossing_state).is_err());

    let mut oversized_ram_count = case.clone();
    oversized_ram_count[81..85].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode_file(&oversized_ram_count).is_err());

    assert!(parse_opcode("e").is_err());
    assert!(parse_opcode("gg").is_err());
    assert!(parse_case_limit("0").is_err());
    assert!(parse_case_limit("2501").is_err());
    assert!(parse_runs("3").is_err());

    let options = corpus_options_from(|name| match name {
        "ZEFF_HUC6280_V1_DIR" => Some("C:/vectors/huc6280/v1".to_owned()),
        "ZEFF_HUC6280_OPCODE" => Some("EA".to_owned()),
        "ZEFF_HUC6280_CASE_LIMIT" => Some("17".to_owned()),
        "ZEFF_HUC6280_RUNS" => Some("2".to_owned()),
        "ZEFF_HUC6280_ALLOW_LONG_CASES" => Some("1".to_owned()),
        _ => None,
    })
    .expect("valid options");
    assert_eq!(options.opcode, Some(0xEA));
    assert_eq!(options.case_limit, Some(17));
    assert_eq!(options.runs, 2);
    assert!(options.allow_long_cases);
}

#[test]
fn flat_ram_reset_clears_loaded_and_written_state() {
    let mut ram = FlatRam::new();
    ram.reset(&[(1, 0x12)], 2);
    ram.write(2, 0x34);
    ram.idle();
    assert_eq!(ram.cycle_count, 2);
    assert_eq!(ram.cycles.len(), 2);

    ram.reset(&[], 0);

    assert_eq!(ram.peek(1), 0xFF);
    assert_eq!(ram.peek(2), 0xFF);
    assert_eq!(ram.cycle_count, 0);
    assert!(ram.cycles.is_empty());
}

fn synthetic_case() -> Vec<u8> {
    let state = |pc: u16| {
        let mut body = vec![1, 2, 3, 0xFD, 0x24];
        body.extend_from_slice(&pc.to_le_bytes());
        body.extend_from_slice(&[0; 8]);
        body.extend_from_slice(&2_i32.to_le_bytes());
        body.extend_from_slice(&0_u32.to_le_bytes());
        body.push(0xEA);
        body.extend_from_slice(&1_u32.to_le_bytes());
        body.push(0xA5);
        with_len(body)
    };

    let mut record = [b' '; 50].to_vec();
    record[..13].copy_from_slice(b"ea #synthetic");
    record.extend_from_slice(&0xEA_u32.to_le_bytes());
    record.extend_from_slice(&state(0));
    record.extend_from_slice(&state(1));
    record.extend_from_slice(&2_u32.to_le_bytes());
    record.extend_from_slice(&2_u32.to_le_bytes());
    record.extend_from_slice(&packed_cycle(0, 0xEA, true, false, false).to_le_bytes());
    record.extend_from_slice(&packed_cycle(1, 0xA5, true, false, true).to_le_bytes());

    let mut file = 1_u32.to_le_bytes().to_vec();
    file.extend_from_slice(&with_len(record));
    file
}

fn with_len(body: Vec<u8>) -> Vec<u8> {
    let len = u32::try_from(body.len() + 4).expect("synthetic record length");
    let mut record = len.to_le_bytes().to_vec();
    record.extend_from_slice(&body);
    record
}

fn packed_cycle(addr: u32, value: u8, read: bool, write: bool, dummy: bool) -> u32 {
    u32::from(read)
        | (u32::from(write) << 1)
        | (u32::from(dummy) << 2)
        | (addr << 3)
        | (u32::from(value) << 24)
}
