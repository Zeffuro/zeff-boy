use super::*;

#[derive(Clone, Copy)]
pub(super) enum Action {
    Reserve,
    Idle,
    Address(u32),
    Flags(u16),
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
    if source == 6 {
        2
    } else {
        usize::from(source >= 3)
    }
}

fn arithmetic(sr: u16, family: u16, value: u32, long: bool) -> (u32, u16) {
    let bits = if long { 32 } else { 16 };
    let modulus = 1u64 << bits;
    let sign = modulus >> 1;
    let source = u64::from(value) & (modulus - 1);
    let incoming = u64::from(family == 0x4000 && sr & 0x10 != 0);
    let extended = source + incoming;
    let result = 0u64.wrapping_sub(extended) & (modulus - 1);
    let signed = if source & sign == 0 {
        source as i64
    } else {
        source as i64 - modulus as i64
    };
    let mathematical = -signed - incoming as i64;
    let overflow = mathematical < -(sign as i64) || mathematical >= sign as i64;
    let zero = result == 0 && (family == 0x4400 || sr & 4 != 0);
    let flags = (u16::from(extended != 0) * 0x11)
        | (u16::from(zero) * 4)
        | (u16::from(result & sign != 0) * 8)
        | (u16::from(overflow) * 2);
    (result as u32, sr & !31 | flags)
}

pub(super) fn stages(sr: u16, family: u16, source: u32, long: bool) -> (u32, u16, u16) {
    assert!(matches!(family, 0x4000 | 0x4400));
    let (result, full) = arithmetic(sr, family, source, long);
    let (low, partial) = arithmetic(sr, family, source, false);
    if long {
        let incoming = u64::from(family == 0x4000 && sr & 0x10 != 0);
        let borrow = u64::from(u64::from(source & 0xffff) + incoming != 0);
        let high_source = u64::from(source >> 16);
        let high = 0u64.wrapping_sub(high_source + borrow) & 0xffff;
        assert_eq!(result, (high as u32) << 16 | low);
        let flags = (u16::from(high_source + borrow != 0) * 0x11)
            | (u16::from(partial & 4 != 0 && high == 0) * 4)
            | (u16::from(high & 0x8000 != 0) * 8)
            | (u16::from(high_source & high & 0x8000 != 0) * 2);
        assert_eq!(full, sr & !31 | flags);
    }
    (result, partial, full)
}
