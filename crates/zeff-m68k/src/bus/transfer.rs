use super::{
    AccessKind, BusAccess, BusFault, BusOperation, BusResponse, ByteLanes, Direction, FunctionCode,
    M68kBus, RmwError,
};

pub fn reset_output<B: M68kBus>(
    bus: &mut B,
    asserted_clocks: u32,
) -> Result<(), BusFault<B::Error>> {
    match bus
        .reset_output(asserted_clocks)
        .map_err(|source| BusFault::ResetOutput { source })?
    {
        BusResponse::Complete(()) => Ok(()),
        BusResponse::Unsupported => Err(BusFault::UnsupportedOperation {
            operation: BusOperation::ResetOutput,
        }),
    }
}

pub fn test_and_set_byte<B: M68kBus>(
    bus: &mut B,
    address: u32,
    function_code: FunctionCode,
) -> Result<u8, BusFault<B::Error>> {
    let access = byte_access(address, function_code, AccessKind::Data);
    let result = bus.test_and_set(access).map_err(|error| {
        let (direction, source) = match error {
            RmwError::Read(source) => (Direction::Read, source),
            RmwError::Write(source) => (Direction::Write, source),
        };
        BusFault::Transfer {
            access,
            direction,
            source,
        }
    })?;
    match result {
        BusResponse::Complete(value) => Ok(if access.lanes == ByteLanes::Upper {
            (value >> 8) as u8
        } else {
            value as u8
        }),
        BusResponse::Unsupported => Err(BusFault::UnsupportedOperation {
            operation: BusOperation::TestAndSet,
        }),
    }
}

fn byte_access(address: u32, function_code: FunctionCode, kind: AccessKind) -> BusAccess {
    BusAccess::new(
        address,
        function_code,
        kind,
        if address & 1 == 0 {
            ByteLanes::Upper
        } else {
            ByteLanes::Lower
        },
    )
}

fn word_access<E>(
    address: u32,
    function_code: FunctionCode,
    kind: AccessKind,
    direction: Direction,
) -> Result<BusAccess, BusFault<E>> {
    let access = BusAccess::new(address, function_code, kind, ByteLanes::Both);
    if address & 1 != 0 {
        return Err(BusFault::AddressError { access, direction });
    }
    Ok(access)
}

pub fn read_byte<B: M68kBus>(
    bus: &mut B,
    address: u32,
    function_code: FunctionCode,
    kind: AccessKind,
) -> Result<u8, BusFault<B::Error>> {
    let access = byte_access(address, function_code, kind);
    let value = bus.read(access).map_err(|source| BusFault::Transfer {
        access,
        direction: Direction::Read,
        source,
    })?;
    Ok(match access.lanes {
        ByteLanes::Upper => (value >> 8) as u8,
        ByteLanes::Lower => value as u8,
        ByteLanes::Both => unreachable!(),
    })
}

pub fn read_word<B: M68kBus>(
    bus: &mut B,
    address: u32,
    function_code: FunctionCode,
    kind: AccessKind,
) -> Result<u16, BusFault<B::Error>> {
    let access = word_access(address, function_code, kind, Direction::Read)?;
    bus.read(access).map_err(|source| BusFault::Transfer {
        access,
        direction: Direction::Read,
        source,
    })
}

/// Forward high-word-first transfer; instruction-specific predecrement order is separate.
pub fn read_long<B: M68kBus>(
    bus: &mut B,
    address: u32,
    function_code: FunctionCode,
    kind: AccessKind,
) -> Result<u32, BusFault<B::Error>> {
    let high = read_word(bus, address, function_code, kind)?;
    let low = read_word(bus, address.wrapping_add(2), function_code, kind)?;
    Ok((u32::from(high) << 16) | u32::from(low))
}

pub fn write_byte<B: M68kBus>(
    bus: &mut B,
    address: u32,
    value: u8,
    function_code: FunctionCode,
    kind: AccessKind,
) -> Result<(), BusFault<B::Error>> {
    let access = byte_access(address, function_code, kind);
    let value = match access.lanes {
        ByteLanes::Upper => u16::from(value) << 8,
        ByteLanes::Lower => u16::from(value),
        ByteLanes::Both => unreachable!(),
    };
    bus.write(access, value)
        .map_err(|source| BusFault::Transfer {
            access,
            direction: Direction::Write,
            source,
        })
}

pub fn write_word<B: M68kBus>(
    bus: &mut B,
    address: u32,
    value: u16,
    function_code: FunctionCode,
    kind: AccessKind,
) -> Result<(), BusFault<B::Error>> {
    let access = word_access(address, function_code, kind, Direction::Write)?;
    bus.write(access, value)
        .map_err(|source| BusFault::Transfer {
            access,
            direction: Direction::Write,
            source,
        })
}

/// A failed second transfer leaves the completed high-word write observable.
pub fn write_long<B: M68kBus>(
    bus: &mut B,
    address: u32,
    value: u32,
    function_code: FunctionCode,
    kind: AccessKind,
) -> Result<(), BusFault<B::Error>> {
    write_word(bus, address, (value >> 16) as u16, function_code, kind)?;
    write_word(
        bus,
        address.wrapping_add(2),
        value as u16,
        function_code,
        kind,
    )
}
