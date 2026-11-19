use super::*;
use zeff_m68k::bus::FunctionCode;

pub(super) fn serial_register(access: BusAccess) -> Option<usize> {
    if !(IO_SERIAL_START..IO_SERIAL_START + 6 * IO_REGISTER_STRIDE).contains(&access.bus_address) {
        return None;
    }
    let logical = access.logical_address & BUS_ADDRESS_MASK;
    let canonical = match access.lanes {
        ByteLanes::Lower => logical & 1 != 0 && logical & !1 == access.bus_address,
        ByteLanes::Both => logical == access.bus_address,
        ByteLanes::Upper => false,
    };
    (access.kind == AccessKind::Data
        && matches!(
            access.function_code,
            FunctionCode::UserData | FunctionCode::SupervisorData
        )
        && canonical
        && access.bus_address & 1 == 0)
        .then(|| ((access.bus_address - IO_SERIAL_START) / IO_REGISTER_STRIDE) as usize)
}

impl Bus {
    pub(super) fn read_timed(
        &mut self,
        access: BusAccess,
        begin: u64,
    ) -> Result<InstructionInput, BusError> {
        self.read_owned(access, serial_register(access).map(|_| begin), true)
    }

    pub(super) fn write_timed_capturing<S: previous::Capture>(
        &mut self,
        access: BusAccess,
        value: u16,
        begin: u64,
        previous: &mut S,
    ) -> Result<(), BusError> {
        self.write_owned_capturing(
            access,
            value,
            serial_register(access).map(|_| begin),
            true,
            previous,
        )
    }
    pub(super) fn read_input_cycle(
        &self,
        access: BusAccess,
        begin: u64,
    ) -> Option<Result<u16, BusError>> {
        let register = serial_register(access)?;
        Some(
            self.input
                .read_serial_at_range(register, begin, self.now.get(), self.region)
                .map(|byte| u16::from_be_bytes([byte, byte]))
                .ok_or(BusError::UnsupportedAccess {
                    address: access.bus_address,
                    direction: Direction::Read,
                }),
        )
    }

    pub(super) fn write_input_cycle(&mut self, access: BusAccess, value: u16, begin: u64) -> bool {
        if let Some(register) = serial_register(access) {
            self.input.write_serial_at_range(
                register,
                value as u8,
                begin,
                self.now.get(),
                self.region,
            )
        } else {
            self.write_input(access, value)
        }
    }
}
