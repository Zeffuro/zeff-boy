pub use crate::constants::BUS_ADDRESS_MASK;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FunctionCode {
    UserData = 1,
    UserProgram = 2,
    SupervisorData = 5,
    SupervisorProgram = 6,
    CpuSpace = 7,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessKind {
    Opcode,
    Extension,
    ResetVector,
    Vector,
    Stack,
    Data,
    Discard,
    Prefetch,
    InterruptAcknowledge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteLanes {
    Upper,
    Lower,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusOperation {
    ResetOutput,
    TestAndSet,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Unsupported leaves the operation unstarted.
pub enum BusResponse<T> {
    Complete(T),
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RmwError<E> {
    Read(E),
    Write(E),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BusAccess {
    pub logical_address: u32,
    /// A23..A1 represented as an aligned byte address; A0 selects the lane.
    pub bus_address: u32,
    pub function_code: FunctionCode,
    pub kind: AccessKind,
    pub lanes: ByteLanes,
}

impl BusAccess {
    fn new(address: u32, function_code: FunctionCode, kind: AccessKind, lanes: ByteLanes) -> Self {
        Self {
            logical_address: address,
            bus_address: address & BUS_ADDRESS_MASK & !1,
            function_code,
            kind,
            lanes,
        }
    }
}

/// Selected lanes use physical D15..D0 positions.
pub trait M68kBus {
    type Error;

    fn read(&mut self, access: BusAccess) -> Result<u16, Self::Error>;
    fn write(&mut self, access: BusAccess, value: u16) -> Result<(), Self::Error>;

    /// Pulses external reset; always deasserts before returning.
    fn reset_output(&mut self, _asserted_clocks: u32) -> Result<BusResponse<()>, Self::Error> {
        Ok(BusResponse::Unsupported)
    }

    /// Holds byte RMW ownership through completion or error.
    fn test_and_set(
        &mut self,
        _access: BusAccess,
    ) -> Result<BusResponse<u16>, RmwError<Self::Error>> {
        Ok(BusResponse::Unsupported)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BusFault<E> {
    UnsupportedOperation {
        operation: BusOperation,
    },
    ResetOutput {
        source: E,
    },
    AddressError {
        access: BusAccess,
        direction: Direction,
    },
    Transfer {
        access: BusAccess,
        direction: Direction,
        source: E,
    },
}

mod transfer;
pub use transfer::{
    read_byte, read_long, read_word, reset_output, test_and_set_byte, write_byte, write_long,
    write_word,
};
