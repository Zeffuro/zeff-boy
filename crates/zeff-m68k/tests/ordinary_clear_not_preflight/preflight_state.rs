use crate::device::Device;
use crate::fixed::Fixed;
use crate::literal::Value;
use crate::oracle::Expected as Primary;
use crate::preflight_axes::PreflightAxis;
use zeff_m68k::Registers;
use zeff_m68k::timing::physical::{
    EntryFault, EntryStep, ImportedGroup0Boundary, PhysicalEntryState, PhysicalInspection,
};
use zeff_m68k::timing::physical_instruction::{InstructionFault, PhysicalInstructionStep};
use zeff_m68k::timing::{TimedOutcome, TimedStep, TimingFault};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cpu {
    pub r: Registers,
    pub queue: Option<[u16; 2]>,
    pub pipeline_address: Option<u32>,
    pub clock: u64,
    pub frozen: bool,
    pub stopped: bool,
    pub trace: bool,
    pub physical: PhysicalEntryState,
    pub inspection: PhysicalInspection,
    pub imported: Option<ImportedGroup0Boundary>,
}

impl Cpu {
    pub fn new(r: Registers, queue: Option<[u16; 2]>, clock: u64) -> Self {
        Self {
            r,
            queue,
            pipeline_address: queue.map(|_| r.pc),
            clock,
            frozen: false,
            stopped: false,
            trace: false,
            physical: PhysicalEntryState::Idle,
            inspection: PhysicalInspection::DiagnosticCompleteValues,
            imported: None,
        }
    }
    pub fn primary(value: &Primary, initial: bool) -> Self {
        if initial {
            Self::new(
                value.initial.r,
                Some(value.initial.queue),
                value.initial.device.clock,
            )
        } else {
            let mut cpu = Self::new(value.r, Some(value.queue), value.device.clock);
            cpu.frozen = value.error.is_some();
            cpu.trace = value.trace;
            cpu
        }
    }
    pub fn debug(&self) -> String {
        let pipeline = self.queue.map_or_else(
            || "None".to_owned(),
            |words| {
                format!(
                    "Some(Pipeline {{ address: {}, words: {:?} }})",
                    self.pipeline_address.unwrap(),
                    words
                )
            },
        );
        format!(
            "PrefetchCpu {{ registers: {:?}, clock: CpuClocks({}), pipeline: {}, frozen: {}, stopped: {}, interrupts: InterruptState {{ level: 0, level7_edge: false }}, pending_trace: {}, physical_state: {:?}, physical_inspection: {:?}, imported_boundary: {:?} }}",
            self.r,
            self.clock,
            pipeline,
            self.frozen,
            self.stopped,
            self.trace,
            self.physical,
            self.inspection,
            self.imported
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Constructor {
    Prefetched {
        r: Registers,
        words: [u16; 2],
        clock: u64,
    },
    Default,
    Imported(ImportedGroup0Boundary),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResultValue {
    Unit,
    Step(Result<TimedStep, TimingFault<usize>>),
    Boundary(Result<TimedOutcome, TimingFault<usize>>),
    Trace(Result<Option<u64>, TimingFault<usize>>),
    Prime(Result<(), TimingFault<usize>>),
    Physical(Result<PhysicalInstructionStep, InstructionFault<usize>>),
    Reset(Result<EntryStep, EntryFault<usize>>),
}

#[derive(Clone, Debug)]
pub struct Record {
    pub method: &'static str,
    pub arguments: Value,
    pub result: ResultValue,
    pub cpu: Cpu,
    pub device: Device,
}

#[derive(Clone, Debug)]
pub struct Expected {
    pub axis: PreflightAxis,
    pub source: Primary,
    pub constructor: Constructor,
    pub constructor_cpu: Cpu,
    pub records: Vec<Record>,
    pub images: Fixed<crate::oracle::Image>,
    pub input_cpu: Cpu,
    pub input_device: Device,
    pub cpu: Cpu,
    pub device: Device,
    pub result: ResultValue,
    pub guard: Option<&'static str>,
}
