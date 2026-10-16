use super::*;

impl Bus {
    pub(super) fn read_input(&self, address: u32) -> Option<u8> {
        match address {
            IO_VERSION..=IO_VERSION_END => Some(self.region.io_version()),
            IO_DATA_START..=IO_DATA_END => Some(self.input.read_data(
                ((address - IO_DATA_START) / IO_REGISTER_STRIDE) as usize,
                self.now.get(),
                self.region,
            )),
            IO_CONTROL_START..=IO_CONTROL_END => Some(
                self.input
                    .read_control(((address - IO_CONTROL_START) / IO_REGISTER_STRIDE) as usize),
            ),
            IO_SERIAL_START..=IO_SERIAL_END => Some(
                self.input
                    .read_serial(((address - IO_SERIAL_START) / IO_REGISTER_STRIDE) as usize),
            ),
            _ => None,
        }
    }

    pub(super) fn write_input(&mut self, access: BusAccess, value: u16) -> bool {
        if access.lanes == ByteLanes::Upper && access.bus_address != IO_VERSION {
            return false;
        }
        let byte = value as u8;
        match access.bus_address {
            IO_VERSION => {}
            IO_DATA_START..=IO_DATA_END => {
                self.input.write_data(
                    ((access.bus_address - IO_DATA_START) / IO_REGISTER_STRIDE) as usize,
                    byte,
                    self.now.get(),
                    self.region,
                );
            }
            IO_CONTROL_START..=IO_CONTROL_END => {
                self.input.write_control(
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
                );
            }
            _ => return false,
        }
        true
    }
}
