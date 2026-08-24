use anyhow::bail;
use zeff_emu_common::save_state::{StateReader, StateWriter};

pub const ARCADE_CARD_RAM_LEN: usize = 2 * 1024 * 1024;
pub(crate) const MAX_ARCADE_CARD_STATE_SECTION_BYTES: usize = ARCADE_CARD_RAM_LEN + 64;

const RAM_ADDRESS_MASK: u32 = ARCADE_CARD_RAM_LEN as u32 - 1;
const BASE_ADDRESS_MASK: u32 = 0xFF_FFFF;
const BANK_WINDOW_START: u32 = 0x08_0000;
const BANK_WINDOW_END: u32 = 0x08_7FFF;
const IO_START: u32 = 0x1F_FA00;
const IO_END: u32 = 0x1F_FAFF;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PceArcadeCardMode {
    #[default]
    Automatic,
    Enabled,
    Disabled,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ArcadeCardPort {
    base: u32,
    offset: u16,
    increment: u16,
    control: u8,
}

impl ArcadeCardPort {
    fn effective_address(self) -> u32 {
        let offset = if self.control & 0x02 == 0 {
            0
        } else if self.control & 0x08 != 0 {
            u32::from(self.offset).wrapping_add(0xFFFF_0000)
        } else {
            u32::from(self.offset)
        };
        self.base.wrapping_add(offset) & RAM_ADDRESS_MASK
    }

    fn add_offset_to_base(&mut self) {
        let offset = if self.control & 0x08 != 0 {
            u32::from(self.offset).wrapping_add(0xFFFF_0000)
        } else {
            u32::from(self.offset)
        };
        self.base = self.base.wrapping_add(offset) & BASE_ADDRESS_MASK;
    }

    fn auto_increment(&mut self) {
        if self.control & 0x01 == 0 {
            return;
        }
        if self.control & 0x10 != 0 {
            self.base = self.base.wrapping_add(u32::from(self.increment)) & BASE_ADDRESS_MASK;
        } else {
            self.offset = self.offset.wrapping_add(self.increment);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArcadeCardPortDebugSnapshot {
    pub base: u32,
    pub offset: u16,
    pub increment: u16,
    pub control: u8,
    pub effective_address: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArcadeCardDebugSnapshot {
    pub ports: [ArcadeCardPortDebugSnapshot; 4],
    pub value: u32,
    pub shift: u8,
    pub rotate: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArcadeCard {
    ram: Box<[u8]>,
    ports: [ArcadeCardPort; 4],
    value: u32,
    shift: u8,
    rotate: u8,
}

impl Default for ArcadeCard {
    fn default() -> Self {
        Self::new()
    }
}

impl ArcadeCard {
    pub fn new() -> Self {
        Self {
            ram: vec![0; ARCADE_CARD_RAM_LEN].into_boxed_slice(),
            ports: [ArcadeCardPort::default(); 4],
            value: 0,
            shift: 0,
            rotate: 0,
        }
    }

    pub fn reset(&mut self) {
        self.ports = [ArcadeCardPort::default(); 4];
        self.value = 0;
        self.shift = 0;
        self.rotate = 0;
    }

    #[inline]
    pub fn ram(&self) -> &[u8] {
        &self.ram
    }

    #[inline]
    pub fn ram_mut(&mut self) -> &mut [u8] {
        &mut self.ram
    }

    pub fn debug_snapshot(&self) -> ArcadeCardDebugSnapshot {
        ArcadeCardDebugSnapshot {
            ports: self.ports.map(|port| ArcadeCardPortDebugSnapshot {
                base: port.base,
                offset: port.offset,
                increment: port.increment,
                control: port.control,
                effective_address: port.effective_address(),
            }),
            value: self.value,
            shift: self.shift,
            rotate: self.rotate,
        }
    }

    pub fn peek_physical(&self, physical_addr: u32) -> Option<u8> {
        match physical_addr {
            BANK_WINDOW_START..=BANK_WINDOW_END => {
                let port = ((physical_addr - BANK_WINDOW_START) >> 13) as usize;
                Some(self.peek_data(port))
            }
            IO_START..=IO_END => self.peek_io((physical_addr - IO_START) as u8),
            _ => None,
        }
    }

    pub fn read_physical(&mut self, physical_addr: u32) -> Option<u8> {
        match physical_addr {
            BANK_WINDOW_START..=BANK_WINDOW_END => {
                let port = ((physical_addr - BANK_WINDOW_START) >> 13) as usize;
                Some(self.read_data(port))
            }
            IO_START..=IO_END => Some(self.read_io((physical_addr - IO_START) as u8)),
            _ => None,
        }
    }

    pub fn write_physical(&mut self, physical_addr: u32, value: u8) -> bool {
        match physical_addr {
            BANK_WINDOW_START..=BANK_WINDOW_END => {
                let port = ((physical_addr - BANK_WINDOW_START) >> 13) as usize;
                self.write_data(port, value);
                true
            }
            IO_START..=IO_END => {
                self.write_io((physical_addr - IO_START) as u8, value);
                true
            }
            _ => false,
        }
    }

    fn peek_io(&self, offset: u8) -> Option<u8> {
        if offset < 0x80 {
            let port = usize::from((offset >> 4) & 3);
            Some(self.peek_port_register(port, offset & 0x0F))
        } else if offset >= 0xE0 {
            Some(self.peek_general_register(offset & 0x1F))
        } else {
            None
        }
    }

    fn read_io(&mut self, offset: u8) -> u8 {
        if offset < 0x80 {
            let port = usize::from((offset >> 4) & 3);
            self.read_port_register(port, offset & 0x0F)
        } else if offset >= 0xE0 {
            self.peek_general_register(offset & 0x1F)
        } else {
            0xFF
        }
    }

    fn write_io(&mut self, offset: u8, value: u8) {
        if offset < 0x80 {
            let port = usize::from((offset >> 4) & 3);
            self.write_port_register(port, offset & 0x0F, value);
        } else if offset >= 0xE0 {
            self.write_general_register(offset & 0x1F, value);
        }
    }

    fn peek_data(&self, port: usize) -> u8 {
        self.ram[self.ports[port].effective_address() as usize]
    }

    fn read_data(&mut self, port: usize) -> u8 {
        let value = self.peek_data(port);
        self.ports[port].auto_increment();
        value
    }

    fn write_data(&mut self, port: usize, value: u8) {
        let address = self.ports[port].effective_address() as usize;
        self.ram[address] = value;
        self.ports[port].auto_increment();
    }

    fn peek_port_register(&self, port: usize, register: u8) -> u8 {
        let registers = self.ports[port];
        match register {
            0 | 1 => self.peek_data(port),
            2 => registers.base as u8,
            3 => (registers.base >> 8) as u8,
            4 => (registers.base >> 16) as u8,
            5 => registers.offset as u8,
            6 => (registers.offset >> 8) as u8,
            7 => registers.increment as u8,
            8 => (registers.increment >> 8) as u8,
            9 => registers.control,
            0x0A => 0,
            _ => 0xFF,
        }
    }

    fn read_port_register(&mut self, port: usize, register: u8) -> u8 {
        match register {
            0 | 1 => self.read_data(port),
            _ => self.peek_port_register(port, register),
        }
    }

    fn write_port_register(&mut self, port: usize, register: u8, value: u8) {
        if matches!(register, 0 | 1) {
            self.write_data(port, value);
            return;
        }
        let registers = &mut self.ports[port];
        match register {
            2 => registers.base = (registers.base & 0xFF_FF00) | u32::from(value),
            3 => registers.base = (registers.base & 0xFF_00FF) | (u32::from(value) << 8),
            4 => registers.base = (registers.base & 0x00_FFFF) | (u32::from(value) << 16),
            5 => {
                registers.offset = (registers.offset & 0xFF00) | u16::from(value);
                if registers.control >> 5 == 1 {
                    registers.add_offset_to_base();
                }
            }
            6 => {
                registers.offset = (registers.offset & 0x00FF) | (u16::from(value) << 8);
                if registers.control >> 5 == 2 {
                    registers.add_offset_to_base();
                }
            }
            7 => registers.increment = (registers.increment & 0xFF00) | u16::from(value),
            8 => {
                registers.increment = (registers.increment & 0x00FF) | (u16::from(value) << 8);
            }
            9 => registers.control = value & 0x7F,
            0x0A if registers.control >> 5 == 3 => registers.add_offset_to_base(),
            _ => {}
        }
    }

    fn peek_general_register(&self, register: u8) -> u8 {
        match register {
            0..=3 => (self.value >> (u32::from(register) * 8)) as u8,
            4 => self.shift,
            5 => self.rotate,
            0x1C | 0x1D => 0,
            0x1E => 0x10,
            0x1F => 0x51,
            _ => 0xFF,
        }
    }

    fn write_general_register(&mut self, register: u8, value: u8) {
        match register {
            0..=3 => {
                let shift = u32::from(register) * 8;
                self.value = (self.value & !(0xFF << shift)) | (u32::from(value) << shift);
            }
            4 => {
                self.shift = value;
                let amount = value & 0x0F;
                if amount & 8 == 0 {
                    self.value = self.value.wrapping_shl(u32::from(amount));
                } else {
                    self.value >>= 16 - u32::from(amount);
                }
            }
            5 => {
                self.rotate = value;
                let amount = value & 0x0F;
                if amount & 8 == 0 {
                    self.value = self.value.rotate_left(u32::from(amount));
                } else {
                    self.value = self.value.rotate_right(16 - u32::from(amount));
                }
            }
            _ => {}
        }
    }

    pub(crate) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.ram);
        for port in self.ports {
            writer.write_u32(port.base);
            writer.write_u16(port.offset);
            writer.write_u16(port.increment);
            writer.write_u8(port.control);
        }
        writer.write_u32(self.value);
        writer.write_u8(self.shift);
        writer.write_u8(self.rotate);
    }

    pub(crate) fn read_state(&mut self, reader: &mut StateReader<'_>) -> anyhow::Result<()> {
        let mut ram = vec![0; ARCADE_CARD_RAM_LEN].into_boxed_slice();
        reader.read_exact(&mut ram)?;
        let mut ports = [ArcadeCardPort::default(); 4];
        for port in &mut ports {
            port.base = reader.read_u32()?;
            if port.base > BASE_ADDRESS_MASK {
                bail!("invalid Arcade Card base address: {:08X}", port.base);
            }
            port.offset = reader.read_u16()?;
            port.increment = reader.read_u16()?;
            port.control = reader.read_u8()?;
            if port.control > 0x7F {
                bail!("invalid Arcade Card control value: {:02X}", port.control);
            }
        }
        let value = reader.read_u32()?;
        let shift = reader.read_u8()?;
        let rotate = reader.read_u8()?;
        self.ram = ram;
        self.ports = ports;
        self.value = value;
        self.shift = shift;
        self.rotate = rotate;
        Ok(())
    }
}
