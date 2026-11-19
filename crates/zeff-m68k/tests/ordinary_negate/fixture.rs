use super::*;

pub(super) fn state(a0: u32, sr: u16, usp: u32, ssp: u32, pc: u32) -> Registers {
    let mut r = Registers::new();
    r.d = [
        0xa1000000, 0xa1000020, 0xa1000040, 0xa1000060, 0xa1000080, 0xa10000a0, 0xa10000c0,
        0xa10000e0,
    ];
    r.a = [
        a0, 0xb2000040, 0xb2000080, 0xb20000c0, 0xb2000100, 0xb2000140, 0xb2000180,
    ];
    r.usp = usp;
    r.ssp = ssp;
    r.pc = pc;
    r.set_status_register(sr);
    r
}

pub(super) fn access(logical: u32, bus: u32, fc: u8, kind: AccessKind) -> BusAccess {
    BusAccess {
        logical_address: logical,
        bus_address: bus,
        function_code: match fc {
            1 => FunctionCode::UserData,
            2 => FunctionCode::UserProgram,
            5 => FunctionCode::SupervisorData,
            6 => FunctionCode::SupervisorProgram,
            _ => unreachable!(),
        },
        kind,
        lanes: ByteLanes::Both,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Complete,
    Host(BusAccess, Direction, usize),
    Overflow,
}

pub(super) struct Stage {
    pub phase: &'static str,
    pub sr: u16,
    pub source_an: u32,
    pub observation: &'static str,
}

pub(super) struct Expected {
    pub r: Registers,
    pub queue: [u16; 2],
    pub queue_address: u32,
    pub ram: Vec<(u32, u16)>,
    pub clock: u64,
    pub duration: u64,
    pub outcome: Outcome,
    pub frozen: bool,
    pub trace: bool,
    pub stopped: bool,
    pub state: PhysicalEntryState,
    pub inspection: PhysicalInspection,
    pub interrupt_state: &'static str,
    pub retry: Option<&'static str>,
}

pub(super) struct Case {
    pub label: &'static str,
    pub profile: &'static str,
    pub route: &'static str,
    pub opcode: u16,
    pub size: u8,
    pub source: u32,
    pub result: u32,
    pub low_result: u16,
    pub partial_sr: u16,
    pub full_sr: u16,
    pub original_x: u8,
    pub published_low_x: u8,
    pub start: u64,
    pub waits: Vec<u32>,
    pub fail: Option<usize>,
    pub failed_write_deposits: Option<bool>,
    pub blocked: Option<usize>,
    pub initial: Registers,
    pub queue: [u16; 2],
    pub queue_address: u32,
    pub ram: Vec<(u32, u16)>,
    pub events: Vec<device::Event>,
    pub stages: Vec<Stage>,
    pub expected: Expected,
}

pub(super) fn consistent(c: &Case) -> bool {
    let source = if c.size == 4 {
        u32::from(c.events[0].value) << 16 | u32::from(c.events[1].value)
    } else {
        u32::from(c.events[0].value)
    };
    c.label.contains(c.profile)
        && c.label.ends_with(c.route)
        && c.source == source
        && c.low_result == c.result as u16
        && c.original_x == u8::from(c.initial.status_register() & 16 != 0)
        && c.published_low_x == u8::from(c.partial_sr & 16 != 0)
        && c.queue_address == c.initial.pc
        && c.queue[0] == c.opcode
        && c.expected.queue_address == c.expected.r.pc
        && c.expected.duration == c.expected.clock - c.start
        && c.expected.frozen == (c.expected.outcome != Outcome::Complete)
        && c.expected.retry.is_some() == c.expected.frozen
        && c.expected.interrupt_state == "default unchanged"
        && c.events.len() == c.stages.len()
        && c.events
            .iter()
            .zip(&c.stages)
            .enumerate()
            .all(|(at, (event, stage))| {
                let size = u32::from(c.size);
                let active = 0x3000 + if c.size == 2 || at > 0 { size } else { 0 };
                stage.observation == "SR/An stage contract only; no live CPU metadata access"
                    && stage.source_an == active
                    && stage.sr
                        == match stage.phase {
                            "RH" | "RL" => c.initial.status_register(),
                            "F" => c.partial_sr,
                            "WL" | "WH" | "W" => c.full_sr,
                            _ => return false,
                        }
                    && event.wait == c.waits[at]
                    && event.elapsed == 4 + u64::from(event.wait)
                    && event.host == (c.fail == Some(at))
                    && event.deposited
                        == (event.direction == Direction::Write
                            && (!event.host || c.failed_write_deposits == Some(true)))
            })
        && c.blocked.is_none_or(|at| {
            at == c.events.len() && c.expected.clock.checked_add(4294967299).is_none()
        })
}
