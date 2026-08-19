use super::apu::{Apu, ApuDebugSnapshot};
use super::cartridge::Cartridge;
use super::constants::{ADDRESS_MASK, IO_PORT_COUNT, WS_INTERNAL_RAM_SIZE, WSC_INTERNAL_RAM_SIZE};
use super::keypad::Keypad;
use super::ppu::{Ppu, PpuDebugSnapshot};
mod dma;
use dma::SoundDma;
mod eeprom;
use eeprom::{EepromCommand, decode_eeprom_command};
mod interrupts;
mod io;
mod ports;
use ports::*;
mod rtc;
use rtc::Rtc;
mod serial;
use serial::Uart;
pub use serial::UartDebugSnapshot;
pub(crate) use serial::UartSaveState;
pub use serial::WonderSwanTxEvent;
mod timers;
pub use zeff_emu_common::debug::BusAccessEvent;
use zeff_emu_common::debug::{TraceWriteKind, TraceWriteWidth};
pub type DebugTraceEvent = BusAccessEvent;

#[cfg(feature = "profiling")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProfilingSnapshot {
    pub bus_step_calls: u64,
    pub master_cycles: u64,
    pub uart_step_calls: u64,
    pub apu_step_calls: u64,
    pub sound_dma_step_calls: u64,
    pub ppu_step_calls: u64,
    pub completed_scanlines: u64,
    pub vblank_starts: u64,
    pub line_compare_events: u64,
    pub hblank_timer_advances: u64,
    pub vblank_timer_advances: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DebugTraceMode {
    #[default]
    None,
    MemoryAndIo,
    IoOnly,
    WritesOnly,
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
    uart: Uart,
    sound_dma: SoundDma,
    internal_eeprom_write_enabled: bool,
    internal_eeprom_protected: bool,
    internal_eeprom_done_delay_reads: u8,
    cartridge_eeprom_write_enabled: bool,
    pending_linear_bank: Option<DeferredLinearBank>,
    pub cycles: u64,
    pub(crate) debug_trace_mode: DebugTraceMode,
    pub(crate) debug_trace_events: Vec<BusAccessEvent>,
    #[cfg(feature = "profiling")]
    profiling: ProfilingSnapshot,
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
            uart: Uart::default(),
            sound_dma: SoundDma::default(),
            internal_eeprom_write_enabled: true,
            internal_eeprom_protected: false,
            internal_eeprom_done_delay_reads: 0,
            cartridge_eeprom_write_enabled: false,
            pending_linear_bank: None,
            cycles: 0,
            debug_trace_mode: DebugTraceMode::None,
            debug_trace_events: Vec::new(),
            #[cfg(feature = "profiling")]
            profiling: ProfilingSnapshot::default(),
        }
    }

    pub fn reset(&mut self) {
        self.ram.fill(0);
        self.io.fill(0);
        self.internal_eeprom = internal_eeprom_for_cartridge(&self.cartridge);
        self.rtc.reset();
        self.uart.reset();
        self.sound_dma = SoundDma::default();
        self.internal_eeprom_write_enabled = true;
        self.internal_eeprom_protected = false;
        self.internal_eeprom_done_delay_reads = 0;
        self.cartridge_eeprom_write_enabled = false;
        self.pending_linear_bank = None;
        self.cycles = 0;
        self.cartridge.reset_banks();
        self.ppu.reset();
        self.apu.reset();
        self.keypad = Keypad::new();
        self.apply_cartridge_start_state();
    }

    pub(crate) fn is_color_model(&self) -> bool {
        self.cartridge.minimum_system() != super::cartridge::MinimumSystem::WonderSwan
    }

    pub fn read8(&mut self, addr: u32) -> u8 {
        let addr = addr & ADDRESS_MASK;
        let value = self.peek8(addr);
        self.record_memory(BusAccessEvent::Read {
            at: None,
            space: TraceWriteKind::Memory,
            addr,
            value: u32::from(value),
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        });
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
        self.record_memory(BusAccessEvent::Write {
            at: None,
            space: TraceWriteKind::Memory,
            addr,
            old_value: u32::from(old_value),
            written_value: u32::from(value),
            new_value: u32::from(new_value),
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        });
    }

    pub fn write16(&mut self, addr: u32, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.write8(addr, lo);
        self.write8(addr.wrapping_add(1), hi);
    }

    pub fn step_cycles(&mut self, cycles: u32) {
        #[cfg(feature = "profiling")]
        {
            self.profiling.bus_step_calls = self.profiling.bus_step_calls.wrapping_add(1);
            self.profiling.master_cycles =
                self.profiling.master_cycles.wrapping_add(u64::from(cycles));
            self.profiling.uart_step_calls = self.profiling.uart_step_calls.wrapping_add(1);
            self.profiling.apu_step_calls = self.profiling.apu_step_calls.wrapping_add(1);
            self.profiling.sound_dma_step_calls =
                self.profiling.sound_dma_step_calls.wrapping_add(1);
            self.profiling.ppu_step_calls = self.profiling.ppu_step_calls.wrapping_add(1);
        }
        self.cycles = self.cycles.wrapping_add(u64::from(cycles));
        let serial_control = self.io[usize::from(SERIAL_CONTROL_PORT)];
        self.uart.step_cycles(cycles, serial_control, self.cycles);
        self.refresh_level_interrupts();
        self.apu.step_cycles(cycles, &self.ram);
        self.step_sound_dma(cycles);
        let ppu_events = self.ppu.step_cycles(cycles, &self.ram, &self.io);
        #[cfg(feature = "profiling")]
        {
            self.profiling.completed_scanlines = self
                .profiling
                .completed_scanlines
                .wrapping_add(u64::from(ppu_events.completed_scanlines));
            self.profiling.hblank_timer_advances = self
                .profiling
                .hblank_timer_advances
                .wrapping_add(u64::from(ppu_events.completed_scanlines));
        }
        self.step_hblank_timer(ppu_events.completed_scanlines);
        if ppu_events.vblank_started {
            #[cfg(feature = "profiling")]
            {
                self.profiling.vblank_starts = self.profiling.vblank_starts.wrapping_add(1);
                self.profiling.vblank_timer_advances =
                    self.profiling.vblank_timer_advances.wrapping_add(1);
            }
            self.raise_interrupt(IRQ_VBLANK);
            self.step_vblank_timer();
        }
        if ppu_events.line_compare {
            #[cfg(feature = "profiling")]
            {
                self.profiling.line_compare_events =
                    self.profiling.line_compare_events.wrapping_add(1);
            }
            self.raise_interrupt(IRQ_LINE_COMPARE);
        }
    }

    #[cfg(feature = "profiling")]
    pub fn profiling_snapshot(&self) -> ProfilingSnapshot {
        self.profiling
    }

    #[cfg(feature = "profiling")]
    pub fn reset_profiling(&mut self) {
        self.profiling = ProfilingSnapshot::default();
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

    pub(crate) fn flush_deferred_linear_bank(&mut self) {
        let Some(pending) = self.pending_linear_bank else {
            return;
        };
        self.cartridge.set_linear_bank(pending.value);
        self.pending_linear_bank = None;
    }

    pub fn ppu_debug_snapshot(&self) -> PpuDebugSnapshot {
        self.ppu.debug_snapshot()
    }

    pub fn apu_debug_snapshot(&self) -> ApuDebugSnapshot {
        self.apu.debug_snapshot()
    }

    pub(crate) fn take_debug_trace_events(&mut self) -> Vec<BusAccessEvent> {
        std::mem::take(&mut self.debug_trace_events)
    }

    fn io_open_bus(&self) -> u8 {
        if self.cartridge.minimum_system() == super::cartridge::MinimumSystem::WonderSwan {
            0x90
        } else {
            0x00
        }
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

    fn record_memory(&mut self, event: BusAccessEvent) {
        if self.debug_trace_mode == DebugTraceMode::MemoryAndIo
            || (self.debug_trace_mode == DebugTraceMode::WritesOnly
                && matches!(event, BusAccessEvent::Write { .. }))
        {
            self.debug_trace_events.push(event);
        }
    }

    fn record_io(&mut self, event: BusAccessEvent) {
        if matches!(
            self.debug_trace_mode,
            DebugTraceMode::MemoryAndIo | DebugTraceMode::IoOnly
        ) || (self.debug_trace_mode == DebugTraceMode::WritesOnly
            && matches!(event, BusAccessEvent::Write { .. }))
        {
            self.debug_trace_events.push(event);
        }
    }

    fn system_control_read(&self) -> u8 {
        let color_bit = u8::from(
            self.cartridge.minimum_system() != super::cartridge::MinimumSystem::WonderSwan,
        ) << 1;
        (self.io[usize::from(SYSTEM_CONTROL_PORT)] & 0x7D) | color_bit | 0x80
    }

    fn serial_control_read(&self) -> u8 {
        self.uart.status(self.io[usize::from(SERIAL_CONTROL_PORT)])
    }

    pub fn uart_debug_snapshot(&self) -> UartDebugSnapshot {
        self.uart
            .debug_snapshot(self.io[usize::from(SERIAL_CONTROL_PORT)])
    }

    pub fn sync_wonder_swan_link_peer(&mut self, peer: &mut Bus) {
        let self_control = self.io[usize::from(SERIAL_CONTROL_PORT)];
        let peer_control = peer.io[usize::from(SERIAL_CONTROL_PORT)];
        while let Some(event) = self.uart.take_completed_tx() {
            peer.uart.receive_byte(event.byte, peer_control);
        }
        while let Some(event) = peer.uart.take_completed_tx() {
            self.uart.receive_byte(event.byte, self_control);
        }
        self.refresh_level_interrupts();
        peer.refresh_level_interrupts();
    }

    pub fn take_wonder_swan_link_tx_event(&mut self) -> Option<WonderSwanTxEvent> {
        self.uart.take_completed_tx()
    }

    pub fn receive_wonder_swan_link_byte(&mut self, value: u8) {
        let control = self.io[usize::from(SERIAL_CONTROL_PORT)];
        self.uart.receive_byte(value, control);
        self.refresh_level_interrupts();
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

    fn write_mono_palette_port(&mut self, port: u16, value: u8) {
        let palette_index = (port - MONO_PALETTE_PORT_START) / 2;
        let mut masked = value & 0x77;
        if palette_index & 0x04 != 0 && port & 1 == 0 {
            masked &= 0x70;
        }
        self.io[usize::from(port)] = masked;
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

    fn internal_eeprom_status_peek(&self) -> u8 {
        let mut status = EEPROM_STATUS_READY;
        if self.io[usize::from(INTERNAL_EEPROM_COMMAND_PORT)] & EEPROM_STATUS_READ_DONE != 0
            && self.internal_eeprom_done_delay_reads == 0
        {
            status |= EEPROM_STATUS_READ_DONE;
        }
        if self.internal_eeprom_protected {
            status |= EEPROM_STATUS_PROTECTED;
        }
        status
    }

    fn internal_eeprom_status_read(&mut self) -> u8 {
        let status = self.internal_eeprom_status_peek();
        if self.internal_eeprom_done_delay_reads > 0 {
            self.internal_eeprom_done_delay_reads -= 1;
            if self.internal_eeprom_done_delay_reads == 0 {
                self.io[usize::from(INTERNAL_EEPROM_COMMAND_PORT)] |= EEPROM_STATUS_READ_DONE;
            }
        }
        status
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
        self.internal_eeprom_done_delay_reads = 0;
        if value & EEPROM_STATUS_PROTECTED != 0 {
            self.internal_eeprom_protected = true;
            self.io[usize::from(INTERNAL_EEPROM_COMMAND_PORT)] = EEPROM_STATUS_READY;
            return;
        }

        match value & 0x70 {
            0x10 => self.read_internal_eeprom_word(),
            0x20 => self.write_internal_eeprom_word(),
            0x40 => self.write_internal_eeprom_short_command(),
            _ => {
                self.io[usize::from(INTERNAL_EEPROM_COMMAND_PORT)] = EEPROM_STATUS_READY;
            }
        }
    }

    fn read_internal_eeprom_word(&mut self) {
        let address = self.internal_eeprom_address();
        let lo = self.internal_eeprom.get(address).copied().unwrap_or(0xFF);
        let hi = self
            .internal_eeprom
            .get(address + 1)
            .copied()
            .unwrap_or(0xFF);
        self.set_internal_eeprom_data(u16::from_le_bytes([lo, hi]));
        self.io[usize::from(INTERNAL_EEPROM_COMMAND_PORT)] = EEPROM_STATUS_READY;
        self.internal_eeprom_done_delay_reads = 1;
    }

    fn write_internal_eeprom_word(&mut self) {
        let address = self.internal_eeprom_address();
        let previous = self.internal_eeprom_word_at(address);
        if self.internal_eeprom_write_enabled && !self.internal_eeprom_address_protected(address) {
            let data = self.internal_eeprom_data().to_le_bytes();
            if address + 1 < self.internal_eeprom.len() {
                self.internal_eeprom[address] = data[0];
                self.internal_eeprom[address + 1] = data[1];
            }
        }
        self.set_internal_eeprom_data(previous);
        self.io[usize::from(INTERNAL_EEPROM_COMMAND_PORT)] = EEPROM_STATUS_READY;
    }

    fn write_internal_eeprom_short_command(&mut self) {
        match decode_eeprom_command(
            super::cartridge::SaveKind::Eeprom128,
            self.internal_eeprom_command(),
        ) {
            EepromCommand::WriteDisable => self.internal_eeprom_write_enabled = false,
            EepromCommand::WriteEnable => self.internal_eeprom_write_enabled = true,
            EepromCommand::Erase { address } => {
                let byte_address = address * 2;
                if self.internal_eeprom_write_enabled
                    && !self.internal_eeprom_address_protected(byte_address)
                    && byte_address + 1 < self.internal_eeprom.len()
                {
                    self.internal_eeprom[byte_address] = 0xFF;
                    self.internal_eeprom[byte_address + 1] = 0xFF;
                }
            }
            _ => {}
        }
        self.io[usize::from(INTERNAL_EEPROM_COMMAND_PORT)] = EEPROM_STATUS_READY;
    }

    fn internal_eeprom_command(&self) -> u16 {
        u16::from_le_bytes([
            self.io[usize::from(INTERNAL_EEPROM_ADDR_LO_PORT)],
            self.io[usize::from(INTERNAL_EEPROM_ADDR_HI_PORT)],
        ])
    }

    fn internal_eeprom_word_at(&self, address: usize) -> u16 {
        u16::from_le_bytes([
            self.internal_eeprom.get(address).copied().unwrap_or(0xFF),
            self.internal_eeprom
                .get(address + 1)
                .copied()
                .unwrap_or(0xFF),
        ])
    }

    fn internal_eeprom_address_protected(&self, address: usize) -> bool {
        self.internal_eeprom_protected && address >= 0x60
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

        let command =
            decode_eeprom_command(self.cartridge.save_kind(), self.cartridge_eeprom_command());
        match (value & 0xF0, command) {
            (0x10, EepromCommand::Read { address }) => {
                let value = self.cartridge.eeprom_read_word(address);
                self.set_cartridge_eeprom_data(value);
                self.set_cartridge_eeprom_read_done(true);
            }
            (0x10, _) => {
                self.set_cartridge_eeprom_read_done(true);
            }
            (0x20, EepromCommand::Write { address }) => {
                if self.cartridge_eeprom_write_enabled {
                    self.cartridge
                        .eeprom_write_word(address, self.cartridge_eeprom_data());
                }
                self.set_cartridge_eeprom_read_done(false);
            }
            (0x20, EepromCommand::WriteAll) => {
                if self.cartridge_eeprom_write_enabled {
                    self.cartridge
                        .eeprom_fill_words(self.cartridge_eeprom_data());
                }
                self.set_cartridge_eeprom_read_done(false);
            }
            (0x40, EepromCommand::Erase { address }) => {
                if self.cartridge_eeprom_write_enabled {
                    self.cartridge.eeprom_write_word(address, 0xFFFF);
                }
                self.set_cartridge_eeprom_read_done(false);
            }
            (0x40, EepromCommand::EraseAll) => {
                if self.cartridge_eeprom_write_enabled {
                    self.cartridge.eeprom_fill_words(0xFFFF);
                }
                self.set_cartridge_eeprom_read_done(false);
            }
            (0x40, EepromCommand::WriteDisable) => {
                self.cartridge_eeprom_write_enabled = false;
                self.set_cartridge_eeprom_read_done(false);
            }
            (0x40, EepromCommand::WriteEnable) => {
                self.cartridge_eeprom_write_enabled = true;
                self.set_cartridge_eeprom_read_done(false);
            }
            (0x80, _) => self.set_cartridge_eeprom_read_done(true),
            _ => {
                self.io[usize::from(CART_EEPROM_CONTROL_STATUS_LO_PORT)] = EEPROM_STATUS_READY;
            }
        }
    }

    pub(crate) fn eeprom_save_values(&self) -> (u8, u8) {
        let flags = u8::from(self.internal_eeprom_write_enabled)
            | (u8::from(self.internal_eeprom_protected) << 1)
            | (u8::from(self.cartridge_eeprom_write_enabled) << 2);
        (flags, self.internal_eeprom_done_delay_reads)
    }

    pub(crate) fn load_eeprom_save_values(&mut self, flags: u8, internal_done_delay_reads: u8) {
        self.internal_eeprom_write_enabled = flags & 0x01 != 0;
        self.internal_eeprom_protected = flags & 0x02 != 0;
        self.cartridge_eeprom_write_enabled = flags & 0x04 != 0;
        self.internal_eeprom_done_delay_reads = internal_done_delay_reads;
    }

    pub(crate) fn uart_save_state(&self) -> UartSaveState {
        self.uart.save_state()
    }

    pub(crate) fn load_uart_save_state(&mut self, state: UartSaveState) {
        self.uart.load_state(state);
        self.refresh_level_interrupts();
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

#[cfg(test)]
mod tests;
