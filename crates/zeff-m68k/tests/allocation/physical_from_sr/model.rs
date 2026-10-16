use super::*;

#[derive(Clone, Copy)]
pub(super) enum Action {
    Reserve,
    Idle,
    Address(u32),
    Data(u32),
    Read(BusAccess),
    Write(BusAccess, u16),
    Commit,
}

#[derive(Clone, Copy)]
pub(super) enum Path {
    Complete,
    Host(usize),
    Blocked(usize),
    Odd,
    Tail(u64),
}

pub(super) fn access(address: u32, supervisor: bool, data: bool) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & 0x00ff_fffe,
        function_code: match (supervisor, data) {
            (false, false) => FunctionCode::UserProgram,
            (false, true) => FunctionCode::UserData,
            (true, false) => FunctionCode::SupervisorProgram,
            (true, true) => FunctionCode::SupervisorData,
        },
        kind: if data {
            AccessKind::Data
        } else {
            AccessKind::Prefetch
        },
        lanes: ByteLanes::Both,
    }
}

pub(super) fn extensions(source: usize) -> usize {
    if source == 7 {
        2
    } else {
        usize::from(source >= 4)
    }
}
