use super::cartridge::Cartridge;
use super::constants::{ADDRESS_MASK, INTERNAL_RAM_SIZE, IO_PORT_COUNT};
use super::keypad::Keypad;
use super::ppu::{Ppu, PpuDebugSnapshot};

const KEYPAD_PORT: u16 = 0x00B5;
const ROM_LINEAR_BANK_PORT: u16 = 0x00C0;
const ROM_BANK0_PORT: u16 = 0x00C2;
const ROM_BANK1_PORT: u16 = 0x00C3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugTraceEvent {
    Read {
        addr: u32,
        value: u8,
    },
    Write {
        addr: u32,
        old_value: u8,
        new_value: u8,
    },
    IoRead {
        port: u16,
        value: u8,
    },
    IoWrite {
        port: u16,
        old_value: u8,
        new_value: u8,
    },
}

#[derive(Clone, Debug)]
pub struct Bus {
    pub cartridge: Cartridge,
    pub ppu: Ppu,
    pub keypad: Keypad,
    pub ram: Vec<u8>,
    pub io: Vec<u8>,
    pub cycles: u64,
    pub(crate) debug_trace_enabled: bool,
    pub(crate) debug_trace_events: Vec<DebugTraceEvent>,
}

impl Bus {
    pub fn new(cartridge: Cartridge) -> Self {
        Self {
            cartridge,
            ppu: Ppu::new(),
            keypad: Keypad::new(),
            ram: vec![0; INTERNAL_RAM_SIZE],
            io: vec![0; IO_PORT_COUNT],
            cycles: 0,
            debug_trace_enabled: false,
            debug_trace_events: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.ram.fill(0);
        self.io.fill(0);
        self.cycles = 0;
        self.cartridge.reset_banks();
        self.ppu.reset();
        self.keypad = Keypad::new();
    }

    pub fn read8(&mut self, addr: u32) -> u8 {
        let addr = addr & ADDRESS_MASK;
        let value = self.peek8(addr);
        self.record(DebugTraceEvent::Read { addr, value });
        value
    }

    pub fn peek8(&self, addr: u32) -> u8 {
        let addr = addr & ADDRESS_MASK;
        match addr {
            0x00000..=0x0FFFF => self.ram[(addr as usize) & (INTERNAL_RAM_SIZE - 1)],
            0x10000..=0xFFFFF => self.cartridge.rom_read8(addr),
            _ => 0xFF,
        }
    }

    pub fn read16(&mut self, addr: u32) -> u16 {
        u16::from_le_bytes([self.read8(addr), self.read8(addr.wrapping_add(1))])
    }

    pub fn peek16(&self, addr: u32) -> u16 {
        u16::from_le_bytes([self.peek8(addr), self.peek8(addr.wrapping_add(1))])
    }

    pub fn write8(&mut self, addr: u32, value: u8) {
        let addr = addr & ADDRESS_MASK;
        let old_value = self.peek8(addr);
        match addr {
            0x00000..=0x0FFFF => {
                self.ram[(addr as usize) & (INTERNAL_RAM_SIZE - 1)] = value;
            }
            0x10000..=0xFFFFF => self.cartridge.rom_write8(addr, value),
            _ => {}
        }
        let new_value = self.peek8(addr);
        self.record(DebugTraceEvent::Write {
            addr,
            old_value,
            new_value,
        });
    }

    pub fn write16(&mut self, addr: u32, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.write8(addr, lo);
        self.write8(addr.wrapping_add(1), hi);
    }

    pub fn io_read8(&mut self, port: u16) -> u8 {
        let value = match port {
            KEYPAD_PORT => self.keypad.read(),
            ROM_LINEAR_BANK_PORT => self.cartridge.linear_bank(),
            ROM_BANK0_PORT => self.cartridge.bank0() as u8,
            ROM_BANK1_PORT => self.cartridge.bank1() as u8,
            _ => self.io[usize::from(port)],
        };
        self.record(DebugTraceEvent::IoRead { port, value });
        value
    }

    pub fn io_read16(&mut self, port: u16) -> u16 {
        u16::from_le_bytes([self.io_read8(port), self.io_read8(port.wrapping_add(1))])
    }

    pub fn io_peek8(&self, port: u16) -> u8 {
        match port {
            KEYPAD_PORT => self.keypad.read(),
            ROM_LINEAR_BANK_PORT => self.cartridge.linear_bank(),
            ROM_BANK0_PORT => self.cartridge.bank0() as u8,
            ROM_BANK1_PORT => self.cartridge.bank1() as u8,
            _ => self.io[usize::from(port)],
        }
    }

    pub fn io_write8(&mut self, port: u16, value: u8) {
        let old_value = self.io_peek8(port);
        match port {
            KEYPAD_PORT => self.keypad.write(value),
            ROM_LINEAR_BANK_PORT => self.cartridge.set_linear_bank(value),
            ROM_BANK0_PORT => self.cartridge.set_bank0(value),
            ROM_BANK1_PORT => self.cartridge.set_bank1(value),
            _ => self.io[usize::from(port)] = value,
        }
        let new_value = self.io_peek8(port);
        self.record(DebugTraceEvent::IoWrite {
            port,
            old_value,
            new_value,
        });
    }

    pub fn io_write16(&mut self, port: u16, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.io_write8(port, lo);
        self.io_write8(port.wrapping_add(1), hi);
    }

    pub fn step_cycles(&mut self, cycles: u32) {
        self.cycles = self.cycles.wrapping_add(u64::from(cycles));
        self.ppu.step_cycles(cycles, &self.ram, &self.io);
    }

    pub fn render_frame(&mut self) {
        self.ppu.render_frame(&self.ram, &self.io);
        self.ppu.frame_ready = true;
    }

    pub fn ppu_debug_snapshot(&self) -> PpuDebugSnapshot {
        self.ppu.debug_snapshot()
    }

    pub(crate) fn take_debug_trace_events(&mut self) -> Vec<DebugTraceEvent> {
        std::mem::take(&mut self.debug_trace_events)
    }

    fn record(&mut self, event: DebugTraceEvent) {
        if self.debug_trace_enabled {
            self.debug_trace_events.push(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::cartridge::compute_footer_checksum;

    fn minimal_cart() -> Cartridge {
        let mut rom = vec![0xFF; 0x10000];
        let footer = rom.len() - 10;
        rom[footer + 4] = 0x01;
        let checksum = compute_footer_checksum(&rom);
        rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
        Cartridge::load(&rom).unwrap()
    }

    #[test]
    fn internal_ram_is_read_write() {
        let mut bus = Bus::new(minimal_cart());
        bus.write8(0x1234, 0x56);
        assert_eq!(bus.read8(0x1234), 0x56);
    }

    #[test]
    fn io_bank_ports_update_cartridge_banks() {
        let mut bus = Bus::new(minimal_cart());
        bus.io_write8(ROM_BANK0_PORT, 7);
        bus.io_write8(ROM_BANK1_PORT, 8);
        bus.io_write8(ROM_LINEAR_BANK_PORT, 2);
        assert_eq!(bus.io_read8(ROM_BANK0_PORT), 7);
        assert_eq!(bus.io_read8(ROM_BANK1_PORT), 8);
        assert_eq!(bus.io_read8(ROM_LINEAR_BANK_PORT), 2);
    }
}
