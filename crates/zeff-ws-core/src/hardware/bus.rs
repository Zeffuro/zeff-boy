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
        assert_eq!(
            bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT),
            EEPROM_STATUS_READY
        );
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
    fn serial_tx_interrupt_is_level_sensitive_to_uart_and_irq_enable() {
        let mut bus = Bus::new(minimal_cart());

        bus.io_write8(SERIAL_CONTROL_PORT, 0x80);
        assert_eq!(bus.io_read8(IRQ_STATUS_PORT) & IRQ_SERIAL_TX, 0);

        bus.io_write8(IRQ_ENABLE_PORT, IRQ_SERIAL_TX);
        assert_ne!(bus.io_read8(IRQ_STATUS_PORT) & IRQ_SERIAL_TX, 0);

        bus.io_write8(IRQ_ACK_PORT, IRQ_SERIAL_TX);
        assert_ne!(bus.io_read8(IRQ_STATUS_PORT) & IRQ_SERIAL_TX, 0);

        bus.io_write8(SERIAL_CONTROL_PORT, 0x00);
        bus.io_write8(IRQ_ACK_PORT, IRQ_SERIAL_TX);
        assert_eq!(bus.io_read8(IRQ_STATUS_PORT) & IRQ_SERIAL_TX, 0);
    }

    #[test]
    fn serial_tx_completes_after_selected_byte_time_and_syncs_to_peer() {
        let mut left = Bus::new(minimal_cart());
        let mut right = Bus::new(minimal_cart());
        left.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);
        right.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);

        left.io_write8(SERIAL_DATA_PORT, 0x5A);
        assert_eq!(
            left.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_TX_EMPTY,
            0
        );

        left.step_cycles(3_199);
        left.sync_wonder_swan_link_peer(&mut right);
        assert_eq!(
            left.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_TX_EMPTY,
            0
        );
        assert_eq!(
            right.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_RX_READY,
            0
        );

        left.step_cycles(1);
        left.sync_wonder_swan_link_peer(&mut right);

        assert_eq!(
            left.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_TX_EMPTY,
            SERIAL_STATUS_TX_EMPTY
        );
        assert_eq!(
            right.io_peek8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_RX_READY,
            SERIAL_STATUS_RX_READY
        );
        assert_eq!(right.io_peek8(SERIAL_DATA_PORT), 0x5A);
        assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0x5A);
        assert_eq!(
            right.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_RX_READY,
            0
        );
    }

    #[test]
    fn serial_write_while_tx_busy_does_not_replace_active_byte() {
        let mut left = Bus::new(minimal_cart());
        let mut right = Bus::new(minimal_cart());
        left.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);
        right.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);

        left.io_write8(SERIAL_DATA_PORT, 0x11);
        left.step_cycles(1_600);
        left.io_write8(SERIAL_DATA_PORT, 0x22);
        left.step_cycles(1_600);
        left.sync_wonder_swan_link_peer(&mut right);

        assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0x11);
        assert_eq!(left.uart_debug_snapshot().tx_data, 0x11);
    }

    #[test]
    fn serial_completed_tx_events_queue_until_peer_sync() {
        let mut left = Bus::new(minimal_cart());
        let mut right = Bus::new(minimal_cart());
        left.io_write8(
            SERIAL_CONTROL_PORT,
            SERIAL_CONTROL_ENABLE | SERIAL_CONTROL_FAST_BAUD,
        );
        right.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);

        left.io_write8(SERIAL_DATA_PORT, 0x11);
        left.step_cycles(800);
        left.io_write8(SERIAL_DATA_PORT, 0x22);
        left.step_cycles(800);
        assert_eq!(left.uart_debug_snapshot().completed_tx_count, 2);

        left.sync_wonder_swan_link_peer(&mut right);

        assert_eq!(left.uart_debug_snapshot().completed_tx_count, 0);
        assert_eq!(
            right.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_OVERRUN,
            SERIAL_STATUS_OVERRUN
        );
        assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0x11);
    }

    #[test]
    fn serial_fast_baud_completes_in_shorter_byte_time() {
        let mut left = Bus::new(minimal_cart());
        let mut right = Bus::new(minimal_cart());
        left.io_write8(
            SERIAL_CONTROL_PORT,
            SERIAL_CONTROL_ENABLE | SERIAL_CONTROL_FAST_BAUD,
        );
        right.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);

        left.io_write8(SERIAL_DATA_PORT, 0xA5);
        left.step_cycles(799);
        left.sync_wonder_swan_link_peer(&mut right);
        assert_eq!(
            right.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_RX_READY,
            0
        );

        left.step_cycles(1);
        left.sync_wonder_swan_link_peer(&mut right);

        assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0xA5);
    }

    #[test]
    fn serial_rx_interrupt_is_level_sensitive_to_uart_and_irq_enable() {
        let mut left = Bus::new(minimal_cart());
        let mut right = Bus::new(minimal_cart());
        left.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);
        right.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);
        right.io_write8(IRQ_ENABLE_PORT, IRQ_SERIAL_RX);

        left.io_write8(SERIAL_DATA_PORT, 0x42);
        left.step_cycles(3_200);
        left.sync_wonder_swan_link_peer(&mut right);

        assert_eq!(
            right.io_read8(IRQ_STATUS_PORT) & IRQ_SERIAL_RX,
            IRQ_SERIAL_RX
        );
        right.io_write8(IRQ_ACK_PORT, IRQ_SERIAL_RX);
        assert_eq!(
            right.io_read8(IRQ_STATUS_PORT) & IRQ_SERIAL_RX,
            IRQ_SERIAL_RX
        );

        assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0x42);
        right.io_write8(IRQ_ACK_PORT, IRQ_SERIAL_RX);

        assert_eq!(right.io_read8(IRQ_STATUS_PORT) & IRQ_SERIAL_RX, 0);
    }

    #[test]
    fn serial_receive_overrun_preserves_buffer_until_reset() {
        let mut left = Bus::new(minimal_cart());
        let mut right = Bus::new(minimal_cart());
        left.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);
        right.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);

        left.io_write8(SERIAL_DATA_PORT, 0x11);
        left.step_cycles(3_200);
        left.sync_wonder_swan_link_peer(&mut right);
        left.io_write8(SERIAL_DATA_PORT, 0x22);
        left.step_cycles(3_200);
        left.sync_wonder_swan_link_peer(&mut right);

        assert_eq!(
            right.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_OVERRUN,
            SERIAL_STATUS_OVERRUN
        );
        assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0x11);

        right.io_write8(
            SERIAL_CONTROL_PORT,
            SERIAL_CONTROL_ENABLE | SERIAL_CONTROL_RESET_OVERRUN,
        );

        assert_eq!(
            right.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_OVERRUN,
            0
        );
    }

    #[test]
    fn serial_overrun_latches_error_but_allows_receive_after_buffer_read() {
        let mut left = Bus::new(minimal_cart());
        let mut right = Bus::new(minimal_cart());
        left.io_write8(
            SERIAL_CONTROL_PORT,
            SERIAL_CONTROL_ENABLE | SERIAL_CONTROL_FAST_BAUD,
        );
        right.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);

        left.io_write8(SERIAL_DATA_PORT, 0x11);
        left.step_cycles(800);
        left.sync_wonder_swan_link_peer(&mut right);
        left.io_write8(SERIAL_DATA_PORT, 0x22);
        left.step_cycles(800);
        left.sync_wonder_swan_link_peer(&mut right);
        assert_eq!(
            right.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_OVERRUN,
            SERIAL_STATUS_OVERRUN
        );

        assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0x11);
        left.io_write8(SERIAL_DATA_PORT, 0x33);
        left.step_cycles(800);
        left.sync_wonder_swan_link_peer(&mut right);
        assert_eq!(
            right.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_RX_READY,
            SERIAL_STATUS_RX_READY
        );
        assert_eq!(
            right.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_OVERRUN,
            SERIAL_STATUS_OVERRUN,
            "overrun remains software-visible until B3 bit 5 reset"
        );
        assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0x33);

        right.io_write8(
            SERIAL_CONTROL_PORT,
            SERIAL_CONTROL_ENABLE | SERIAL_CONTROL_RESET_OVERRUN,
        );
        left.io_write8(SERIAL_DATA_PORT, 0x44);
        left.step_cycles(800);
        left.sync_wonder_swan_link_peer(&mut right);

        assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0x44);
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
        bus.debug_trace_mode = DebugTraceMode::IoOnly;

        bus.io_write8(IRQ_ACK_PORT, IRQ_VBLANK);

        assert_eq!(bus.io_read8(IRQ_STATUS_PORT) & IRQ_VBLANK, 0);
        assert_eq!(bus.pending_interrupt_vector(), None);
        assert!(bus.debug_trace_events.iter().any(|event| matches!(
            event,
            BusAccessEvent::Write {
                at: None,
                space: TraceWriteKind::Io,
                addr: 0x00B6,
                written_value,
                width: TraceWriteWidth::Byte,
                ..
            } if *written_value == u32::from(IRQ_VBLANK)
        )));
    }

    #[test]
    fn writes_only_trace_skips_reads() {
        let mut bus = Bus::new(minimal_cart());
        bus.debug_trace_mode = DebugTraceMode::WritesOnly;

        bus.read8(0);
        bus.io_read8(IRQ_ENABLE_PORT);
        bus.write8(0, 0x12);
        bus.io_write8(IRQ_ENABLE_PORT, IRQ_VBLANK);

        assert_eq!(bus.debug_trace_events.len(), 2);
        assert!(
            bus.debug_trace_events
                .iter()
                .all(|event| matches!(event, BusAccessEvent::Write { .. }))
        );
    }

    #[cfg(feature = "profiling")]
    #[test]
    fn profiling_counts_device_calls_and_transitions() {
        let mut bus = Bus::new(minimal_cart());
        let cycles = super::super::constants::CYCLES_PER_SCANLINE * 144;

        bus.step_cycles(cycles);

        let snapshot = bus.profiling_snapshot();
        assert_eq!(snapshot.bus_step_calls, 1);
        assert_eq!(snapshot.master_cycles, u64::from(cycles));
        assert_eq!(snapshot.uart_step_calls, 1);
        assert_eq!(snapshot.apu_step_calls, 1);
        assert_eq!(snapshot.sound_dma_step_calls, 1);
        assert_eq!(snapshot.ppu_step_calls, 1);
        assert_eq!(snapshot.completed_scanlines, 144);
        assert_eq!(snapshot.vblank_starts, 1);
        assert_eq!(snapshot.hblank_timer_advances, 144);
        assert_eq!(snapshot.vblank_timer_advances, 1);

        bus.reset_profiling();
        assert_eq!(bus.profiling_snapshot(), ProfilingSnapshot::default());
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

        assert_eq!(bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT) & 0x7E, 0x02);

        bus.io_write8(INTERNAL_EEPROM_DATA_LO_PORT, 0x00);
        bus.io_write8(INTERNAL_EEPROM_DATA_HI_PORT, 0x00);
        bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x10);

        assert_eq!(bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT) & 0x01, 0x00);
        assert_eq!(bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT) & 0x01, 0x01);
        assert_eq!(bus.io_read8(INTERNAL_EEPROM_DATA_LO_PORT), 0x34);
        assert_eq!(bus.io_read8(INTERNAL_EEPROM_DATA_HI_PORT), 0x12);
    }

    #[test]
    fn internal_eeprom_lock_unlock_and_protect_affect_writes() {
        let mut bus = Bus::new(minimal_cart());

        bus.io_write16(INTERNAL_EEPROM_ADDR_LO_PORT, 0x0100);
        bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x40);
        bus.io_write16(INTERNAL_EEPROM_ADDR_LO_PORT, 0x0140);
        bus.io_write16(INTERNAL_EEPROM_DATA_LO_PORT, 0x1234);
        bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x20);
        bus.io_write16(INTERNAL_EEPROM_ADDR_LO_PORT, 0x0180);
        bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x10);
        bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT);
        bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT);
        assert_ne!(bus.io_read16(INTERNAL_EEPROM_DATA_LO_PORT), 0x1234);

        bus.io_write16(INTERNAL_EEPROM_ADDR_LO_PORT, 0x0130);
        bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x40);
        bus.io_write16(INTERNAL_EEPROM_ADDR_LO_PORT, 0x0140);
        bus.io_write16(INTERNAL_EEPROM_DATA_LO_PORT, 0x1234);
        bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x20);
        bus.io_write16(INTERNAL_EEPROM_ADDR_LO_PORT, 0x0180);
        bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x10);
        bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT);
        bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT);
        assert_eq!(bus.io_read16(INTERNAL_EEPROM_DATA_LO_PORT), 0x1234);

        bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, EEPROM_STATUS_PROTECTED);
        assert_ne!(
            bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT) & EEPROM_STATUS_PROTECTED,
            0
        );
        bus.io_write16(INTERNAL_EEPROM_ADDR_LO_PORT, 0x0170);
        bus.io_write16(INTERNAL_EEPROM_DATA_LO_PORT, 0x5678);
        bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x20);
        bus.io_write16(INTERNAL_EEPROM_ADDR_LO_PORT, 0x01B0);
        bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x10);
        bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT);
        bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT);
        assert_ne!(bus.io_read16(INTERNAL_EEPROM_DATA_LO_PORT), 0x5678);
    }

    #[test]
    fn system_control_reports_console_type_and_unlock_bit() {
        let mut mono = Bus::new(minimal_cart());
        let mut color = Bus::new(color_cart());

        assert_eq!(mono.io_read8(SYSTEM_CONTROL_PORT) & 0x82, 0x80);
        assert_eq!(color.io_read8(SYSTEM_CONTROL_PORT) & 0x82, 0x82);
    }

    #[test]
    fn mono_palette_registers_mask_to_hardware_writable_bits() {
        let mut bus = Bus::new(minimal_cart());

        bus.io_write16(0x20, 0xFFFF);
        bus.io_write16(0x28, 0xFFFF);
        bus.io_write16(0x30, 0x4321);
        bus.io_write16(0x38, 0x4321);

        assert_eq!(bus.io_read16(0x20), 0x7777);
        assert_eq!(bus.io_read16(0x28), 0x7770);
        assert_eq!(bus.io_read16(0x30), 0x4321);
        assert_eq!(bus.io_read16(0x38), 0x4320);
    }

    #[test]
    fn color_model_keeps_mono_palette_io_byte_writes_unmasked() {
        let mut bus = Bus::new(color_cart());

        bus.io_write16(0x28, 0xFFFF);

        assert_eq!(bus.io_read16(0x28), 0xFFFF);
    }

    #[test]
    fn mono_model_hides_color_dma_ports() {
        let mut bus = Bus::new(minimal_cart());
        bus.write8(0x0000, 0x12);
        bus.io_write16(DMA_SOURCE_LO_PORT, 0x0000);
        bus.io_write16(DMA_SOURCE_SEGMENT_PORT, 0x0000);
        bus.io_write16(DMA_DESTINATION_LO_PORT, 0x0100);
        bus.io_write16(DMA_LENGTH_LO_PORT, 0x0002);
        bus.io_write8(DMA_CONTROL_PORT, 0x80);

        assert_eq!(bus.io_read8(DMA_SOURCE_LO_PORT), 0x90);
        assert_eq!(bus.io_read8(DMA_CONTROL_PORT), 0x90);
        assert_eq!(bus.read8(0x0100), 0x00);
    }

    #[test]
    fn mono_model_hides_hyper_voice_control_ports() {
        let mut bus = Bus::new(minimal_cart());

        bus.io_write8(0x006A, 0xFF);
        bus.io_write8(0x006B, 0xFF);

        assert_eq!(bus.io_read8(0x006A), 0x90);
        assert_eq!(bus.io_read8(0x006B), 0x90);
        let apu = bus.apu_debug_snapshot();
        assert_eq!(apu.hyper_voice_control, 0);
        assert_eq!(apu.hyper_voice_channel_control, 0);
    }

    #[test]
    fn color_model_exposes_hyper_voice_control_ports() {
        let mut bus = Bus::new(color_cart());

        bus.io_write8(0x006A, 0x8F);
        bus.io_write8(0x006B, 0xFF);

        assert_eq!(bus.io_read8(0x006A), 0x8F);
        assert_eq!(bus.io_read8(0x006B), 0x70);
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
    fn dma_registers_mask_alignment_and_source_high_word() {
        let mut bus = Bus::new(color_cart());

        bus.io_write16(DMA_SOURCE_LO_PORT, 0xB001);
        bus.io_write16(DMA_SOURCE_SEGMENT_PORT, 0xFFFF);
        bus.io_write16(DMA_DESTINATION_LO_PORT, 0x7001);
        bus.io_write16(DMA_LENGTH_LO_PORT, 0xFFFF);

        assert_eq!(bus.io_read16(DMA_SOURCE_LO_PORT), 0xB000);
        assert_eq!(bus.io_read16(DMA_SOURCE_SEGMENT_PORT), 0x000F);
        assert_eq!(bus.io_read16(DMA_DESTINATION_LO_PORT), 0x7000);
        assert_eq!(bus.io_read16(DMA_LENGTH_LO_PORT), 0xFFFE);
    }

    #[test]
    fn dma_rejects_sram_and_slow_rom_sources_without_consuming_length() {
        let mut bus = Bus::new(color_cart());

        bus.io_write16(DMA_SOURCE_SEGMENT_PORT, 0x0001);
        bus.io_write16(DMA_SOURCE_LO_PORT, 0x0000);
        bus.io_write16(DMA_DESTINATION_LO_PORT, 0x7000);
        bus.io_write16(DMA_LENGTH_LO_PORT, 0x1000);
        bus.io_write8(DMA_CONTROL_PORT, 0x80);
        assert_eq!(bus.io_read16(DMA_LENGTH_LO_PORT), 0x1000);

        bus.io_write16(DMA_SOURCE_SEGMENT_PORT, 0x0008);
        bus.io_write16(DMA_SOURCE_LO_PORT, 0x0000);
        bus.io_write16(DMA_DESTINATION_LO_PORT, 0x7000);
        bus.io_write16(DMA_LENGTH_LO_PORT, 0x1000);
        let slow_rom_control = bus.io_read8(SYSTEM_CONTROL_PORT) | SYSTEM_CTRL1_ROM_WAIT;
        bus.io_write8(SYSTEM_CONTROL_PORT, slow_rom_control);
        bus.io_write8(DMA_CONTROL_PORT, 0x80);
        assert_eq!(bus.io_read16(DMA_LENGTH_LO_PORT), 0x1000);

        let fast_rom_control = bus.io_read8(SYSTEM_CONTROL_PORT) & !SYSTEM_CTRL1_ROM_WAIT;
        bus.io_write8(SYSTEM_CONTROL_PORT, fast_rom_control);
        bus.io_write8(DMA_CONTROL_PORT, 0x80);
        assert_eq!(bus.io_read16(DMA_LENGTH_LO_PORT), 0x0000);
    }

    #[test]
    fn sound_dma_registers_are_twenty_bit_and_transfer_to_channel_2_volume() {
        let mut bus = Bus::new(color_cart());

        bus.io_write16(SOUND_DMA_SOURCE_LO_PORT, 0x5555);
        bus.io_write16(SOUND_DMA_LENGTH_LO_PORT, 0x5555);
        bus.io_write16(SOUND_DMA_SOURCE_SEGMENT_PORT, 0xFFFF);
        bus.io_write16(SOUND_DMA_LENGTH_SEGMENT_PORT, 0xFFFF);

        assert_eq!(bus.io_read16(SOUND_DMA_SOURCE_LO_PORT), 0x5555);
        assert_eq!(bus.io_read16(SOUND_DMA_SOURCE_SEGMENT_PORT), 0x000F);
        assert_eq!(bus.io_read16(SOUND_DMA_LENGTH_LO_PORT), 0x5555);
        assert_eq!(bus.io_read16(SOUND_DMA_LENGTH_SEGMENT_PORT), 0x000F);

        bus.write8(0x1234, 0x5A);
        bus.io_write16(SOUND_DMA_SOURCE_LO_PORT, 0x1234);
        bus.io_write16(SOUND_DMA_SOURCE_SEGMENT_PORT, 0x0000);
        bus.io_write16(SOUND_DMA_LENGTH_LO_PORT, 0x0001);
        bus.io_write16(SOUND_DMA_LENGTH_SEGMENT_PORT, 0x0000);
        bus.io_write8(SOUND_DMA_CONTROL_PORT, SOUND_DMA_ENABLE | 0x03);
        bus.step_cycles(128);

        assert_eq!(bus.io_read8(SOUND_VOLUME_CHANNEL2_PORT), 0x5A);
        assert_eq!(bus.io_read16(SOUND_DMA_SOURCE_LO_PORT), 0x1235);
        assert_eq!(bus.io_read16(SOUND_DMA_LENGTH_LO_PORT), 0x0000);
        assert_eq!(bus.io_read8(SOUND_DMA_CONTROL_PORT), 0x03);
    }

    #[test]
    fn sound_dma_zero_length_enable_fails() {
        let mut bus = Bus::new(color_cart());
        bus.io_write16(SOUND_DMA_LENGTH_LO_PORT, 0x0000);
        bus.io_write16(SOUND_DMA_LENGTH_SEGMENT_PORT, 0x0000);

        bus.io_write8(SOUND_DMA_CONTROL_PORT, SOUND_DMA_ENABLE | 0x03);

        assert_eq!(bus.io_read8(SOUND_DMA_CONTROL_PORT) & SOUND_DMA_ENABLE, 0);
    }

    #[test]
    fn sound_dma_hold_writes_zero_without_consuming_length() {
        let mut bus = Bus::new(color_cart());
        bus.write8(0x1234, 0x7F);
        bus.io_write16(SOUND_DMA_SOURCE_LO_PORT, 0x1234);
        bus.io_write16(SOUND_DMA_SOURCE_SEGMENT_PORT, 0x0000);
        bus.io_write16(SOUND_DMA_LENGTH_LO_PORT, 0x0001);
        bus.io_write16(SOUND_DMA_LENGTH_SEGMENT_PORT, 0x0000);
        bus.apu.write_hyper_voice_dma_sample(0x55);
        bus.io_write8(
            SOUND_DMA_CONTROL_PORT,
            SOUND_DMA_ENABLE | SOUND_DMA_HOLD | SOUND_DMA_TARGET_HYPERVOICE | 0x03,
        );

        bus.step_cycles(128);

        assert_eq!(bus.apu_debug_snapshot().hyper_voice_sample, 0x00);
        assert_eq!(bus.io_read16(SOUND_DMA_LENGTH_LO_PORT), 0x0001);
        assert_eq!(
            bus.io_read8(SOUND_DMA_CONTROL_PORT) & SOUND_DMA_ENABLE,
            SOUND_DMA_ENABLE
        );
    }

    #[test]
    fn sound_dma_decrement_direction_reads_source_backwards() {
        let mut bus = Bus::new(color_cart());
        bus.write8(0x1234, 0x22);
        bus.write8(0x1235, 0x11);
        bus.io_write16(SOUND_DMA_SOURCE_LO_PORT, 0x1235);
        bus.io_write16(SOUND_DMA_SOURCE_SEGMENT_PORT, 0x0000);
        bus.io_write16(SOUND_DMA_LENGTH_LO_PORT, 0x0002);
        bus.io_write16(SOUND_DMA_LENGTH_SEGMENT_PORT, 0x0000);
        bus.io_write8(
            SOUND_DMA_CONTROL_PORT,
            SOUND_DMA_ENABLE | SOUND_DMA_DECREMENT | 0x03,
        );

        bus.step_cycles(128);
        assert_eq!(bus.io_read8(SOUND_VOLUME_CHANNEL2_PORT), 0x11);
        assert_eq!(bus.io_read16(SOUND_DMA_SOURCE_LO_PORT), 0x1234);
        assert_eq!(bus.io_read16(SOUND_DMA_LENGTH_LO_PORT), 0x0001);

        bus.step_cycles(128);
        assert_eq!(bus.io_read8(SOUND_VOLUME_CHANNEL2_PORT), 0x22);
        assert_eq!(bus.io_read16(SOUND_DMA_SOURCE_LO_PORT), 0x1233);
        assert_eq!(bus.io_read16(SOUND_DMA_LENGTH_LO_PORT), 0x0000);
        assert_eq!(bus.io_read8(SOUND_DMA_CONTROL_PORT) & SOUND_DMA_ENABLE, 0);
    }

    #[test]
    fn sound_dma_can_target_hyper_voice_sample() {
        let mut bus = Bus::new(color_cart());
        bus.write8(0x1234, 0x6A);
        bus.io_write16(SOUND_DMA_SOURCE_LO_PORT, 0x1234);
        bus.io_write16(SOUND_DMA_SOURCE_SEGMENT_PORT, 0x0000);
        bus.io_write16(SOUND_DMA_LENGTH_LO_PORT, 0x0001);
        bus.io_write16(SOUND_DMA_LENGTH_SEGMENT_PORT, 0x0000);
        bus.io_write8(
            SOUND_DMA_CONTROL_PORT,
            SOUND_DMA_ENABLE | SOUND_DMA_TARGET_HYPERVOICE | 0x03,
        );

        bus.step_cycles(128);

        assert_eq!(bus.apu_debug_snapshot().hyper_voice_sample, 0x6A);
        assert_eq!(bus.io_read8(SOUND_DMA_CONTROL_PORT) & SOUND_DMA_ENABLE, 0);
    }

    #[test]
    fn cartridge_eeprom_ports_read_back_written_word() {
        let mut bus = Bus::new(eeprom_cart(0x10));
        bus.io_write16(CART_EEPROM_COMMAND_LO_PORT, 0x0130);
        bus.io_write8(CART_EEPROM_CONTROL_STATUS_LO_PORT, 0x40);

        let write_command = 0x0100 | 0x0040 | 3;
        bus.io_write16(CART_EEPROM_COMMAND_LO_PORT, write_command);
        bus.io_write16(CART_EEPROM_DATA_LO_PORT, 0x1234);
        bus.io_write8(CART_EEPROM_CONTROL_STATUS_LO_PORT, 0x20);

        let read_command = 0x0100 | 0x0080 | 3;
        bus.io_write16(CART_EEPROM_COMMAND_LO_PORT, read_command);
        bus.io_write8(CART_EEPROM_CONTROL_STATUS_LO_PORT, 0x10);

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
        bus.io_write16(CART_EEPROM_COMMAND_LO_PORT, 0x1300);
        bus.io_write8(CART_EEPROM_CONTROL_STATUS_LO_PORT, 0x40);

        let address = 0x02A5;
        bus.io_write16(
            CART_EEPROM_COMMAND_LO_PORT,
            0x1000 | 0x0400 | address as u16,
        );
        bus.io_write16(CART_EEPROM_DATA_LO_PORT, 0xBEEF);
        bus.io_write8(CART_EEPROM_CONTROL_STATUS_LO_PORT, 0x20);

        bus.io_write16(
            CART_EEPROM_COMMAND_LO_PORT,
            0x1000 | 0x0800 | address as u16,
        );
        bus.io_write8(CART_EEPROM_CONTROL_STATUS_LO_PORT, 0x10);

        assert_eq!(bus.io_read16(CART_EEPROM_DATA_LO_PORT), 0xBEEF);
    }

    #[test]
    fn rtc_command_status_reports_ready() {
        let mut bus = Bus::new(color_cart());

        bus.io_write8(RTC_COMMAND_STATUS_PORT, 0x13);

        assert_eq!(bus.io_read8(RTC_COMMAND_STATUS_PORT), RTC_ACTIVE);
        assert_eq!(bus.io_read8(RTC_COMMAND_STATUS_PORT), RTC_ACTIVE);
        assert_eq!(bus.io_read8(RTC_COMMAND_STATUS_PORT), RTC_READY);
    }

    #[test]
    fn rtc_datetime_payload_can_be_written_and_read_back() {
        let mut bus = Bus::new(color_cart());
        let payload = [0x26, 0x08, 0x02, 0x00, 0x19, 0x45, 0x30];

        bus.io_write8(RTC_PAYLOAD_PORT, payload[0]);
        bus.io_write8(RTC_COMMAND_STATUS_PORT, RTC_WRITE_DATETIME_COMMAND);
        for &value in &payload[1..] {
            bus.io_write8(RTC_PAYLOAD_PORT, value);
        }

        bus.io_write8(RTC_COMMAND_STATUS_PORT, RTC_READ_DATETIME_COMMAND);
        assert_eq!(bus.io_read8(RTC_COMMAND_STATUS_PORT), RTC_ACTIVE);
        assert_eq!(bus.io_read8(RTC_COMMAND_STATUS_PORT), RTC_ACTIVE);
        let mut read_back = [0; 7];
        for value in &mut read_back {
            *value = bus.io_read8(RTC_PAYLOAD_PORT);
        }

        assert_eq!(read_back, payload);
        assert_eq!(bus.io_read8(RTC_PAYLOAD_PORT), RTC_READY);
    }
}
