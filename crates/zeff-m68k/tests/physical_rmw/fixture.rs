use super::*;
use device::{Device, Event, Kind, Stage};

#[derive(Clone, Copy, Debug)]
pub(super) struct Image {
    pub r: Registers,
    pub queue: Option<[u16; 2]>,
    pub clock: u64,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Transfer {
    pub event: Event,
    pub boundary: Option<[u32; 7]>,
    pub phase: Option<InstructionPhase>,
}
#[derive(Clone, Copy, Debug)]
pub(super) enum ResultKind {
    Instruction,
    Group0,
    Host,
    Unavailable,
    Blocked,
}
pub(super) struct Case {
    pub index: usize,
    pub label: &'static str,
    pub source: usize,
    pub initial: Image,
    pub expected: Image,
    pub memory: Vec<(u32, u16)>,
    pub final_memory: Vec<(u32, u16)>,
    pub transfers: Vec<Transfer>,
    pub kind: ResultKind,
    pub capture: Option<(Group0Capture, u16)>,
    pub duration: u64,
    pub writes: Vec<(u32, u16)>,
    pub frame: Vec<(u32, u16)>,
}
pub(super) fn registers(values: [u32; 7]) -> Registers {
    let [a0, usp, ssp, sr, pc, _, _] = values;
    let mut r = Registers::new();
    r.d = [
        286331153, 572662306, 858993459, 1145324612, 1431655765, 1717986918, 2004318071, 2290649224,
    ];
    r.a = [a0, 16384, 20480, 24576, 28672, 32768, 36864];
    r.usp = usp;
    r.ssp = ssp;
    r.pc = pc;
    r.set_status_register(sr as u16);
    r
}
pub(super) fn image(values: [u32; 7], clock: u64, present: bool) -> Image {
    Image {
        r: registers(values),
        queue: present.then_some([values[5] as u16, values[6] as u16]),
        clock,
    }
}
pub(super) fn access(values: (u32, u32, u8), kind: AccessKind) -> BusAccess {
    BusAccess {
        logical_address: values.0,
        bus_address: values.1,
        function_code: fc(values.2),
        kind,
        lanes: ByteLanes::Both,
    }
}
pub(super) fn fc(value: u8) -> FunctionCode {
    match value {
        1 => FunctionCode::UserData,
        2 => FunctionCode::UserProgram,
        5 => FunctionCode::SupervisorData,
        6 => FunctionCode::SupervisorProgram,
        _ => panic!("literal FC"),
    }
}
pub(super) fn instruction(
    phase: InstructionPhase,
    route: (u32, u32, u8),
    data: (u16, u64, u32, Kind, bool),
    boundary: [u32; 7],
) -> Transfer {
    let direction = if matches!(
        phase,
        InstructionPhase::OperandWriteHigh | InstructionPhase::OperandWriteLow
    ) {
        Direction::Write
    } else {
        Direction::Read
    };
    let kind = if phase == InstructionPhase::OperandRefill {
        AccessKind::Prefetch
    } else {
        AccessKind::Data
    };
    Transfer {
        event: Event::Transfer {
            stage: Stage::Instruction,
            access: access(route, kind),
            direction,
            value: data.0,
            start: data.1,
            elapsed: data.2,
            outcome: data.3,
            deposited: data.4,
        },
        boundary: Some(boundary),
        phase: Some(phase),
    }
}
pub(super) fn entry(
    route: (u32, u32, u8),
    data: (u16, u64, u32),
    kind: AccessKind,
    direction: Direction,
) -> Transfer {
    Transfer {
        event: Event::Transfer {
            stage: Stage::Entry,
            access: access(route, kind),
            direction,
            value: data.0,
            start: data.1,
            elapsed: data.2,
            outcome: Kind::Complete,
            deposited: direction == Direction::Write,
        },
        boundary: None,
        phase: None,
    }
}
pub(super) fn idle(start: u64, elapsed: u32) -> Transfer {
    Transfer {
        event: Event::Idle(start, elapsed),
        boundary: None,
        phase: None,
    }
}
pub(super) fn capture(values: [u32; 7], direction: Direction) -> (Group0Capture, u16) {
    (
        Group0Capture {
            vector: values[0] as u8,
            internal_pc: values[1],
            phase_sr: values[2] as u16,
            ird: values[3] as u16,
            logical_address: values[4],
            function_code: fc(values[5] as u8),
            direction,
            processing_class: ProcessingClass::InstructionOrGroup2,
            ssw_profile: SswProfile::SelectedIrdUpperBits,
        },
        values[6] as u16,
    )
}
impl Case {
    pub fn device(&self) -> Device {
        let mut bus = Device::new(self.initial.clock);
        bus.words = self.memory.iter().copied().collect();
        let mut index = 0;
        for t in &self.transfers {
            if let Event::Transfer {
                stage: Stage::Instruction,
                outcome,
                elapsed,
                deposited,
                ..
            } = t.event
            {
                if outcome != Kind::Complete {
                    bus.injection = Some((index, outcome, elapsed, deposited));
                }
                index += 1;
            }
        }
        bus
    }
}
