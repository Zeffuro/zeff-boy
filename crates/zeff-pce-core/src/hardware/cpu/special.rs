use super::{Cpu, CpuBus, VdcPort, instructions::Register};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockTransferMode {
    IncrementIncrement,
    DecrementDecrement,
    IncrementFixed,
    IncrementAlternate,
    AlternateIncrement,
}

impl Cpu {
    pub(super) fn execute_special<B: CpuBus>(&mut self, bus: &mut B, opcode: u8) -> Option<u32> {
        let cycles = match opcode {
            0x02 => self.swap_registers(bus, Register::X, Register::Y),
            0x22 => self.swap_registers(bus, Register::Accumulator, Register::X),
            0x42 => self.swap_registers(bus, Register::Accumulator, Register::Y),
            0x62 => self.clear_register(bus, Register::Accumulator),
            0x82 => self.clear_register(bus, Register::X),
            0xC2 => self.clear_register(bus, Register::Y),
            0x03 => self.store_immediate_to_vdc(bus, VdcPort::SelectOrStatus),
            0x13 => self.store_immediate_to_vdc(bus, VdcPort::DataLow),
            0x23 => self.store_immediate_to_vdc(bus, VdcPort::DataHigh),
            0x73 => self.block_transfer(bus, BlockTransferMode::IncrementIncrement),
            0xC3 => self.block_transfer(bus, BlockTransferMode::DecrementDecrement),
            0xD3 => self.block_transfer(bus, BlockTransferMode::IncrementFixed),
            0xE3 => self.block_transfer(bus, BlockTransferMode::IncrementAlternate),
            0xF3 => self.block_transfer(bus, BlockTransferMode::AlternateIncrement),
            _ => return None,
        };
        Some(cycles)
    }

    fn swap_registers<B: CpuBus>(&mut self, bus: &mut B, first: Register, second: Register) -> u32 {
        let first_value = self.read_register(first);
        let second_value = self.read_register(second);
        self.write_register(first, second_value);
        self.write_register(second, first_value);
        self.dummy_fetch(bus);
        bus.idle();
        3
    }

    fn clear_register<B: CpuBus>(&mut self, bus: &mut B, register: Register) -> u32 {
        self.write_register(register, 0);
        self.dummy_fetch(bus);
        2
    }

    fn store_immediate_to_vdc<B: CpuBus>(&mut self, bus: &mut B, port: VdcPort) -> u32 {
        let value = self.fetch(bus);
        bus.idle();
        bus.write_vdc(port, value);
        4
    }

    fn block_transfer<B: CpuBus>(&mut self, bus: &mut B, mode: BlockTransferMode) -> u32 {
        self.dummy_fetch(bus);
        bus.idle();
        self.push(bus, self.registers.y);
        self.push(bus, self.registers.a);
        self.push(bus, self.registers.x);

        let mut source = self.fetch_word(bus);
        let mut destination = self.fetch_word(bus);
        let encoded_length = self.fetch_word(bus);
        let length = if encoded_length == 0 {
            65_536
        } else {
            u32::from(encoded_length)
        };
        bus.idle();
        bus.idle();

        let mut alternate = false;
        for _ in 0..length {
            let value = self.read(bus, source);
            bus.idle();
            self.write(bus, destination, value);
            bus.idle();
            bus.idle();
            bus.idle();

            match mode {
                BlockTransferMode::IncrementIncrement => {
                    source = source.wrapping_add(1);
                    destination = destination.wrapping_add(1);
                }
                BlockTransferMode::DecrementDecrement => {
                    source = source.wrapping_sub(1);
                    destination = destination.wrapping_sub(1);
                }
                BlockTransferMode::IncrementFixed => source = source.wrapping_add(1),
                BlockTransferMode::IncrementAlternate => {
                    source = source.wrapping_add(1);
                    destination = alternate_address(destination, alternate);
                    alternate = !alternate;
                }
                BlockTransferMode::AlternateIncrement => {
                    source = alternate_address(source, alternate);
                    destination = destination.wrapping_add(1);
                    alternate = !alternate;
                }
            }
        }

        self.registers.x = self.pull(bus);
        self.registers.a = self.pull(bus);
        self.registers.y = self.pull(bus);
        17 + 6 * length
    }
}

#[inline]
fn alternate_address(address: u16, reverse: bool) -> u16 {
    if reverse {
        address.wrapping_sub(1)
    } else {
        address.wrapping_add(1)
    }
}
