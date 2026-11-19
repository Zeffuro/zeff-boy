use super::moves::address::{Stream, extension_address};
use super::*;
use crate::ea::Size;

impl PrefetchCpu {
    pub(super) fn read_operand<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        stream: &mut Stream,
        operand: (Size, u16, usize),
    ) -> Result<(u32, u32), TimingFault<B::Error>> {
        let (size, mode, register) = operand;
        if mode == 0 {
            return Ok((size.truncate(self.registers.d[register]), 0));
        }
        if mode == 7 && register == 4 {
            let high = stream.take(self, bus, false)?;
            let value = if size == Size::Long {
                (u32::from(high) << 16) | u32::from(stream.take(self, bus, false)?)
            } else {
                size.truncate(u32::from(high))
            };
            return Ok((value, 0));
        }
        let base = self.registers.address_register(register).unwrap();
        let increment = if size == Size::Byte && register == 7 {
            2
        } else {
            size.bytes()
        };
        let address = if mode >= 5 {
            extension_address(self, bus, stream, mode, register, false)?
        } else if mode == 4 {
            self.idle(bus, 2)?;
            let address = base.wrapping_sub(increment);
            self.registers.set_address_register(register, address);
            address
        } else {
            base
        };
        if mode == 3 && size != Size::Long {
            self.registers
                .set_address_register(register, base.wrapping_add(increment));
        }
        let fc = if mode == 7 && matches!(register, 2 | 3) {
            crate::ea::program_space(&self.registers)
        } else {
            crate::ea::data_space(&self.registers)
        };
        let high = self.read_data(bus, address, size == Size::Byte, fc)?;
        let value = if size == Size::Long {
            if mode == 3 {
                self.registers
                    .set_address_register(register, base.wrapping_add(4));
            }
            (u32::from(high) << 16)
                | u32::from(self.read_data(bus, address.wrapping_add(2), false, fc)?)
        } else {
            u32::from(high)
        };
        Ok((value, address))
    }
}
