use super::*;

pub(super) const DN: [u32; 8] = [
    0xa1000000, 0xa1000020, 0xa1000040, 0xa1000060, 0xa1000080, 0xa10000a0, 0xa10000c0, 0xa10000e0,
];

pub(super) fn state(a0: u32, sr: u16, usp: u32, ssp: u32, pc: u32) -> Registers {
    let mut registers = Registers::new();
    registers.d = DN;
    registers.a = [
        a0, 0xb2000040, 0xb2000080, 0xb20000c0, 0xb2000100, 0xb2000140, 0xb2000180,
    ];
    registers.usp = usp;
    registers.ssp = ssp;
    registers.pc = pc;
    registers.set_status_register(sr);
    registers
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

pub(super) struct Case {
    pub label: &'static str,
    pub opcode: u16,
    pub start: u64,
    pub initial: Registers,
    pub queue: [u16; 2],
    pub ram: Vec<(u32, u16)>,
    pub waits: Vec<u32>,
    pub fail: Option<usize>,
    pub outcome: Outcome,
    pub final_registers: Registers,
    pub final_queue: [u16; 2],
    pub final_ram: Vec<(u32, u16)>,
    pub clock: u64,
    pub duration: u64,
    pub events: Vec<device::Event>,
}
