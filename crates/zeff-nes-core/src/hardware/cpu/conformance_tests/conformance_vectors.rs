use super::*;
use crate::hardware::cpu::CpuBus;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};
use zeff_emu_common::cpu::CpuCore;

#[derive(Deserialize)]
struct ProcessorCase {
    name: String,
    initial: ProcessorState,
    #[serde(rename = "final")]
    final_state: ProcessorState,
    cycles: Vec<(u16, u8, ProcessorAccess)>,
}

#[derive(Deserialize)]
struct ProcessorState {
    pc: u16,
    s: u8,
    a: u8,
    x: u8,
    y: u8,
    p: u8,
    ram: Vec<(u16, u8)>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ProcessorAccess {
    Read,
    Write,
}

struct ParsedCase {
    name: String,
    case: CpuCase<Nes6502State>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Nes6502State {
    pc: u16,
    sp: u8,
    a: u8,
    x: u8,
    y: u8,
    status: u8,
}

fn parse_cases(source: &str) -> Result<Vec<ParsedCase>, String> {
    let cases: Vec<ProcessorCase> =
        serde_json::from_str(source).map_err(|error| error.to_string())?;
    cases.into_iter().map(parse_case).collect()
}

fn parse_case(vector: ProcessorCase) -> Result<ParsedCase, String> {
    let initial_state = state_from(&vector.initial);
    let expected_state = state_from(&vector.final_state);
    let initial_memory = sparse_memory(&vector.name, "initial", &vector.initial.ram)?;
    let expected_memory = sparse_memory(&vector.name, "final", &vector.final_state.ram)?;
    let bus_events = trace_events(&vector.name, &initial_memory, &vector.cycles)?;

    Ok(ParsedCase {
        name: vector.name,
        case: CpuCase {
            initial_state,
            expected_state,
            initial_memory: memory_blocks(initial_memory),
            expected_memory: memory_blocks(expected_memory),
            expected_step: StepObservation {
                kind: StepKind::Instruction,
                cpu_cycles: u64::try_from(bus_events.len()).expect("vector cycle count"),
                master_ticks: MasterTicks::new(
                    u64::try_from(bus_events.len()).expect("vector cycle count"),
                ),
                master_rate: ClockRate::from_ratio(21_477_272, 12),
                bus_events,
            },
        },
    })
}

fn state_from(state: &ProcessorState) -> Nes6502State {
    Nes6502State {
        pc: state.pc,
        sp: state.s,
        a: state.a,
        x: state.x,
        y: state.y,
        status: state.p,
    }
}

fn sparse_memory(
    name: &str,
    phase: &str,
    entries: &[(u16, u8)],
) -> Result<BTreeMap<u16, u8>, String> {
    let mut memory = BTreeMap::new();
    for &(address, value) in entries {
        if memory.insert(address, value).is_some() {
            return Err(format!(
                "{name}: {phase} RAM repeats address {address:#06X}"
            ));
        }
    }
    Ok(memory)
}

fn memory_blocks(memory: BTreeMap<u16, u8>) -> Vec<MemoryBlock> {
    memory
        .into_iter()
        .map(|(address, value)| MemoryBlock::new(u32::from(address), [value]))
        .collect()
}

fn trace_events(
    name: &str,
    initial_memory: &BTreeMap<u16, u8>,
    cycles: &[(u16, u8, ProcessorAccess)],
) -> Result<Vec<BusAccessEvent>, String> {
    let mut memory = BTreeMap::new();
    for (&address, &value) in initial_memory {
        memory.insert(address, value);
    }

    cycles
        .iter()
        .copied()
        .enumerate()
        .map(|(index, (address, value, access))| {
            let at = Some(MasterTicks::new(
                u64::try_from(index).expect("vector cycle index"),
            ));
            match access {
                ProcessorAccess::Read => {
                    let stored_value = memory.get(&address).ok_or_else(|| {
                        format!(
                            "{name}: cycle {index} reads {address:#06X} without initial memory"
                        )
                    })?;
                    if *stored_value != value {
                        return Err(format!(
                            "{name}: cycle {index} reads {value:#04X} from {address:#06X}, expected {stored_value:#04X}"
                        ));
                    }
                    Ok(BusAccessEvent::Read {
                        at,
                        space: TraceWriteKind::Memory,
                        addr: u32::from(address),
                        value: u32::from(value),
                        width: TraceWriteWidth::Byte,
                        mapped_addr: None,
                    })
                }
                ProcessorAccess::Write => {
                    let old_value = memory.insert(address, value).unwrap_or(0xFF);
                    Ok(BusAccessEvent::Write {
                        at,
                        space: TraceWriteKind::Memory,
                        addr: u32::from(address),
                        old_value: u32::from(old_value),
                        written_value: u32::from(value),
                        new_value: u32::from(value),
                        width: TraceWriteWidth::Byte,
                        mapped_addr: None,
                    })
                }
            }
        })
        .collect()
}

struct FlatRam {
    memory: Box<[u8; 0x10000]>,
    dirty_addresses: Vec<u16>,
    dirty_marked: Box<[bool; 0x10000]>,
    access_cursor: u64,
    events: Vec<BusAccessEvent>,
}

impl FlatRam {
    fn new() -> Self {
        Self {
            memory: Box::new([0xFF; 0x10000]),
            dirty_addresses: Vec::new(),
            dirty_marked: Box::new([false; 0x10000]),
            access_cursor: 0,
            events: Vec::new(),
        }
    }

    fn reset_to_default(&mut self) {
        for address in self.dirty_addresses.drain(..) {
            let index = usize::from(address);
            self.memory[index] = 0xFF;
            self.dirty_marked[index] = false;
        }
        self.access_cursor = 0;
        self.events.clear();
    }

    fn write_untraced(&mut self, address: u16, value: u8) -> u8 {
        let index = usize::from(address);
        if !self.dirty_marked[index] {
            self.dirty_marked[index] = true;
            self.dirty_addresses.push(address);
        }
        std::mem::replace(&mut self.memory[index], value)
    }

    fn load(&mut self, blocks: &[MemoryBlock]) {
        for block in blocks {
            for (offset, &value) in block.bytes.iter().enumerate() {
                let address = u16::try_from(
                    block
                        .start
                        .wrapping_add(u32::try_from(offset).expect("test memory block offset")),
                )
                .expect("NES test address must fit in 16 bits");
                self.write_untraced(address, value);
            }
        }
    }

    fn load_sparse(&mut self, memory: &BTreeMap<u16, u8>) {
        for (&address, &value) in memory {
            self.write_untraced(address, value);
        }
    }

    fn peek(&self, address: u32) -> u8 {
        let address = u16::try_from(address).expect("NES test address must fit in 16 bits");
        self.memory[usize::from(address)]
    }

    fn take_events(&mut self) -> Vec<BusAccessEvent> {
        std::mem::take(&mut self.events)
    }
}

impl CpuBus for FlatRam {
    fn cpu_read(&mut self, addr: u16) -> u8 {
        let value = self.memory[usize::from(addr)];
        self.events.push(BusAccessEvent::Read {
            at: Some(MasterTicks::new(self.access_cursor)),
            space: TraceWriteKind::Memory,
            addr: u32::from(addr),
            value: u32::from(value),
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        });
        self.access_cursor = self.access_cursor.wrapping_add(1);
        value
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        let old_value = self.write_untraced(addr, value);
        self.events.push(BusAccessEvent::Write {
            at: Some(MasterTicks::new(self.access_cursor)),
            space: TraceWriteKind::Memory,
            addr: u32::from(addr),
            old_value: u32::from(old_value),
            written_value: u32::from(value),
            new_value: u32::from(value),
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        });
        self.access_cursor = self.access_cursor.wrapping_add(1);
    }

    fn cpu_read_after_elapsed_cycles(&mut self, addr: u16, elapsed_cycles: u64) -> u8 {
        self.access_cursor = self.access_cursor.max(elapsed_cycles);
        self.cpu_read(addr)
    }

    fn cpu_write_after_elapsed_cycles(&mut self, addr: u16, value: u8, elapsed_cycles: u64) {
        self.access_cursor = self.access_cursor.max(elapsed_cycles);
        self.cpu_write(addr, value);
    }

    fn prepare_cpu_instruction_accesses(&mut self) {
        self.access_cursor = 0;
        self.events.clear();
    }

    fn finish_cpu_instruction_accesses(&mut self, total_cycles: u64, pc: u16) {
        while self.access_cursor < total_cycles {
            let _ = self.cpu_read(pc);
        }
        debug_assert_eq!(self.access_cursor, total_cycles);
    }
}

struct FlatRamAdapter {
    cpu: Cpu,
    bus: FlatRam,
}

impl CpuConformanceAdapter for FlatRamAdapter {
    type State = Nes6502State;

    const TRACE_TIMING: TraceTiming = TraceTiming::Exact;

    fn from_case(case: &CpuCase<Self::State>) -> Self {
        let mut adapter = Self {
            cpu: Cpu::new(),
            bus: FlatRam::new(),
        };
        adapter.apply_state(&case.initial_state);
        adapter.bus.load(&case.initial_memory);
        adapter
    }

    fn snapshot(&self) -> Self::State {
        Nes6502State {
            pc: self.cpu.pc,
            sp: self.cpu.sp,
            a: self.cpu.regs.a,
            x: self.cpu.regs.x,
            y: self.cpu.regs.y,
            status: self.cpu.regs.p.bits(),
        }
    }

    fn peek8(&self, address: u32) -> u8 {
        self.bus.peek(address)
    }

    fn step(&mut self) -> StepObservation {
        let cpu_cycles = CpuCore::step_cpu(&mut self.cpu, &mut self.bus);
        StepObservation {
            kind: match self.cpu.last_step_kind {
                CpuStepKind::Instruction => StepKind::Instruction,
                CpuStepKind::Nmi | CpuStepKind::Irq => StepKind::Interrupt,
                CpuStepKind::Idle => StepKind::Idle,
            },
            cpu_cycles,
            master_ticks: MasterTicks::new(cpu_cycles),
            master_rate: ClockRate::from_ratio(21_477_272, 12),
            bus_events: self.bus.take_events(),
        }
    }
}

impl FlatRamAdapter {
    fn apply_state(&mut self, state: &Nes6502State) {
        self.cpu.pc = state.pc;
        self.cpu.sp = state.sp;
        self.cpu.regs.a = state.a;
        self.cpu.regs.x = state.x;
        self.cpu.regs.y = state.y;
        self.cpu.regs.p = StatusFlags::from_bits_truncate(state.status);
        self.cpu.state = CpuState::Running;
        self.cpu.cycles = 0;
        self.cpu.last_step_cycles = 0;
        self.cpu.nmi_pending = false;
        self.cpu.irq_line = false;
    }
}

const KIL_OPCODES: [u8; 12] = [
    0x02, 0x12, 0x22, 0x32, 0x42, 0x52, 0x62, 0x72, 0x92, 0xB2, 0xD2, 0xF2,
];

struct ReusableFlatRunner {
    cpu: Cpu,
    bus: FlatRam,
    window_events: Vec<BusAccessEvent>,
}

impl ReusableFlatRunner {
    fn new() -> Self {
        Self {
            cpu: Cpu::new(),
            bus: FlatRam::new(),
            window_events: Vec::new(),
        }
    }

    fn reset_case(&mut self, state: &ProcessorState, initial_memory: &BTreeMap<u16, u8>) {
        self.cpu = Cpu::new();
        self.bus.reset_to_default();
        self.cpu.pc = state.pc;
        self.cpu.sp = state.s;
        self.cpu.regs.a = state.a;
        self.cpu.regs.x = state.x;
        self.cpu.regs.y = state.y;
        self.cpu.regs.p = StatusFlags::from_bits_truncate(state.p);
        self.cpu.cycles = 0;
        self.bus.load_sparse(initial_memory);
    }

    fn snapshot(&self) -> Nes6502State {
        Nes6502State {
            pc: self.cpu.pc,
            sp: self.cpu.sp,
            a: self.cpu.regs.a,
            x: self.cpu.regs.x,
            y: self.cpu.regs.y,
            status: self.cpu.regs.p.bits(),
        }
    }

    fn run_case(&mut self, opcode: u8, vector: &ProcessorCase) -> Result<(), String> {
        let initial_memory = sparse_memory(&vector.name, "initial", &vector.initial.ram)?;
        let expected_memory = sparse_memory(&vector.name, "final", &vector.final_state.ram)?;
        let expected_events = trace_events(&vector.name, &initial_memory, &vector.cycles)?;
        let actual_opcode = initial_memory
            .get(&vector.initial.pc)
            .copied()
            .ok_or_else(|| {
                format!(
                    "{}: initial RAM omits opcode at PC {:#06X}",
                    vector.name, vector.initial.pc
                )
            })?;
        if actual_opcode != opcode {
            return Err(format!(
                "{}: initial opcode {actual_opcode:#04X} does not match file opcode {opcode:#04X}",
                vector.name
            ));
        }

        self.reset_case(&vector.initial, &initial_memory);
        let initial_state = state_from(&vector.initial);
        if self.snapshot() != initial_state {
            return Err(format!("{}: initial CPU state", vector.name));
        }

        let expected_cycles = u64::try_from(expected_events.len()).expect("vector cycle count");
        let kil = KIL_OPCODES.contains(&opcode);
        let cpu_cycles = if kil {
            self.run_kil_window(expected_cycles, &vector.name)?
        } else {
            let cpu_cycles = CpuCore::step_cpu(&mut self.cpu, &mut self.bus);
            if self.cpu.last_step_kind != CpuStepKind::Instruction {
                return Err(format!("{}: step was not an instruction", vector.name));
            }
            cpu_cycles
        };
        if cpu_cycles != expected_cycles {
            return Err(format!(
                "{}: CPU cycles {cpu_cycles}, expected {expected_cycles}",
                vector.name
            ));
        }
        let actual_events = if kil {
            self.window_events.as_slice()
        } else {
            self.bus.events.as_slice()
        };
        if actual_events != expected_events.as_slice() {
            let mismatch = actual_events
                .iter()
                .zip(&expected_events)
                .position(|(actual, expected)| actual != expected);
            return Err(match mismatch {
                Some(index) => format!(
                    "{}: bus event {index} {:?}, expected {:?}",
                    vector.name, actual_events[index], expected_events[index]
                ),
                None => format!(
                    "{}: {} bus events, expected {}",
                    vector.name,
                    actual_events.len(),
                    expected_events.len()
                ),
            });
        }
        let observed_cycles = if kil {
            cpu_cycles
        } else {
            self.bus.access_cursor
        };
        if observed_cycles != expected_cycles {
            return Err(format!(
                "{}: bus cycles {}, expected {expected_cycles}",
                vector.name, observed_cycles
            ));
        }

        let expected_state = state_from(&vector.final_state);
        let actual_state = self.snapshot();
        if actual_state != expected_state {
            return Err(format!(
                "{}: final CPU state {actual_state:?}, expected {expected_state:?}",
                vector.name
            ));
        }
        for (&address, &value) in &expected_memory {
            let actual = self.bus.peek(u32::from(address));
            if actual != value {
                return Err(format!(
                    "{}: final RAM {address:#06X} is {actual:#04X}, expected {value:#04X}",
                    vector.name
                ));
            }
        }

        let expected_cpu_state = if kil {
            CpuState::Halted
        } else {
            CpuState::Running
        };
        if self.cpu.state != expected_cpu_state {
            return Err(format!(
                "{}: CPU state {:?}, expected {:?}",
                vector.name, self.cpu.state, expected_cpu_state
            ));
        }
        Ok(())
    }

    fn run_kil_window(&mut self, expected_cycles: u64, name: &str) -> Result<u64, String> {
        self.window_events.clear();
        let mut elapsed_cycles = 0;

        let initial_cycles = CpuCore::step_cpu(&mut self.cpu, &mut self.bus);
        if initial_cycles != 2 || self.cpu.last_step_kind != CpuStepKind::Instruction {
            return Err(format!("{name}: KIL initial instruction window"));
        }
        self.require_halted_kil_step(name, initial_cycles)?;
        self.append_rebased_events(elapsed_cycles);
        elapsed_cycles += initial_cycles;

        while elapsed_cycles < expected_cycles {
            let idle_cycles = CpuCore::step_cpu(&mut self.cpu, &mut self.bus);
            if idle_cycles != 1 || self.cpu.last_step_kind != CpuStepKind::Idle {
                return Err(format!("{name}: KIL halted observation step"));
            }
            self.require_halted_kil_step(name, idle_cycles)?;
            self.append_rebased_events(elapsed_cycles);
            elapsed_cycles += idle_cycles;
        }

        if elapsed_cycles != expected_cycles {
            return Err(format!(
                "{name}: KIL observation cycles {elapsed_cycles}, expected {expected_cycles}"
            ));
        }
        Ok(elapsed_cycles)
    }

    fn require_halted_kil_step(&self, name: &str, cycles: u64) -> Result<(), String> {
        if self.cpu.state != CpuState::Halted {
            return Err(format!("{name}: KIL did not halt CPU"));
        }
        if self.bus.events.len() != usize::try_from(cycles).expect("KIL cycle count") {
            return Err(format!(
                "{name}: KIL step has {} bus events for {cycles} cycles",
                self.bus.events.len()
            ));
        }
        if self.bus.access_cursor != cycles {
            return Err(format!(
                "{name}: KIL bus cycles {}, expected {cycles}",
                self.bus.access_cursor
            ));
        }
        Ok(())
    }

    fn append_rebased_events(&mut self, base: u64) {
        self.window_events.extend(
            self.bus
                .events
                .iter()
                .copied()
                .map(|event| rebase_event(event, base)),
        );
    }
}

fn rebase_event(event: BusAccessEvent, base: u64) -> BusAccessEvent {
    match event {
        BusAccessEvent::Read {
            at,
            space,
            addr,
            value,
            width,
            mapped_addr,
        } => BusAccessEvent::Read {
            at: at.map(|at| MasterTicks::new(base + at.get())),
            space,
            addr,
            value,
            width,
            mapped_addr,
        },
        BusAccessEvent::Write {
            at,
            space,
            addr,
            old_value,
            written_value,
            new_value,
            width,
            mapped_addr,
        } => BusAccessEvent::Write {
            at: at.map(|at| MasterTicks::new(base + at.get())),
            space,
            addr,
            old_value,
            written_value,
            new_value,
            width,
            mapped_addr,
        },
    }
}

#[derive(Debug, PartialEq, Eq)]
struct CorpusOptions {
    directory: PathBuf,
    opcode: Option<u8>,
    case_limit: Option<usize>,
    runs: usize,
}

fn corpus_options_from<F>(get: F) -> Result<CorpusOptions, String>
where
    F: Fn(&str) -> Option<String>,
{
    let directory = get("ZEFF_NES6502_V1_DIR")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "ZEFF_NES6502_V1_DIR must name a local SingleStepTests nes6502/v1 directory".to_owned()
        })?;
    let opcode = get("ZEFF_NES6502_OPCODE")
        .map(|value| parse_opcode(&value))
        .transpose()?;
    let case_limit = get("ZEFF_NES6502_CASE_LIMIT")
        .map(|value| parse_positive(&value, "ZEFF_NES6502_CASE_LIMIT"))
        .transpose()?;
    let runs = get("ZEFF_NES6502_RUNS")
        .map(|value| parse_runs(&value))
        .transpose()?
        .unwrap_or(1);

    Ok(CorpusOptions {
        directory: PathBuf::from(directory),
        opcode,
        case_limit,
        runs,
    })
}

fn full_corpus_options_from<F>(get: F) -> Result<CorpusOptions, String>
where
    F: Fn(&str) -> Option<String>,
{
    let directory = get("ZEFF_NES6502_V1_DIR")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "ZEFF_NES6502_V1_DIR must name a local SingleStepTests nes6502/v1 directory".to_owned()
        })?;
    for name in ["ZEFF_NES6502_OPCODE", "ZEFF_NES6502_CASE_LIMIT"] {
        if get(name).is_some() {
            return Err(format!("{name} is not allowed for the full corpus test"));
        }
    }
    if let Some(runs) = get("ZEFF_NES6502_RUNS")
        && parse_runs(&runs)? != 2
    {
        return Err("ZEFF_NES6502_RUNS must be 2 for the full corpus test".to_owned());
    }

    Ok(CorpusOptions {
        directory: PathBuf::from(directory),
        opcode: None,
        case_limit: None,
        runs: 2,
    })
}

fn parse_opcode(value: &str) -> Result<u8, String> {
    if value.len() != 2 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("ZEFF_NES6502_OPCODE must be exactly two hexadecimal digits".to_owned());
    }
    u8::from_str_radix(value, 16)
        .map_err(|_| "ZEFF_NES6502_OPCODE must be exactly two hexadecimal digits".to_owned())
}

fn parse_positive(value: &str, name: &str) -> Result<usize, String> {
    match value.parse() {
        Ok(0) | Err(_) => Err(format!("{name} must be a positive integer")),
        Ok(value) => Ok(value),
    }
}

fn parse_runs(value: &str) -> Result<usize, String> {
    match value {
        "1" => Ok(1),
        "2" => Ok(2),
        _ => Err("ZEFF_NES6502_RUNS must be 1 or 2".to_owned()),
    }
}

fn requested_opcodes(opcode: Option<u8>) -> Vec<u8> {
    match opcode {
        Some(opcode) => vec![opcode],
        None => (0..=u8::MAX).collect(),
    }
}

fn opcode_path(directory: &Path, opcode: u8) -> PathBuf {
    directory.join(format!("{opcode:02x}.json"))
}

fn corpus_paths(options: &CorpusOptions) -> Result<Vec<(u8, PathBuf)>, String> {
    if !options.directory.is_dir() {
        return Err(format!(
            "ZEFF_NES6502_V1_DIR is not a directory: {}",
            options.directory.display()
        ));
    }

    let mut paths = Vec::new();
    let mut missing = Vec::new();
    for opcode in requested_opcodes(options.opcode) {
        let path = opcode_path(&options.directory, opcode);
        if path.is_file() {
            paths.push((opcode, path));
        } else {
            missing.push(format!("{opcode:02x}.json"));
        }
    }
    if missing.is_empty() {
        Ok(paths)
    } else {
        Err(format!(
            "{} is missing {}",
            options.directory.display(),
            missing.join(", ")
        ))
    }
}

fn read_processor_file(path: &Path) -> Result<Vec<ProcessorCase>, String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_reader(BufReader::new(file))
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn run_processor_file(
    runner: &mut ReusableFlatRunner,
    opcode: u8,
    path: &Path,
    options: &CorpusOptions,
) -> Result<(), String> {
    let cases = read_processor_file(path)?;
    if cases.len() != 10_000 {
        return Err(format!(
            "{}: expected 10000 vectors, found {}",
            path.display(),
            cases.len()
        ));
    }

    for (index, vector) in cases
        .into_iter()
        .take(options.case_limit.unwrap_or(usize::MAX))
        .enumerate()
    {
        if KIL_OPCODES.contains(&opcode) && vector.cycles.len() != 11 {
            return Err(format!(
                "{}: case {index} {:?}: KIL corpus window has {} cycles, expected 11",
                path.display(),
                vector.name,
                vector.cycles.len()
            ));
        }
        for run in 0..options.runs {
            runner.run_case(opcode, &vector).map_err(|error| {
                format!(
                    "{}: case {index} {:?}, run {}: {error}",
                    path.display(),
                    vector.name,
                    run + 1
                )
            })?;
        }
    }
    Ok(())
}

#[test]
fn nes6502_v1_vectors_run_through_common_adapter() {
    let parsed = parse_cases(include_str!("fixtures/nes6502-v1-flat-ram.json"))
        .expect("valid processor vectors");
    assert_eq!(parsed.len(), 6, "fixture coverage");
    for parsed_case in parsed {
        eprintln!("processor vector: {}", parsed_case.name);
        assert_case::<FlatRamAdapter>(&parsed_case.case);
    }
}

#[test]
fn reusable_runner_resets_dirty_ram_and_cpu_state() {
    let vectors: Vec<ProcessorCase> =
        serde_json::from_str(include_str!("fixtures/nes6502-v1-flat-ram.json"))
            .expect("valid processor vectors");
    let mut runner = ReusableFlatRunner::new();

    runner
        .run_case(0x8D, &vectors[0])
        .expect("first store case");
    assert_eq!(runner.bus.peek(0x0300), 0x7E, "store changed flat RAM");
    let next_memory = sparse_memory(&vectors[1].name, "initial", &vectors[1].initial.ram)
        .expect("next sparse RAM");
    runner.reset_case(&vectors[1].initial, &next_memory);
    assert_eq!(runner.bus.peek(0x0300), 0xFF, "dirty RAM restored");

    for vector in &vectors {
        let opcode = sparse_memory(&vector.name, "initial", &vector.initial.ram)
            .expect("sparse RAM")[&vector.initial.pc];
        runner.run_case(opcode, vector).expect("first replay");
    }
    assert_eq!(runner.cpu.state, CpuState::Halted, "KIL vector halts CPU");

    runner
        .run_case(0x8D, &vectors[0])
        .expect("KIL reset replay");
    assert_eq!(runner.cpu.state, CpuState::Running, "next case resets CPU");
    for vector in &vectors {
        let opcode = sparse_memory(&vector.name, "initial", &vector.initial.ram)
            .expect("sparse RAM")[&vector.initial.pc];
        runner.run_case(opcode, vector).expect("second replay");
    }
}

#[test]
fn corpus_options_and_inventory_helpers_are_strict() {
    let options = corpus_options_from(|name| match name {
        "ZEFF_NES6502_V1_DIR" => Some("C:/vectors/nes6502/v1".to_owned()),
        "ZEFF_NES6502_OPCODE" => Some("B2".to_owned()),
        "ZEFF_NES6502_CASE_LIMIT" => Some("17".to_owned()),
        "ZEFF_NES6502_RUNS" => Some("2".to_owned()),
        _ => None,
    })
    .expect("valid options");
    assert_eq!(options.opcode, Some(0xB2));
    assert_eq!(options.case_limit, Some(17));
    assert_eq!(options.runs, 2);
    assert_eq!(
        opcode_path(Path::new("vectors"), 0),
        PathBuf::from("vectors/00.json")
    );
    assert_eq!(
        opcode_path(Path::new("vectors"), 0xFF),
        PathBuf::from("vectors/ff.json")
    );
    assert_eq!(requested_opcodes(None).len(), 256);
    assert_eq!(requested_opcodes(Some(0x69)), vec![0x69]);

    assert!(parse_opcode("6").is_err());
    assert!(parse_opcode("gg").is_err());
    assert!(parse_positive("0", "LIMIT").is_err());
    assert!(parse_runs("3").is_err());

    let full = full_corpus_options_from(|name| match name {
        "ZEFF_NES6502_V1_DIR" => Some("C:/vectors/nes6502/v1".to_owned()),
        _ => None,
    })
    .expect("valid full corpus options");
    assert_eq!(full.opcode, None);
    assert_eq!(full.case_limit, None);
    assert_eq!(full.runs, 2);
    assert!(
        full_corpus_options_from(|name| match name {
            "ZEFF_NES6502_V1_DIR" => Some("C:/vectors/nes6502/v1".to_owned()),
            "ZEFF_NES6502_CASE_LIMIT" => Some("1".to_owned()),
            _ => None,
        })
        .is_err()
    );
}

#[test]
fn reusable_runner_rejects_opcode_mismatch() {
    let vectors: Vec<ProcessorCase> =
        serde_json::from_str(include_str!("fixtures/nes6502-v1-flat-ram.json"))
            .expect("valid processor vectors");
    assert!(
        ReusableFlatRunner::new()
            .run_case(0x69, &vectors[0])
            .expect_err("opcode file mismatch should fail")
            .contains("does not match file opcode")
    );
}

#[test]
fn kil_window_rebases_halted_step_events() {
    let event = BusAccessEvent::Read {
        at: Some(MasterTicks::new(0)),
        space: TraceWriteKind::Memory,
        addr: 0xFFFF,
        value: 0xA5,
        width: TraceWriteWidth::Byte,
        mapped_addr: None,
    };
    assert_eq!(
        rebase_event(event, 2),
        BusAccessEvent::Read {
            at: Some(MasterTicks::new(2)),
            space: TraceWriteKind::Memory,
            addr: 0xFFFF,
            value: 0xA5,
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        }
    );
}

#[test]
#[ignore = "requires a local SingleStepTests 65x02 nes6502/v1 corpus"]
fn nes6502_v1_full_corpus() {
    let options =
        full_corpus_options_from(|name| std::env::var(name).ok()).expect("full corpus options");
    let paths = corpus_paths(&options).expect("full corpus file inventory");
    assert_eq!(paths.len(), 256, "full opcode inventory");
    let mut runner = ReusableFlatRunner::new();
    let mut case_count = 0;
    let mut execution_count = 0;
    for (opcode, path) in paths {
        run_processor_file(&mut runner, opcode, &path, &options)
            .unwrap_or_else(|error| panic!("{error}"));
        case_count += 10_000;
        execution_count += 10_000 * options.runs;
    }
    assert_eq!(case_count, 2_560_000, "full vector count");
    assert_eq!(execution_count, 5_120_000, "full execution count");
}

#[test]
#[ignore = "requires a local SingleStepTests 65x02 nes6502/v1 corpus"]
fn nes6502_v1_diagnostic_corpus() {
    let options = corpus_options_from(|name| std::env::var(name).ok()).expect("corpus options");
    let paths = corpus_paths(&options).expect("corpus file inventory");
    let mut runner = ReusableFlatRunner::new();
    for (opcode, path) in paths {
        run_processor_file(&mut runner, opcode, &path, &options)
            .unwrap_or_else(|error| panic!("{error}"));
    }
}

#[test]
fn parser_rejects_duplicate_entries() {
    let duplicate = r#"[{"name":"duplicate","initial":{"pc":0,"s":0,"a":0,"x":0,"y":0,"p":0,"ram":[[0,0],[0,1]]},"final":{"pc":0,"s":0,"a":0,"x":0,"y":0,"p":0,"ram":[]},"cycles":[]}]"#;
    assert!(
        parse_cases(duplicate)
            .err()
            .expect("duplicate entry should fail")
            .contains("repeats address")
    );
}

#[test]
fn parser_rejects_missing_reads_and_accepts_default_writes() {
    let missing_read = r#"[{"name":"missing-read","initial":{"pc":0,"s":0,"a":0,"x":0,"y":0,"p":0,"ram":[]},"final":{"pc":0,"s":0,"a":0,"x":0,"y":0,"p":0,"ram":[]},"cycles":[[0,0,"read"]]}]"#;
    assert!(
        parse_cases(missing_read)
            .err()
            .expect("missing read memory should fail")
            .contains("without initial memory")
    );

    let missing_write = r#"[{"name":"missing-write","initial":{"pc":0,"s":0,"a":0,"x":0,"y":0,"p":0,"ram":[]},"final":{"pc":0,"s":0,"a":0,"x":0,"y":0,"p":0,"ram":[]},"cycles":[[0,0,"write"]]}]"#;
    let parsed = parse_cases(missing_write).expect("baseline write is valid");
    assert!(matches!(
        parsed[0].case.expected_step.bus_events.as_slice(),
        [BusAccessEvent::Write {
            old_value: 0xFF,
            written_value: 0,
            ..
        }]
    ));
}
