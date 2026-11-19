use super::*;
use zeff_m68k::bus::FunctionCode;

impl Bus {
    pub(super) fn unused_input_observation(access: BusAccess) -> bool {
        access.kind == AccessKind::Data
            && access.lanes == ByteLanes::Both
            && matches!(
                access.function_code,
                FunctionCode::UserData | FunctionCode::SupervisorData
            )
            && access.bus_address & 1 == 0
            && (IO_UNUSED_START..=IO_UNUSED_END).contains(&access.bus_address)
            && access.logical_address & BUS_ADDRESS_MASK == access.bus_address
    }

    pub(super) fn read_unused_input(&self, access: BusAccess) -> Option<Result<u16, BusError>> {
        Self::unused_input_observation(access).then(|| {
            self.prefetch.word().ok_or(BusError::UnsupportedAccess {
                address: access.bus_address,
                direction: Direction::Read,
            })
        })
    }

    pub(super) fn read_input(&self, address: u32) -> Option<u8> {
        if self.input.fault().is_some() {
            return None;
        }
        match address {
            IO_VERSION..=IO_VERSION_END => {
                Some(self.region.io_version() | u8::from(self.tmss.is_some()))
            }
            IO_DATA_START..=IO_DATA_END => {
                let port = ((address - IO_DATA_START) / IO_REGISTER_STRIDE) as usize;
                self.input
                    .data_read_allowed(port)
                    .then(|| self.input.read_data(port, self.now.get(), self.region))
            }
            IO_CONTROL_START..=IO_CONTROL_END => Some(
                self.input
                    .read_control(((address - IO_CONTROL_START) / IO_REGISTER_STRIDE) as usize),
            ),
            IO_SERIAL_START..=IO_SERIAL_END => {
                let register = ((address - IO_SERIAL_START) / IO_REGISTER_STRIDE) as usize;
                self.input.serial_read_allowed(register).then(|| {
                    self.input
                        .read_serial(register, self.now.get(), self.region)
                })
            }
            _ => None,
        }
    }

    pub(super) fn write_input(&mut self, access: BusAccess, value: u16) -> bool {
        if self.input.fault().is_some() {
            return false;
        }
        if access.lanes == ByteLanes::Upper {
            // The I/O write latches only receive the lower-byte strobe.
            return access.kind == AccessKind::Data
                && matches!(
                    access.function_code,
                    FunctionCode::UserData | FunctionCode::SupervisorData
                )
                && access.bus_address & 1 == 0
                && (IO_VERSION..=IO_SERIAL_END).contains(&access.bus_address)
                && access.logical_address & BUS_ADDRESS_MASK == access.bus_address;
        }
        let byte = value as u8;
        match access.bus_address {
            IO_VERSION => {}
            IO_DATA_START..=IO_DATA_END => {
                return self.input.write_data(
                    ((access.bus_address - IO_DATA_START) / IO_REGISTER_STRIDE) as usize,
                    byte,
                    self.now.get(),
                    self.region,
                );
            }
            IO_CONTROL_START..=IO_CONTROL_END => {
                return self.input.write_control(
                    ((access.bus_address - IO_CONTROL_START) / IO_REGISTER_STRIDE) as usize,
                    byte,
                    self.now.get(),
                    self.region,
                );
            }
            IO_SERIAL_START..=IO_SERIAL_END => {
                return self.input.write_serial(
                    ((access.bus_address - IO_SERIAL_START) / IO_REGISTER_STRIDE) as usize,
                    byte,
                    self.now.get(),
                    self.region,
                );
            }
            _ => return false,
        }
        true
    }
}
