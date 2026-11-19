use super::*;
use value::Value;

pub(super) type Step = Result<PhysicalInstructionStep, InstructionFault<&'static str>>;

#[derive(Clone, Copy, Debug)]
pub(super) struct Image {
    pub registers: Registers,
    pub queue: Option<[u16; 2]>,
    pub clock: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Complete,
    Host,
    Boundary,
    Unavailable,
    Critical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Transfer {
        stage: u8,
        access: BusAccess,
        direction: Direction,
        value: Option<u16>,
        start: u64,
        elapsed: u32,
        outcome: Outcome,
        deposited: bool,
    },
    Idle(u64, u32),
}

pub(super) struct Case {
    pub label: &'static str,
    pub raw: Value,
    pub initial: Image,
    pub expected: Image,
    pub memory: Vec<(u32, u16)>,
    pub final_memory: Vec<(u32, u16)>,
    pub events: Vec<Event>,
    pub result: Step,
    pub state: PhysicalEntryState,
    pub inspection: PhysicalInspection,
    pub retry: Option<TimingFault<&'static str>>,
    pub frozen: bool,
    pub queue_address: Option<u32>,
    pub capture: Option<(Group0Capture, u16)>,
    pub capture_observable: bool,
    pub frame: Vec<(u32, u16)>,
    pub before_entry: Option<(u64, BTreeMap<u32, u16>)>,
    pub entry_event: Option<usize>,
    pub counts: [usize; 5],
    pub mutations: Vec<(usize, u32, u16)>,
    pub elapsed: u64,
    pub initial_level: u8,
    pub initial_edge: bool,
    pub expected_level: u8,
    pub expected_edge: bool,
    pub default_detector: bool,
    pub next_rejection: Option<InstructionFault<&'static str>>,
}
