use super::apu::{Apu, ApuDebugSnapshot};
use super::cartridge::Cartridge;
use super::constants::{ADDRESS_MASK, IO_PORT_COUNT, WS_INTERNAL_RAM_SIZE, WSC_INTERNAL_RAM_SIZE};
use super::keypad::Keypad;
use super::ppu::{Ppu, PpuDebugSnapshot};

const KEYPAD_PORT: u16 = 0x00B5;
const IRQ_VECTOR_BASE_PORT: u16 = 0x00B0;
const IRQ_ENABLE_PORT: u16 = 0x00B2;
const IRQ_STATUS_PORT: u16 = 0x00B4;
const IRQ_ACK_PORT: u16 = 0x00B6;
const SERIAL_DATA_PORT: u16 = 0x00B1;
const SERIAL_CONTROL_PORT: u16 = 0x00B3;
const CURRENT_LINE_PORT: u16 = 0x0002;
const LINE_COMPARE_PORT: u16 = 0x0003;
const TIMER_CONTROL_PORT: u16 = 0x00A2;
const HBLANK_TIMER_RELOAD_LO_PORT: u16 = 0x00A4;
const HBLANK_TIMER_RELOAD_HI_PORT: u16 = 0x00A5;
const VBLANK_TIMER_RELOAD_LO_PORT: u16 = 0x00A6;
const VBLANK_TIMER_RELOAD_HI_PORT: u16 = 0x00A7;
const HBLANK_TIMER_COUNT_LO_PORT: u16 = 0x00A8;
const HBLANK_TIMER_COUNT_HI_PORT: u16 = 0x00A9;
const VBLANK_TIMER_COUNT_LO_PORT: u16 = 0x00AA;
const VBLANK_TIMER_COUNT_HI_PORT: u16 = 0x00AB;
const SYSTEM_CONTROL_PORT: u16 = 0x00A0;
const LCD_CONTROL_PORT: u16 = 0x0014;
const LCD_VTOTAL_PORT: u16 = 0x0016;
const DMA_SOURCE_LO_PORT: u16 = 0x0040;
const DMA_SOURCE_HI_PORT: u16 = 0x0041;
const DMA_SOURCE_SEGMENT_PORT: u16 = 0x0042;
const DMA_DESTINATION_LO_PORT: u16 = 0x0044;
const DMA_DESTINATION_HI_PORT: u16 = 0x0045;
const DMA_LENGTH_LO_PORT: u16 = 0x0046;
const DMA_LENGTH_HI_PORT: u16 = 0x0047;
const DMA_CONTROL_PORT: u16 = 0x0048;
const INTERNAL_EEPROM_DATA_LO_PORT: u16 = 0x00BA;
const INTERNAL_EEPROM_DATA_HI_PORT: u16 = 0x00BB;
const INTERNAL_EEPROM_ADDR_LO_PORT: u16 = 0x00BC;
const INTERNAL_EEPROM_ADDR_HI_PORT: u16 = 0x00BD;
const INTERNAL_EEPROM_COMMAND_PORT: u16 = 0x00BE;
const ROM_LINEAR_BANK_PORT: u16 = 0x00C0;
const ROM_RAM_BANK_PORT: u16 = 0x00C1;
const ROM_BANK0_PORT: u16 = 0x00C2;
const ROM_BANK1_PORT: u16 = 0x00C3;
const CART_EEPROM_DATA_LO_PORT: u16 = 0x00C4;
const CART_EEPROM_DATA_HI_PORT: u16 = 0x00C5;
const CART_EEPROM_COMMAND_LO_PORT: u16 = 0x00C6;
const CART_EEPROM_COMMAND_HI_PORT: u16 = 0x00C7;
const CART_EEPROM_CONTROL_STATUS_LO_PORT: u16 = 0x00C8;
const CART_EEPROM_CONTROL_STATUS_HI_PORT: u16 = 0x00C9;
const RTC_COMMAND_STATUS_PORT: u16 = 0x00CA;
const RTC_PAYLOAD_PORT: u16 = 0x00CB;
const WS_INTERNAL_EEPROM_SIZE: usize = 0x80;
const WSC_INTERNAL_EEPROM_SIZE: usize = 0x800;
const EEPROM_STATUS_READ_DONE: u8 = 0x01;
const EEPROM_STATUS_READY: u8 = 0x02;
const RTC_READY: u8 = 0x80;
const RTC_COMMAND_MASK: u8 = 0x1F;
const RTC_WRITE_DATETIME_COMMAND: u8 = 0x14;
const RTC_READ_DATETIME_COMMAND: u8 = 0x15;
const IRQ_KEYPAD: u8 = 0x02;
const IRQ_LINE_COMPARE: u8 = 0x10;
const IRQ_VBLANK_TIMER: u8 = 0x20;
const IRQ_VBLANK: u8 = 0x40;
const IRQ_HBLANK_TIMER: u8 = 0x80;
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
    pub apu: Apu,
    pub keypad: Keypad,
    pub ram: Vec<u8>,
    pub io: Vec<u8>,
    pub internal_eeprom: Vec<u8>,
    rtc: Rtc,
    pending_linear_bank: Option<DeferredLinearBank>,
    pub cycles: u64,
    pub(crate) debug_trace_enabled: bool,
    pub(crate) debug_trace_events: Vec<DebugTraceEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DeferredLinearBank {
    value: u8,
    remaining_instruction_retires: u8,
}

impl Bus {
    pub fn new(cartridge: Cartridge) -> Self {
        let ram_size = internal_ram_size_for_cartridge(&cartridge);
        let internal_eeprom = internal_eeprom_for_cartridge(&cartridge);
        Self {
            cartridge,
            ppu: Ppu::new(),
            apu: Apu::new(48_000),
            keypad: Keypad::new(),
            ram: vec![0; ram_size],
            io: vec![0; IO_PORT_COUNT],
            internal_eeprom,
            rtc: Rtc::new(),
            pending_linear_bank: None,
            cycles: 0,
            debug_trace_enabled: false,
            debug_trace_events: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.ram.fill(0);
        self.io.fill(0);
        self.internal_eeprom = internal_eeprom_for_cartridge(&self.cartridge);
        self.rtc.reset();
        self.pending_linear_bank = None;
        self.cycles = 0;
        self.cartridge.reset_banks();
        self.ppu.reset();
        self.apu.reset();
        self.keypad = Keypad::new();
        self.apply_cartridge_start_state();
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
            0x00000..=0x0FFFF => internal_ram_read(&self.ram, addr),
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
            0x00000..=0x0FFFF => internal_ram_write(&mut self.ram, addr, value),
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
            CURRENT_LINE_PORT => self.ppu.vcount() as u8,
            port if Apu::handles_port(port) => self.apu.read8(port),
            LINE_COMPARE_PORT => self.io[usize::from(LINE_COMPARE_PORT)],
            HBLANK_TIMER_COUNT_LO_PORT | HBLANK_TIMER_COUNT_HI_PORT => {
                let count = self.hblank_timer_count();
                count.to_le_bytes()[usize::from(port - HBLANK_TIMER_COUNT_LO_PORT)]
            }
            VBLANK_TIMER_COUNT_LO_PORT | VBLANK_TIMER_COUNT_HI_PORT => {
                let count = self.vblank_timer_count();
                count.to_le_bytes()[usize::from(port - VBLANK_TIMER_COUNT_LO_PORT)]
            }
            SYSTEM_CONTROL_PORT => self.system_control_read(),
            INTERNAL_EEPROM_DATA_LO_PORT
            | INTERNAL_EEPROM_DATA_HI_PORT
            | INTERNAL_EEPROM_ADDR_LO_PORT
            | INTERNAL_EEPROM_ADDR_HI_PORT
            | INTERNAL_EEPROM_COMMAND_PORT => self.io[usize::from(port)],
            IRQ_VECTOR_BASE_PORT => self.interrupt_base_read(),
            IRQ_ENABLE_PORT => self.io[usize::from(IRQ_ENABLE_PORT)],
            SERIAL_DATA_PORT => self.io[usize::from(SERIAL_DATA_PORT)],
            SERIAL_CONTROL_PORT => self.serial_control_read(),
            KEYPAD_PORT => self.keypad.read(),
            IRQ_STATUS_PORT => self.io[usize::from(IRQ_STATUS_PORT)],
            IRQ_ACK_PORT => self.io_open_bus(),
            ROM_LINEAR_BANK_PORT => self.linear_bank_read() | 0x20,
            ROM_RAM_BANK_PORT => self.cartridge.ram_bank(),
            ROM_BANK0_PORT => self.cartridge.bank0() as u8,
            ROM_BANK1_PORT => self.cartridge.bank1() as u8,
            CART_EEPROM_DATA_LO_PORT
            | CART_EEPROM_DATA_HI_PORT
            | CART_EEPROM_COMMAND_LO_PORT
            | CART_EEPROM_COMMAND_HI_PORT => self.io[usize::from(port)],
            CART_EEPROM_CONTROL_STATUS_LO_PORT => self.cartridge_eeprom_status_read(),
            CART_EEPROM_CONTROL_STATUS_HI_PORT => 0,
            RTC_COMMAND_STATUS_PORT => self.rtc.read_status(),
            RTC_PAYLOAD_PORT => self.rtc.read_payload(),
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
            CURRENT_LINE_PORT => self.ppu.vcount() as u8,
            port if Apu::handles_port(port) => self.apu.read8(port),
            LINE_COMPARE_PORT => self.io[usize::from(LINE_COMPARE_PORT)],
            HBLANK_TIMER_COUNT_LO_PORT | HBLANK_TIMER_COUNT_HI_PORT => {
                let count = self.hblank_timer_count();
                count.to_le_bytes()[usize::from(port - HBLANK_TIMER_COUNT_LO_PORT)]
            }
            VBLANK_TIMER_COUNT_LO_PORT | VBLANK_TIMER_COUNT_HI_PORT => {
                let count = self.vblank_timer_count();
                count.to_le_bytes()[usize::from(port - VBLANK_TIMER_COUNT_LO_PORT)]
            }
            SYSTEM_CONTROL_PORT => self.system_control_read(),
            INTERNAL_EEPROM_DATA_LO_PORT
            | INTERNAL_EEPROM_DATA_HI_PORT
            | INTERNAL_EEPROM_ADDR_LO_PORT
            | INTERNAL_EEPROM_ADDR_HI_PORT
            | INTERNAL_EEPROM_COMMAND_PORT => self.io[usize::from(port)],
            IRQ_VECTOR_BASE_PORT => self.interrupt_base_read(),
            IRQ_ENABLE_PORT => self.io[usize::from(IRQ_ENABLE_PORT)],
            SERIAL_DATA_PORT => self.io[usize::from(SERIAL_DATA_PORT)],
            SERIAL_CONTROL_PORT => self.serial_control_read(),
            KEYPAD_PORT => self.keypad.read(),
            IRQ_STATUS_PORT => self.io[usize::from(IRQ_STATUS_PORT)],
            IRQ_ACK_PORT => self.io_open_bus(),
            ROM_LINEAR_BANK_PORT => self.linear_bank_read() | 0x20,
            ROM_RAM_BANK_PORT => self.cartridge.ram_bank(),
            ROM_BANK0_PORT => self.cartridge.bank0() as u8,
            ROM_BANK1_PORT => self.cartridge.bank1() as u8,
            CART_EEPROM_DATA_LO_PORT
            | CART_EEPROM_DATA_HI_PORT
            | CART_EEPROM_COMMAND_LO_PORT
            | CART_EEPROM_COMMAND_HI_PORT => self.io[usize::from(port)],
            CART_EEPROM_CONTROL_STATUS_LO_PORT => self.cartridge_eeprom_status_read(),
            CART_EEPROM_CONTROL_STATUS_HI_PORT => 0,
            RTC_COMMAND_STATUS_PORT => self.rtc.peek_status(),
            RTC_PAYLOAD_PORT => self.rtc.peek_payload(),
            _ => self.io[usize::from(port)],
        }
    }

    pub fn io_write8(&mut self, port: u16, value: u8) {
        let old_value = self.io_peek8(port);
        match port {
            CURRENT_LINE_PORT => {}
            port if Apu::handles_port(port) => self.apu.write8(port, value),
            LINE_COMPARE_PORT => self.io[usize::from(LINE_COMPARE_PORT)] = value,
            TIMER_CONTROL_PORT => self.io[usize::from(TIMER_CONTROL_PORT)] = value & 0x0F,
            HBLANK_TIMER_RELOAD_LO_PORT | HBLANK_TIMER_RELOAD_HI_PORT => {
                self.io[usize::from(port)] = value;
                self.set_hblank_timer_count(self.hblank_timer_reload());
            }
            VBLANK_TIMER_RELOAD_LO_PORT | VBLANK_TIMER_RELOAD_HI_PORT => {
                self.io[usize::from(port)] = value;
                self.set_vblank_timer_count(self.vblank_timer_reload());
            }
            HBLANK_TIMER_COUNT_LO_PORT | HBLANK_TIMER_COUNT_HI_PORT => {
                self.io[usize::from(port)] = value;
            }
            VBLANK_TIMER_COUNT_LO_PORT | VBLANK_TIMER_COUNT_HI_PORT => {
                self.io[usize::from(port)] = value;
            }
            DMA_SOURCE_LO_PORT
            | DMA_SOURCE_HI_PORT
            | DMA_SOURCE_SEGMENT_PORT
            | DMA_DESTINATION_LO_PORT
            | DMA_DESTINATION_HI_PORT
            | DMA_LENGTH_LO_PORT
            | DMA_LENGTH_HI_PORT => {
                self.io[usize::from(port)] = value;
                if port == DMA_SOURCE_SEGMENT_PORT {
                    self.io[usize::from(port)] &= 0x0F;
                }
            }
            DMA_CONTROL_PORT => {
                self.io[usize::from(DMA_CONTROL_PORT)] = value;
                if value & 0x80 != 0 {
                    self.run_dma_transfer(value);
                }
            }
            SYSTEM_CONTROL_PORT => {
                let color_bit = u8::from(
                    self.cartridge.minimum_system() != super::cartridge::MinimumSystem::WonderSwan,
                ) << 1;
                self.io[usize::from(SYSTEM_CONTROL_PORT)] = (value & 0xFD) | color_bit;
            }
            INTERNAL_EEPROM_DATA_LO_PORT
            | INTERNAL_EEPROM_DATA_HI_PORT
            | INTERNAL_EEPROM_ADDR_LO_PORT
            | INTERNAL_EEPROM_ADDR_HI_PORT => {
                self.io[usize::from(port)] = value;
            }
            INTERNAL_EEPROM_COMMAND_PORT => self.write_internal_eeprom_command(value),
            IRQ_VECTOR_BASE_PORT => self.io[usize::from(IRQ_VECTOR_BASE_PORT)] = value & 0xF8,
            IRQ_ENABLE_PORT => {
                self.io[usize::from(IRQ_ENABLE_PORT)] = value;
            }
            SERIAL_DATA_PORT => {
                self.io[usize::from(SERIAL_DATA_PORT)] = value;
            }
            SERIAL_CONTROL_PORT => {
                self.io[usize::from(SERIAL_CONTROL_PORT)] = value & 0xC0;
            }
            KEYPAD_PORT => self.keypad.write(value),
            IRQ_ACK_PORT => self.io[usize::from(IRQ_STATUS_PORT)] &= !value,
            ROM_LINEAR_BANK_PORT => self.defer_linear_bank(value),
            ROM_RAM_BANK_PORT => self.cartridge.set_ram_bank(value),
            ROM_BANK0_PORT => self.cartridge.set_bank0(value),
            ROM_BANK1_PORT => self.cartridge.set_bank1(value),
            CART_EEPROM_DATA_LO_PORT
            | CART_EEPROM_DATA_HI_PORT
            | CART_EEPROM_COMMAND_LO_PORT
            | CART_EEPROM_COMMAND_HI_PORT => self.io[usize::from(port)] = value,
            CART_EEPROM_CONTROL_STATUS_LO_PORT => self.write_cartridge_eeprom_control(value),
            CART_EEPROM_CONTROL_STATUS_HI_PORT => {}
            RTC_COMMAND_STATUS_PORT => {
                self.rtc.write_command(value);
                self.io[usize::from(RTC_COMMAND_STATUS_PORT)] = value;
            }
            RTC_PAYLOAD_PORT => {
                self.rtc.write_payload(value);
                self.io[usize::from(RTC_PAYLOAD_PORT)] = value;
            }
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
        self.apu.step_cycles(cycles, &self.ram);
        let ppu_events = self.ppu.step_cycles(cycles, &self.ram, &self.io);
        self.step_hblank_timer(ppu_events.completed_scanlines);
        if ppu_events.vblank_started {
            self.raise_interrupt(IRQ_VBLANK);
            self.step_vblank_timer();
        }
        if ppu_events.line_compare {
            self.raise_interrupt(IRQ_LINE_COMPARE);
        }
    }

    pub fn render_frame(&mut self) {
        self.ppu.render_frame(&self.ram, &self.io);
        self.ppu.frame_ready = true;
    }

    pub(crate) fn retire_instruction(&mut self) {
        let Some(mut pending) = self.pending_linear_bank else {
            return;
        };
        if pending.remaining_instruction_retires > 1 {
            pending.remaining_instruction_retires -= 1;
            self.pending_linear_bank = Some(pending);
            return;
        }
        self.cartridge.set_linear_bank(pending.value);
        self.pending_linear_bank = None;
    }

    pub fn ppu_debug_snapshot(&self) -> PpuDebugSnapshot {
        self.ppu.debug_snapshot()
    }

    pub fn apu_debug_snapshot(&self) -> ApuDebugSnapshot {
        self.apu.debug_snapshot()
    }

    pub(crate) fn take_debug_trace_events(&mut self) -> Vec<DebugTraceEvent> {
        std::mem::take(&mut self.debug_trace_events)
    }

    pub(crate) fn pending_interrupt_vector(&self) -> Option<u8> {
        highest_interrupt_id(self.io[usize::from(IRQ_STATUS_PORT)])
            .map(|id| (self.io[usize::from(IRQ_VECTOR_BASE_PORT)] & 0xF8) | id)
    }

    pub(crate) fn has_pending_interrupt_signal(&self) -> bool {
        self.io[usize::from(IRQ_STATUS_PORT)] != 0
    }

    fn interrupt_base_read(&self) -> u8 {
        (self.io[usize::from(IRQ_VECTOR_BASE_PORT)] & 0xF8)
            | highest_interrupt_id(self.io[usize::from(IRQ_STATUS_PORT)]).unwrap_or(0)
    }

    fn io_open_bus(&self) -> u8 {
        if self.cartridge.minimum_system() == super::cartridge::MinimumSystem::WonderSwan {
            0x90
        } else {
            0x00
        }
    }

    pub(crate) fn raise_keypad_interrupt(&mut self) {
        self.raise_interrupt(IRQ_KEYPAD);
    }

    fn apply_cartridge_start_state(&mut self) {
        let color = self.cartridge.minimum_system() != super::cartridge::MinimumSystem::WonderSwan;
        if color && self.ram.len() > 0xFE00 {
            self.ram[0xFE00..].fill(0xFF);
        }

        self.io[usize::from(SYSTEM_CONTROL_PORT)] = if color { 0x87 } else { 0x85 };
        self.io[usize::from(LINE_COMPARE_PORT)] = 0x00;
        self.io[usize::from(LCD_CONTROL_PORT)] = 0x01;
        self.io[usize::from(LCD_VTOTAL_PORT)] = 0x9E;
        self.io[usize::from(IRQ_VECTOR_BASE_PORT)] = 0x00;
        self.io[usize::from(IRQ_ENABLE_PORT)] = 0x00;
        self.io[usize::from(IRQ_STATUS_PORT)] = 0x00;
        self.io[usize::from(IRQ_ACK_PORT)] = if color { 0xFF } else { 0x00 };
        self.io[0x0060] = 0x0A;
        if color {
            self.io[0x009E] = 0x03;
        }
        self.io[usize::from(INTERNAL_EEPROM_DATA_LO_PORT)] = 0x00;
        self.io[usize::from(INTERNAL_EEPROM_DATA_HI_PORT)] = 0x00;
        self.io[usize::from(INTERNAL_EEPROM_COMMAND_PORT)] = 0x80;
        self.keypad.write(0x40);
    }

    fn record(&mut self, event: DebugTraceEvent) {
        if self.debug_trace_enabled {
            self.debug_trace_events.push(event);
        }
    }

    fn raise_interrupt(&mut self, mask: u8) {
        if self.io[usize::from(IRQ_ENABLE_PORT)] & mask != 0 {
            self.io[usize::from(IRQ_STATUS_PORT)] |= mask;
        }
    }

    fn hblank_timer_reload(&self) -> u16 {
        u16::from_le_bytes([
            self.io[usize::from(HBLANK_TIMER_RELOAD_LO_PORT)],
            self.io[usize::from(HBLANK_TIMER_RELOAD_HI_PORT)],
        ])
    }

    fn set_hblank_timer_reload(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.io[usize::from(HBLANK_TIMER_RELOAD_LO_PORT)] = lo;
        self.io[usize::from(HBLANK_TIMER_RELOAD_HI_PORT)] = hi;
    }

    fn hblank_timer_count(&self) -> u16 {
        u16::from_le_bytes([
            self.io[usize::from(HBLANK_TIMER_COUNT_LO_PORT)],
            self.io[usize::from(HBLANK_TIMER_COUNT_HI_PORT)],
        ])
    }

    fn set_hblank_timer_count(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.io[usize::from(HBLANK_TIMER_COUNT_LO_PORT)] = lo;
        self.io[usize::from(HBLANK_TIMER_COUNT_HI_PORT)] = hi;
    }

    fn vblank_timer_reload(&self) -> u16 {
        u16::from_le_bytes([
            self.io[usize::from(VBLANK_TIMER_RELOAD_LO_PORT)],
            self.io[usize::from(VBLANK_TIMER_RELOAD_HI_PORT)],
        ])
    }

    fn set_vblank_timer_reload(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.io[usize::from(VBLANK_TIMER_RELOAD_LO_PORT)] = lo;
        self.io[usize::from(VBLANK_TIMER_RELOAD_HI_PORT)] = hi;
    }

    fn vblank_timer_count(&self) -> u16 {
        u16::from_le_bytes([
            self.io[usize::from(VBLANK_TIMER_COUNT_LO_PORT)],
            self.io[usize::from(VBLANK_TIMER_COUNT_HI_PORT)],
        ])
    }

    fn set_vblank_timer_count(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.io[usize::from(VBLANK_TIMER_COUNT_LO_PORT)] = lo;
        self.io[usize::from(VBLANK_TIMER_COUNT_HI_PORT)] = hi;
    }

    fn step_hblank_timer(&mut self, completed_scanlines: u32) {
        let control = self.io[usize::from(TIMER_CONTROL_PORT)];
        let enabled = control & 0x01 != 0;
        let repeat = control & 0x02 != 0;
        for _ in 0..completed_scanlines {
            let count = self.hblank_timer_count();
            let next_count = count.wrapping_sub(1);
            if enabled && count != 0 {
                self.set_hblank_timer_count(next_count);
                if next_count == 0 {
                    if repeat {
                        self.set_hblank_timer_count(self.hblank_timer_reload());
                    } else {
                        self.set_hblank_timer_reload(0);
                    }
                }
            }
            if next_count == 0 {
                self.raise_interrupt(IRQ_HBLANK_TIMER);
            }
        }
    }

    fn step_vblank_timer(&mut self) {
        let control = self.io[usize::from(TIMER_CONTROL_PORT)];
        let enabled = control & 0x04 != 0;
        let count = self.vblank_timer_count();
        let next_count = count.wrapping_sub(1);
        if enabled && count != 0 {
            self.set_vblank_timer_count(next_count);
            if next_count == 0 {
                if control & 0x08 != 0 {
                    self.set_vblank_timer_count(self.vblank_timer_reload());
                } else {
                    self.set_vblank_timer_reload(0);
                }
            }
        }
        if next_count == 0 {
            self.raise_interrupt(IRQ_VBLANK_TIMER);
        }
    }

    fn system_control_read(&self) -> u8 {
        let color_bit = u8::from(
            self.cartridge.minimum_system() != super::cartridge::MinimumSystem::WonderSwan,
        ) << 1;
        (self.io[usize::from(SYSTEM_CONTROL_PORT)] & 0x7D) | color_bit | 0x80
    }

    fn serial_control_read(&self) -> u8 {
        (self.io[usize::from(SERIAL_CONTROL_PORT)] & 0xC0) | 0x04
    }

    fn linear_bank_read(&self) -> u8 {
        self.pending_linear_bank
            .map(|pending| pending.value)
            .unwrap_or_else(|| self.cartridge.linear_bank())
    }

    fn defer_linear_bank(&mut self, value: u8) {
        self.pending_linear_bank = Some(DeferredLinearBank {
            value: value & 0x0F,
            remaining_instruction_retires: 2,
        });
    }

    fn internal_eeprom_data(&self) -> u16 {
        u16::from_le_bytes([
            self.io[usize::from(INTERNAL_EEPROM_DATA_LO_PORT)],
            self.io[usize::from(INTERNAL_EEPROM_DATA_HI_PORT)],
        ])
    }

    fn set_internal_eeprom_data(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.io[usize::from(INTERNAL_EEPROM_DATA_LO_PORT)] = lo;
        self.io[usize::from(INTERNAL_EEPROM_DATA_HI_PORT)] = hi;
    }

    fn internal_eeprom_address(&self) -> usize {
        let raw = u16::from_le_bytes([
            self.io[usize::from(INTERNAL_EEPROM_ADDR_LO_PORT)],
            self.io[usize::from(INTERNAL_EEPROM_ADDR_HI_PORT)],
        ]);
        let word_index = match self.cartridge.minimum_system() {
            super::cartridge::MinimumSystem::WonderSwan => raw & 0x003F,
            super::cartridge::MinimumSystem::WonderSwanColor
            | super::cartridge::MinimumSystem::Unknown(_) => raw & 0x01FF,
        };
        usize::from(word_index) * 2
    }

    fn write_internal_eeprom_command(&mut self, value: u8) {
        let mut command = value & 0xFC;
        if command & 0x20 != 0 {
            let address = self.internal_eeprom_address();
            let data = self.internal_eeprom_data().to_le_bytes();
            if address + 1 < self.internal_eeprom.len() {
                self.internal_eeprom[address] = data[0];
                self.internal_eeprom[address + 1] = data[1];
            }
            command |= 0x02;
        } else if command & 0x10 != 0 {
            let address = self.internal_eeprom_address();
            let lo = self.internal_eeprom.get(address).copied().unwrap_or(0);
            let hi = self.internal_eeprom.get(address + 1).copied().unwrap_or(0);
            self.set_internal_eeprom_data(u16::from_le_bytes([lo, hi]));
            command |= 0x01;
        }
        self.io[usize::from(INTERNAL_EEPROM_COMMAND_PORT)] = command;
    }

    fn cartridge_eeprom_data(&self) -> u16 {
        u16::from_le_bytes([
            self.io[usize::from(CART_EEPROM_DATA_LO_PORT)],
            self.io[usize::from(CART_EEPROM_DATA_HI_PORT)],
        ])
    }

    fn set_cartridge_eeprom_data(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.io[usize::from(CART_EEPROM_DATA_LO_PORT)] = lo;
        self.io[usize::from(CART_EEPROM_DATA_HI_PORT)] = hi;
    }

    fn cartridge_eeprom_command(&self) -> u16 {
        u16::from_le_bytes([
            self.io[usize::from(CART_EEPROM_COMMAND_LO_PORT)],
            self.io[usize::from(CART_EEPROM_COMMAND_HI_PORT)],
        ])
    }

    fn cartridge_eeprom_status_read(&self) -> u8 {
        if !self.cartridge.save_kind().is_eeprom() {
            return self.io[usize::from(CART_EEPROM_CONTROL_STATUS_LO_PORT)];
        }
        (self.io[usize::from(CART_EEPROM_CONTROL_STATUS_LO_PORT)] & EEPROM_STATUS_READ_DONE)
            | EEPROM_STATUS_READY
    }

    fn set_cartridge_eeprom_read_done(&mut self, done: bool) {
        let status = &mut self.io[usize::from(CART_EEPROM_CONTROL_STATUS_LO_PORT)];
        if done {
            *status |= EEPROM_STATUS_READ_DONE;
        } else {
            *status &= !EEPROM_STATUS_READ_DONE;
        }
        *status |= EEPROM_STATUS_READY;
    }

    fn write_cartridge_eeprom_control(&mut self, value: u8) {
        if !self.cartridge.save_kind().is_eeprom() {
            self.io[usize::from(CART_EEPROM_CONTROL_STATUS_LO_PORT)] = value;
            return;
        }

        let operation = value & 0x07;
        let invalid = value & 0x80 != 0 || !matches!(operation, 0x01 | 0x02 | 0x04);
        if invalid {
            self.io[usize::from(CART_EEPROM_CONTROL_STATUS_LO_PORT)] = EEPROM_STATUS_READY;
            return;
        }

        let command =
            decode_eeprom_command(self.cartridge.save_kind(), self.cartridge_eeprom_command());
        match (operation, command) {
            (0x01, EepromCommand::Read { address }) => {
                let value = self.cartridge.eeprom_read_word(address);
                self.set_cartridge_eeprom_data(value);
                self.set_cartridge_eeprom_read_done(true);
            }
            (0x02, EepromCommand::Write { address }) => {
                self.cartridge
                    .eeprom_write_word(address, self.cartridge_eeprom_data());
                self.set_cartridge_eeprom_read_done(false);
            }
            (0x02, EepromCommand::WriteAll) => {
                self.cartridge
                    .eeprom_fill_words(self.cartridge_eeprom_data());
                self.set_cartridge_eeprom_read_done(false);
            }
            (0x04, EepromCommand::Erase { address }) => {
                self.cartridge.eeprom_write_word(address, 0xFFFF);
                self.set_cartridge_eeprom_read_done(false);
            }
            (0x04, EepromCommand::EraseAll) => {
                self.cartridge.eeprom_fill_words(0xFFFF);
                self.set_cartridge_eeprom_read_done(false);
            }
            (0x04, EepromCommand::WriteDisable | EepromCommand::WriteEnable) => {
                self.set_cartridge_eeprom_read_done(false);
            }
            _ => {
                self.io[usize::from(CART_EEPROM_CONTROL_STATUS_LO_PORT)] = EEPROM_STATUS_READY;
            }
        }
    }

    fn dma_source_offset(&self) -> u16 {
        u16::from_le_bytes([
            self.io[usize::from(DMA_SOURCE_LO_PORT)],
            self.io[usize::from(DMA_SOURCE_HI_PORT)],
        ])
    }

    fn set_dma_source_offset(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.io[usize::from(DMA_SOURCE_LO_PORT)] = lo;
        self.io[usize::from(DMA_SOURCE_HI_PORT)] = hi;
    }

    fn dma_source_segment(&self) -> u16 {
        u16::from(self.io[usize::from(DMA_SOURCE_SEGMENT_PORT)] & 0x0F)
    }

    fn set_dma_source_segment(&mut self, value: u16) {
        self.io[usize::from(DMA_SOURCE_SEGMENT_PORT)] = (value & 0x0F) as u8;
        self.io[usize::from(DMA_SOURCE_SEGMENT_PORT + 1)] = 0;
    }

    fn dma_destination(&self) -> u16 {
        u16::from_le_bytes([
            self.io[usize::from(DMA_DESTINATION_LO_PORT)],
            self.io[usize::from(DMA_DESTINATION_HI_PORT)],
        ])
    }

    fn set_dma_destination(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.io[usize::from(DMA_DESTINATION_LO_PORT)] = lo;
        self.io[usize::from(DMA_DESTINATION_HI_PORT)] = hi;
    }

    fn dma_length(&self) -> u16 {
        u16::from_le_bytes([
            self.io[usize::from(DMA_LENGTH_LO_PORT)],
            self.io[usize::from(DMA_LENGTH_HI_PORT)],
        ])
    }

    fn set_dma_length(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.io[usize::from(DMA_LENGTH_LO_PORT)] = lo;
        self.io[usize::from(DMA_LENGTH_HI_PORT)] = hi;
    }

    fn run_dma_transfer(&mut self, control: u8) {
        let mut source =
            u32::from(self.dma_source_offset()) | (u32::from(self.dma_source_segment()) << 16);
        let mut destination = u32::from(self.dma_destination());
        let mut remaining = self.dma_length();
        let decrement = control & 0x40 != 0;

        while remaining > 0 {
            let lo = self.peek8(source);
            let hi = self.peek8(source.wrapping_add(1));
            self.write8(destination, lo);
            self.write8(destination.wrapping_add(1), hi);
            remaining = remaining.saturating_sub(2);
            if decrement {
                source = source.wrapping_sub(2);
                destination = destination.wrapping_sub(2);
            } else {
                source = source.wrapping_add(2);
                destination = destination.wrapping_add(2);
            }
        }

        self.set_dma_source_offset(source as u16);
        self.set_dma_source_segment((source >> 16) as u16);
        self.set_dma_destination(destination as u16);
        self.set_dma_length(0);
        self.io[usize::from(DMA_CONTROL_PORT)] = control & 0x7F;
    }
}

fn internal_ram_size_for_cartridge(cartridge: &Cartridge) -> usize {
    match cartridge.minimum_system() {
        super::cartridge::MinimumSystem::WonderSwan => WS_INTERNAL_RAM_SIZE,
        super::cartridge::MinimumSystem::WonderSwanColor
        | super::cartridge::MinimumSystem::Unknown(_) => WSC_INTERNAL_RAM_SIZE,
    }
}

fn internal_eeprom_for_cartridge(cartridge: &Cartridge) -> Vec<u8> {
    let (size, label): (usize, &[u8]) = match cartridge.minimum_system() {
        super::cartridge::MinimumSystem::WonderSwan => (WS_INTERNAL_EEPROM_SIZE, b"WONDERSWAN"),
        super::cartridge::MinimumSystem::WonderSwanColor
        | super::cartridge::MinimumSystem::Unknown(_) => {
            (WSC_INTERNAL_EEPROM_SIZE, b"WONDERSWANCOLOR")
        }
    };
    let mut eeprom = vec![0xFF; size];
    eeprom[..label.len()].copy_from_slice(label);
    eeprom
}

fn internal_ram_read(ram: &[u8], addr: u32) -> u8 {
    ram.get(addr as usize).copied().unwrap_or(0x90)
}

fn internal_ram_write(ram: &mut [u8], addr: u32, value: u8) {
    if let Some(slot) = ram.get_mut(addr as usize) {
        *slot = value;
    }
}

fn highest_interrupt_id(pending: u8) -> Option<u8> {
    (pending != 0).then(|| 7 - pending.leading_zeros() as u8)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EepromCommand {
    Read { address: usize },
    Write { address: usize },
    Erase { address: usize },
    WriteDisable,
    WriteAll,
    EraseAll,
    WriteEnable,
    Invalid,
}

fn decode_eeprom_command(save_kind: super::cartridge::SaveKind, command: u16) -> EepromCommand {
    let Some(address_bits) = eeprom_address_bits(save_kind) else {
        return EepromCommand::Invalid;
    };
    let address_mask = (1usize << address_bits) - 1;
    if address_bits <= 6 {
        if command & 0xFF00 != 0x0100 {
            return EepromCommand::Invalid;
        }
        let op = ((command >> 6) & 0x03) as u8;
        if op != 0 {
            return decode_eeprom_address_command(op, usize::from(command & 0x003F));
        }
        return decode_eeprom_short_command(((command >> 4) & 0x03) as u8);
    }

    if command & 0xF000 != 0x1000 {
        return EepromCommand::Invalid;
    }
    let op = ((command >> 10) & 0x03) as u8;
    if op != 0 {
        return decode_eeprom_address_command(op, usize::from(command) & address_mask);
    }
    decode_eeprom_short_command(((command >> 8) & 0x03) as u8)
}

fn eeprom_address_bits(save_kind: super::cartridge::SaveKind) -> Option<usize> {
    match save_kind {
        super::cartridge::SaveKind::Eeprom128 => Some(6),
        super::cartridge::SaveKind::Eeprom1K => Some(9),
        super::cartridge::SaveKind::Eeprom2K => Some(10),
        _ => None,
    }
}

fn decode_eeprom_address_command(op: u8, address: usize) -> EepromCommand {
    match op {
        0x01 => EepromCommand::Write { address },
        0x02 => EepromCommand::Read { address },
        0x03 => EepromCommand::Erase { address },
        _ => EepromCommand::Invalid,
    }
}

fn decode_eeprom_short_command(sub_op: u8) -> EepromCommand {
    match sub_op {
        0x00 => EepromCommand::WriteDisable,
        0x01 => EepromCommand::WriteAll,
        0x02 => EepromCommand::EraseAll,
        0x03 => EepromCommand::WriteEnable,
        _ => EepromCommand::Invalid,
    }
}

#[derive(Clone, Debug)]
struct Rtc {
    command: u8,
    payload: [u8; 7],
    payload_index: usize,
    payload_len: usize,
}

impl Rtc {
    fn new() -> Self {
        Self {
            command: 0,
            payload: default_rtc_payload(),
            payload_index: 0,
            payload_len: 0,
        }
    }

    fn reset(&mut self) {
        self.command = 0;
        self.payload = default_rtc_payload();
        self.payload_index = 0;
        self.payload_len = 0;
    }

    fn write_command(&mut self, value: u8) {
        self.command = value & RTC_COMMAND_MASK;
        self.payload_index = 0;
        self.payload_len = match self.command {
            RTC_WRITE_DATETIME_COMMAND | RTC_READ_DATETIME_COMMAND => self.payload.len(),
            _ => 0,
        };
    }

    fn write_payload(&mut self, value: u8) {
        if self.command != RTC_WRITE_DATETIME_COMMAND || self.payload_index >= self.payload_len {
            return;
        }
        self.payload[self.payload_index] = value;
        self.payload_index += 1;
    }

    fn read_status(&self) -> u8 {
        self.peek_status()
    }

    fn peek_status(&self) -> u8 {
        self.command | RTC_READY
    }

    fn read_payload(&mut self) -> u8 {
        if self.command == RTC_READ_DATETIME_COMMAND && self.payload_index < self.payload_len {
            let value = self.payload[self.payload_index];
            self.payload_index += 1;
            value
        } else {
            RTC_READY
        }
    }

    fn peek_payload(&self) -> u8 {
        if self.command == RTC_READ_DATETIME_COMMAND && self.payload_index < self.payload_len {
            self.payload[self.payload_index]
        } else {
            RTC_READY
        }
    }
}

fn default_rtc_payload() -> [u8; 7] {
    [
        0x00, // year
        0x01, // month
        0x01, // day of month
        0x00, // day of week
        0x00, // hour
        0x00, // minute
        0x00, // second
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::cartridge::compute_footer_checksum;

    fn minimal_cart() -> Cartridge {
        let mut rom = vec![0xFF; 0x10000];
        let footer = rom.len() - 10;
        rom[footer + 1] = 0x00;
        rom[footer + 4] = 0x01;
        let checksum = compute_footer_checksum(&rom);
        rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
        Cartridge::load(&rom).unwrap()
    }

    fn color_cart() -> Cartridge {
        let mut rom = vec![0xFF; 0x10000];
        let footer = rom.len() - 10;
        rom[footer + 1] = 0x01;
        rom[footer + 4] = 0x01;
        let checksum = compute_footer_checksum(&rom);
        rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
        Cartridge::load(&rom).unwrap()
    }

    fn large_cart() -> Cartridge {
        let mut rom = vec![0xFF; 2 * 1024 * 1024];
        rom[0x1F_FFF8] = 0xEA;
        rom[0x0F_FFF8] = 0xFF;
        let footer = rom.len() - 10;
        rom[footer + 1] = 0x00;
        rom[footer + 4] = 0x04;
        let checksum = compute_footer_checksum(&rom);
        rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
        Cartridge::load(&rom).unwrap()
    }

    fn eeprom_cart(save_code: u8) -> Cartridge {
        let mut rom = vec![0xFF; 0x10000];
        let footer = rom.len() - 10;
        rom[footer + 1] = 0x00;
        rom[footer + 4] = 0x01;
        rom[footer + 5] = save_code;
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
    fn mono_internal_ram_above_16k_is_open_bus() {
        let mut bus = Bus::new(minimal_cart());
        assert_eq!(bus.ram.len(), WS_INTERNAL_RAM_SIZE);
        bus.write8(0x0000, 0x12);
        bus.write8(0x3FFF, 0x34);
        bus.write8(0x4000, 0x56);
        bus.write8(0xFFFF, 0x78);
        assert_eq!(bus.read8(0x0000), 0x12);
        assert_eq!(bus.read8(0x3FFF), 0x34);
        assert_eq!(bus.read8(0x4000), 0x90);
        assert_eq!(bus.read8(0xFFFF), 0x90);
    }

    #[test]
    fn color_internal_ram_is_64k() {
        let mut bus = Bus::new(color_cart());
        assert_eq!(bus.ram.len(), WSC_INTERNAL_RAM_SIZE);
        bus.write8(0x4000, 0x56);
        bus.write8(0xFFFF, 0x78);
        assert_eq!(bus.read8(0x4000), 0x56);
        assert_eq!(bus.read8(0xFFFF), 0x78);
    }

    #[test]
    fn reset_applies_wsc_boot_handoff_memory_and_io() {
        let mut bus = Bus::new(color_cart());
        bus.write8(0xFE00, 0x12);
        bus.io_write8(SYSTEM_CONTROL_PORT, 0x00);

        bus.reset();

        assert_eq!(bus.read8(0xFE00), 0xFF);
        assert_eq!(bus.read8(0xFFFF), 0xFF);
        assert_eq!(bus.io_read8(SYSTEM_CONTROL_PORT), 0x87);
        assert_eq!(bus.io_read8(LINE_COMPARE_PORT), 0x00);
        assert_eq!(bus.io_read8(LCD_CONTROL_PORT), 0x01);
        assert_eq!(bus.io_read8(LCD_VTOTAL_PORT), 0x9E);
        assert_eq!(bus.io_read8(0x0060), 0x0A);
        assert_eq!(bus.io_read8(0x009E), 0x03);
        assert_eq!(bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT), 0x80);
        assert_eq!(bus.io_read8(KEYPAD_PORT), 0x40);
        assert_eq!(bus.io_read8(IRQ_STATUS_PORT), 0x00);
        assert_eq!(bus.io_peek8(IRQ_ACK_PORT), 0x00);
    }

    #[test]
    fn io_bank_ports_update_cartridge_banks() {
        let mut bus = Bus::new(minimal_cart());
        bus.io_write8(ROM_BANK0_PORT, 7);
        bus.io_write8(ROM_BANK1_PORT, 8);
        bus.io_write8(ROM_RAM_BANK_PORT, 9);
        bus.io_write8(ROM_LINEAR_BANK_PORT, 2);
        assert_eq!(bus.io_read8(ROM_BANK0_PORT), 7);
        assert_eq!(bus.io_read8(ROM_BANK1_PORT), 8);
        assert_eq!(bus.io_read8(ROM_RAM_BANK_PORT), 9);
        assert_eq!(bus.io_read8(ROM_LINEAR_BANK_PORT), 0x22);
    }

    #[test]
    fn linear_bank_write_is_deferred_for_one_prefetched_instruction() {
        let mut bus = Bus::new(large_cart());

        bus.io_write8(ROM_LINEAR_BANK_PORT, 0x0E);

        assert_eq!(bus.io_read8(ROM_LINEAR_BANK_PORT), 0x2E);
        assert_eq!(bus.read8(0xFFFF8), 0xEA);

        bus.retire_instruction();
        assert_eq!(bus.read8(0xFFFF8), 0xEA);

        bus.retire_instruction();
        assert_eq!(bus.read8(0xFFFF8), 0xFF);
    }

    #[test]
    fn serial_control_reports_transmit_ready_and_masks_writable_bits() {
        let mut bus = Bus::new(minimal_cart());

        assert_eq!(bus.io_read8(SERIAL_CONTROL_PORT), 0x04);

        bus.io_write8(SERIAL_CONTROL_PORT, 0xA7);

        assert_eq!(bus.io_read8(SERIAL_CONTROL_PORT), 0x84);

        bus.io_write8(SERIAL_CONTROL_PORT, 0xC4);

        assert_eq!(bus.io_read8(SERIAL_CONTROL_PORT), 0xC4);
    }

    #[test]
    fn vblank_sets_enabled_interrupt_active_bit() {
        let mut bus = Bus::new(minimal_cart());
        bus.io_write8(IRQ_VECTOR_BASE_PORT, 0x20);
        bus.io_write8(IRQ_ENABLE_PORT, IRQ_VBLANK);

        bus.step_cycles(super::super::constants::CYCLES_PER_SCANLINE * 144);

        assert_eq!(bus.io_read8(IRQ_STATUS_PORT) & IRQ_VBLANK, IRQ_VBLANK);
        assert_eq!(bus.pending_interrupt_vector(), Some(0x26));
        assert_eq!(bus.io_read8(IRQ_VECTOR_BASE_PORT), 0x26);
        assert_eq!(bus.io_read8(IRQ_ACK_PORT), 0x90);
    }

    #[test]
    fn interrupt_acknowledge_port_clears_status_bits() {
        let mut bus = Bus::new(minimal_cart());
        bus.io_write8(IRQ_ENABLE_PORT, IRQ_VBLANK);
        bus.step_cycles(super::super::constants::CYCLES_PER_SCANLINE * 144);
        assert_ne!(bus.io_read8(IRQ_STATUS_PORT) & IRQ_VBLANK, 0);

        bus.io_write8(IRQ_ACK_PORT, IRQ_VBLANK);

        assert_eq!(bus.io_read8(IRQ_STATUS_PORT) & IRQ_VBLANK, 0);
        assert_eq!(bus.pending_interrupt_vector(), None);
    }

    #[test]
    fn interrupt_priority_prefers_highest_pending_bit() {
        let mut bus = Bus::new(minimal_cart());
        bus.io_write8(IRQ_VECTOR_BASE_PORT, 0x20);
        bus.io_write8(IRQ_ENABLE_PORT, IRQ_HBLANK_TIMER | IRQ_VBLANK);
        bus.raise_interrupt(IRQ_VBLANK);
        bus.raise_interrupt(IRQ_HBLANK_TIMER);

        assert_eq!(bus.pending_interrupt_vector(), Some(0x27));
        assert_eq!(bus.io_read8(IRQ_VECTOR_BASE_PORT), 0x27);
        bus.io_write8(IRQ_ACK_PORT, IRQ_HBLANK_TIMER);
        assert_eq!(bus.pending_interrupt_vector(), Some(0x26));
        assert_eq!(bus.io_read8(IRQ_VECTOR_BASE_PORT), 0x26);
    }

    #[test]
    fn interrupt_enable_write_does_not_clear_latched_status_bits() {
        let mut bus = Bus::new(minimal_cart());
        bus.io_write8(IRQ_VECTOR_BASE_PORT, 0x20);
        bus.io_write8(IRQ_ENABLE_PORT, IRQ_HBLANK_TIMER);
        bus.raise_interrupt(IRQ_HBLANK_TIMER);
        assert_eq!(
            bus.io_read8(IRQ_STATUS_PORT) & IRQ_HBLANK_TIMER,
            IRQ_HBLANK_TIMER
        );

        bus.io_write8(IRQ_ENABLE_PORT, IRQ_VBLANK);

        assert_eq!(
            bus.io_read8(IRQ_STATUS_PORT) & IRQ_HBLANK_TIMER,
            IRQ_HBLANK_TIMER
        );
        assert_eq!(bus.pending_interrupt_vector(), Some(0x27));
    }

    #[test]
    fn current_line_port_tracks_ppu_vcount() {
        let mut bus = Bus::new(minimal_cart());

        bus.step_cycles(super::super::constants::CYCLES_PER_SCANLINE * 3);

        assert_eq!(bus.io_read8(CURRENT_LINE_PORT), 3);
    }

    #[test]
    fn line_compare_raises_enabled_interrupt() {
        let mut bus = Bus::new(minimal_cart());
        bus.io_write8(IRQ_VECTOR_BASE_PORT, 0x20);
        bus.io_write8(IRQ_ENABLE_PORT, IRQ_LINE_COMPARE);
        bus.io_write8(LINE_COMPARE_PORT, 2);

        bus.step_cycles(super::super::constants::CYCLES_PER_SCANLINE * 2);

        assert_eq!(
            bus.io_read8(IRQ_STATUS_PORT) & IRQ_LINE_COMPARE,
            IRQ_LINE_COMPARE
        );
        assert_eq!(bus.pending_interrupt_vector(), Some(0x24));
    }

    #[test]
    fn hblank_timer_counts_scanlines_and_raises_interrupt() {
        let mut bus = Bus::new(minimal_cart());
        bus.io_write8(IRQ_VECTOR_BASE_PORT, 0x20);
        bus.io_write8(IRQ_ENABLE_PORT, IRQ_HBLANK_TIMER);
        bus.io_write8(HBLANK_TIMER_RELOAD_LO_PORT, 2);
        bus.io_write8(HBLANK_TIMER_RELOAD_HI_PORT, 0);
        bus.io_write8(TIMER_CONTROL_PORT, 0x01);

        bus.step_cycles(super::super::constants::CYCLES_PER_SCANLINE);
        assert_eq!(bus.io_read8(HBLANK_TIMER_COUNT_LO_PORT), 1);
        assert_eq!(bus.io_read8(IRQ_STATUS_PORT) & IRQ_HBLANK_TIMER, 0);

        bus.step_cycles(super::super::constants::CYCLES_PER_SCANLINE);

        assert_eq!(
            bus.io_read8(IRQ_STATUS_PORT) & IRQ_HBLANK_TIMER,
            IRQ_HBLANK_TIMER
        );
        assert_eq!(bus.pending_interrupt_vector(), Some(0x27));
    }

    #[test]
    fn vblank_timer_counts_frames_and_raises_interrupt() {
        let mut bus = Bus::new(minimal_cart());
        bus.io_write8(IRQ_VECTOR_BASE_PORT, 0x20);
        bus.io_write8(IRQ_ENABLE_PORT, IRQ_VBLANK_TIMER);
        bus.io_write8(VBLANK_TIMER_RELOAD_LO_PORT, 1);
        bus.io_write8(VBLANK_TIMER_RELOAD_HI_PORT, 0);
        bus.io_write8(TIMER_CONTROL_PORT, 0x04);

        bus.step_cycles(super::super::constants::CYCLES_PER_SCANLINE * 144);

        assert_eq!(
            bus.io_read8(IRQ_STATUS_PORT) & IRQ_VBLANK_TIMER,
            IRQ_VBLANK_TIMER
        );
        assert_eq!(bus.pending_interrupt_vector(), Some(0x25));
    }

    #[test]
    fn disabled_hblank_timer_does_not_count_but_still_signals_zero_transition() {
        let mut bus = Bus::new(minimal_cart());
        bus.io_write8(IRQ_VECTOR_BASE_PORT, 0x20);
        bus.io_write8(IRQ_ENABLE_PORT, IRQ_HBLANK_TIMER);
        bus.io_write8(HBLANK_TIMER_RELOAD_LO_PORT, 1);
        bus.io_write8(HBLANK_TIMER_RELOAD_HI_PORT, 0);
        bus.io_write8(TIMER_CONTROL_PORT, 0x00);

        bus.step_cycles(super::super::constants::CYCLES_PER_SCANLINE);

        assert_eq!(bus.io_read8(HBLANK_TIMER_COUNT_LO_PORT), 1);
        assert_eq!(
            bus.io_read8(IRQ_STATUS_PORT) & IRQ_HBLANK_TIMER,
            IRQ_HBLANK_TIMER
        );
        assert_eq!(bus.pending_interrupt_vector(), Some(0x27));
    }

    #[test]
    fn disabled_vblank_timer_does_not_count_but_still_signals_zero_transition() {
        let mut bus = Bus::new(minimal_cart());
        bus.io_write8(IRQ_VECTOR_BASE_PORT, 0x20);
        bus.io_write8(IRQ_ENABLE_PORT, IRQ_VBLANK_TIMER);
        bus.io_write8(VBLANK_TIMER_RELOAD_LO_PORT, 1);
        bus.io_write8(VBLANK_TIMER_RELOAD_HI_PORT, 0);
        bus.io_write8(TIMER_CONTROL_PORT, 0x00);

        bus.step_cycles(super::super::constants::CYCLES_PER_SCANLINE * 144);

        assert_eq!(bus.io_read8(VBLANK_TIMER_COUNT_LO_PORT), 1);
        assert_eq!(
            bus.io_read8(IRQ_STATUS_PORT) & IRQ_VBLANK_TIMER,
            IRQ_VBLANK_TIMER
        );
        assert_eq!(bus.pending_interrupt_vector(), Some(0x25));
    }

    #[test]
    fn internal_eeprom_write_sets_completion_and_can_be_read_back() {
        let mut bus = Bus::new(minimal_cart());
        bus.io_write8(INTERNAL_EEPROM_ADDR_LO_PORT, 3);
        bus.io_write8(INTERNAL_EEPROM_ADDR_HI_PORT, 0);
        bus.io_write8(INTERNAL_EEPROM_DATA_LO_PORT, 0x34);
        bus.io_write8(INTERNAL_EEPROM_DATA_HI_PORT, 0x12);

        bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x20);

        assert_eq!(bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT) & 0x02, 0x02);

        bus.io_write8(INTERNAL_EEPROM_DATA_LO_PORT, 0x00);
        bus.io_write8(INTERNAL_EEPROM_DATA_HI_PORT, 0x00);
        bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x10);

        assert_eq!(bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT) & 0x01, 0x01);
        assert_eq!(bus.io_read8(INTERNAL_EEPROM_DATA_LO_PORT), 0x34);
        assert_eq!(bus.io_read8(INTERNAL_EEPROM_DATA_HI_PORT), 0x12);
    }

    #[test]
    fn system_control_reports_console_type_and_unlock_bit() {
        let mut mono = Bus::new(minimal_cart());
        let mut color = Bus::new(color_cart());

        assert_eq!(mono.io_read8(SYSTEM_CONTROL_PORT) & 0x82, 0x80);
        assert_eq!(color.io_read8(SYSTEM_CONTROL_PORT) & 0x82, 0x82);
    }

    #[test]
    fn dma_control_start_copies_words_and_clears_start_bit() {
        let mut bus = Bus::new(color_cart());
        bus.write8(0x0000, 0x12);
        bus.write8(0x0001, 0x34);
        bus.write8(0x0002, 0x56);
        bus.write8(0x0003, 0x78);
        bus.io_write8(DMA_SOURCE_LO_PORT, 0x00);
        bus.io_write8(DMA_SOURCE_HI_PORT, 0x00);
        bus.io_write8(DMA_SOURCE_SEGMENT_PORT, 0x00);
        bus.io_write8(DMA_DESTINATION_LO_PORT, 0x00);
        bus.io_write8(DMA_DESTINATION_HI_PORT, 0x01);
        bus.io_write8(DMA_LENGTH_LO_PORT, 0x04);
        bus.io_write8(DMA_LENGTH_HI_PORT, 0x00);

        bus.io_write8(DMA_CONTROL_PORT, 0x80);

        assert_eq!(bus.read8(0x0100), 0x12);
        assert_eq!(bus.read8(0x0101), 0x34);
        assert_eq!(bus.read8(0x0102), 0x56);
        assert_eq!(bus.read8(0x0103), 0x78);
        assert_eq!(bus.io_read8(DMA_LENGTH_LO_PORT), 0);
        assert_eq!(bus.io_read8(DMA_LENGTH_HI_PORT), 0);
        assert_eq!(bus.io_read8(DMA_CONTROL_PORT) & 0x80, 0);
    }

    #[test]
    fn cartridge_eeprom_ports_read_back_written_word() {
        let mut bus = Bus::new(eeprom_cart(0x10));
        let write_command = 0x0100 | 0x0040 | 3;
        bus.io_write16(CART_EEPROM_COMMAND_LO_PORT, write_command);
        bus.io_write16(CART_EEPROM_DATA_LO_PORT, 0x1234);
        bus.io_write8(CART_EEPROM_CONTROL_STATUS_LO_PORT, 0x02);

        let read_command = 0x0100 | 0x0080 | 3;
        bus.io_write16(CART_EEPROM_COMMAND_LO_PORT, read_command);
        bus.io_write8(CART_EEPROM_CONTROL_STATUS_LO_PORT, 0x01);

        assert_eq!(bus.io_read16(CART_EEPROM_DATA_LO_PORT), 0x1234);
        assert_eq!(
            bus.io_read8(CART_EEPROM_CONTROL_STATUS_LO_PORT)
                & (EEPROM_STATUS_READ_DONE | EEPROM_STATUS_READY),
            EEPROM_STATUS_READ_DONE | EEPROM_STATUS_READY
        );
    }

    #[test]
    fn cartridge_eeprom_16kbit_command_uses_extended_address() {
        let mut bus = Bus::new(eeprom_cart(0x20));
        let address = 0x02A5;
        bus.io_write16(
            CART_EEPROM_COMMAND_LO_PORT,
            0x1000 | 0x0400 | address as u16,
        );
        bus.io_write16(CART_EEPROM_DATA_LO_PORT, 0xBEEF);
        bus.io_write8(CART_EEPROM_CONTROL_STATUS_LO_PORT, 0x02);

        bus.io_write16(
            CART_EEPROM_COMMAND_LO_PORT,
            0x1000 | 0x0800 | address as u16,
        );
        bus.io_write8(CART_EEPROM_CONTROL_STATUS_LO_PORT, 0x01);

        assert_eq!(bus.io_read16(CART_EEPROM_DATA_LO_PORT), 0xBEEF);
    }

    #[test]
    fn rtc_command_status_reports_ready() {
        let mut bus = Bus::new(color_cart());

        bus.io_write8(RTC_COMMAND_STATUS_PORT, 0x13);

        assert_eq!(
            bus.io_read8(RTC_COMMAND_STATUS_PORT),
            RTC_READY | (0x13 & RTC_COMMAND_MASK)
        );
    }

    #[test]
    fn rtc_datetime_payload_can_be_written_and_read_back() {
        let mut bus = Bus::new(color_cart());
        let payload = [0x26, 0x08, 0x02, 0x00, 0x19, 0x45, 0x30];

        bus.io_write8(RTC_COMMAND_STATUS_PORT, RTC_WRITE_DATETIME_COMMAND);
        for value in payload {
            bus.io_write8(RTC_PAYLOAD_PORT, value);
        }

        bus.io_write8(RTC_COMMAND_STATUS_PORT, RTC_READ_DATETIME_COMMAND);
        let mut read_back = [0; 7];
        for value in &mut read_back {
            *value = bus.io_read8(RTC_PAYLOAD_PORT);
        }

        assert_eq!(read_back, payload);
        assert_eq!(bus.io_read8(RTC_PAYLOAD_PORT), RTC_READY);
    }
}
