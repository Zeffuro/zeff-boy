use super::*;

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
    Boundary,
    Host,
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
    pub writes: Vec<(u32, u16)>,
    pub frame: Vec<(u32, u16)>,
    pub before_entry: Option<(u64, Vec<(u32, u16)>)>,
    pub counts: [usize; 5],
}

pub(super) fn image(v: [u32; 5], queue: Option<[u16; 2]>, clock: u64) -> Image {
    let mut registers = Registers::new();
    registers.d = [
        286331153, 572662306, 858993459, 1145324612, 1431655765, 1717986918, 2004318071, 2290649224,
    ];
    registers.a = [v[0], 16384, 20480, 24576, 28672, 32768, 36864];
    registers.usp = v[1];
    registers.ssp = v[2];
    registers.set_status_register(v[3] as u16);
    registers.pc = v[4];
    Image {
        registers,
        queue,
        clock,
    }
}

pub(super) fn access(v: [u32; 4]) -> BusAccess {
    BusAccess {
        logical_address: v[0],
        bus_address: v[1],
        function_code: match v[2] {
            1 => FunctionCode::UserData,
            2 => FunctionCode::UserProgram,
            5 => FunctionCode::SupervisorData,
            6 => FunctionCode::SupervisorProgram,
            _ => panic!("literal FC"),
        },
        kind: match v[3] {
            0 => AccessKind::Data,
            1 => AccessKind::Prefetch,
            2 => AccessKind::Stack,
            3 => AccessKind::Vector,
            _ => panic!("literal access"),
        },
        lanes: ByteLanes::Both,
    }
}

pub(super) fn event(v: [u64; 11]) -> Event {
    if v[0] == 4 {
        return Event::Idle(v[7], v[8] as u32);
    }
    Event::Transfer {
        stage: v[0] as u8,
        access: access([v[1] as u32, v[2] as u32, v[3] as u32, v[4] as u32]),
        direction: if v[5] == 0 {
            Direction::Read
        } else {
            Direction::Write
        },
        value: (v[6] != 65536).then_some(v[6] as u16),
        start: v[7],
        elapsed: v[8] as u32,
        outcome: match v[9] {
            0 => Outcome::Complete,
            1 => Outcome::Boundary,
            2 => Outcome::Host,
            3 => Outcome::Unavailable,
            4 => Outcome::Critical,
            _ => panic!("literal report"),
        },
        deposited: v[10] != 0,
    }
}

pub(super) fn capture(v: [u32; 7], direction: Direction) -> (Group0Capture, u16) {
    (
        Group0Capture {
            vector: v[0] as u8,
            internal_pc: v[1],
            phase_sr: v[2] as u16,
            ird: v[3] as u16,
            logical_address: v[4],
            function_code: access([0, 0, v[5], 0]).function_code,
            direction,
            processing_class: ProcessingClass::InstructionOrGroup2,
            ssw_profile: SswProfile::SelectedIrdUpperBits,
        },
        v[6] as u16,
    )
}
