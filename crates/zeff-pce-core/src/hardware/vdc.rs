use anyhow::bail;
use bitflags::bitflags;
use zeff_emu_common::save_state::{StateReader, StateWriter};

use super::bus::BaseBusDevices;
use super::cpu::{LineLevel, VdcPort};
use super::vdc_horizontal::VdcHorizontalPhase;
use super::vdc_horizontal::VdcHorizontalState;
use super::vdc_horizontal::VdcPortWriteResult;
use super::vdc_scanline::{VdcScanlineState, VdcSyncOutput, VdcVerticalPhase};

mod dma;

pub use dma::{
    DETERMINISTIC_VDC_INITIAL_SATB_WORD, DETERMINISTIC_VDC_RESET_CLEARS_SATB, VDC_SATB_WORDS,
    VdcDmaAccess, VdcDmaChannel, VdcDmaDirection, VdcDmaError, VdcDmaProgress, VramDmaState,
    VramSatbDmaState,
};

pub const VDC_VRAM_BYTES: usize = 0x1_0000;
pub const VDC_VRAM_WORDS: usize = VDC_VRAM_BYTES / 2;
pub const VDC_VRAM_WORD_ADDRESS_MASK: usize = VDC_VRAM_WORDS - 1;
pub const VDC_UNAVAILABLE_READ_VALUE: u8 = 0xFF;
pub const DETERMINISTIC_VDC_RESET_VALUE: u16 = 0;
pub const DETERMINISTIC_VDC_RESET_HORIZONTAL_DISPLAY: u16 = 0x001F;
pub const DETERMINISTIC_VDC_RESET_VERTICAL_DISPLAY: u16 = 0x00EF;
pub const DETERMINISTIC_VDC_INITIAL_VRAM_WORD: u16 = 0;
pub const DETERMINISTIC_VDC_RESET_PRESERVES_VRAM: bool = true;

const fn reset_registers() -> [u16; 0x14] {
    let mut registers = [DETERMINISTIC_VDC_RESET_VALUE; 0x14];
    registers[VdcRegister::HorizontalDisplay as usize] = DETERMINISTIC_VDC_RESET_HORIZONTAL_DISPLAY;
    registers[VdcRegister::VerticalDisplay as usize] = DETERMINISTIC_VDC_RESET_VERTICAL_DISPLAY;
    registers
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdcDebugSnapshot {
    pub selected_register_id: u8,
    pub selected_register: Option<VdcRegister>,
    pub registers: [u16; 0x14],
    pub vram_read_buffer: u16,
    pub status: VdcStatus,
    pub irq_asserted: bool,
    pub pending_vram_dma: Option<VramDmaState>,
    pub active_vram_dma: Option<VramDmaState>,
    pub pending_satb_dma: Option<VramSatbDmaState>,
    pub active_satb_dma: Option<VramSatbDmaState>,
    pub horizontal_phase: VdcHorizontalPhase,
    pub horizontal_pixels_remaining: u16,
    pub dma_pixel_remainder: u8,
    pub frame_burst: bool,
    pub sync_output: VdcSyncOutput,
    pub vertical_phase: VdcVerticalPhase,
    pub vertical_phase_line: u16,
    pub vertical_phase_duration: u16,
    pub frame_line: u16,
    pub raster_counter: u16,
    pub satb: [u16; VDC_SATB_WORDS],
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct VdcStatus: u8 {
        const SPRITE_COLLISION = 1 << 0;
        const SPRITE_OVERFLOW = 1 << 1;
        const RASTER_MATCH = 1 << 2;
        const SATB_DMA_COMPLETE = 1 << 3;
        const VRAM_DMA_COMPLETE = 1 << 4;
        const VERTICAL_BLANK = 1 << 5;
        const BUSY = 1 << 6;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum VdcRegister {
    MemoryAddressWrite = 0x00,
    MemoryAddressRead = 0x01,
    VramData = 0x02,
    Control = 0x05,
    RasterCounter = 0x06,
    BackgroundScrollX = 0x07,
    BackgroundScrollY = 0x08,
    MemoryWidth = 0x09,
    HorizontalSync = 0x0A,
    HorizontalDisplay = 0x0B,
    VerticalSync = 0x0C,
    VerticalDisplay = 0x0D,
    VerticalDisplayEnd = 0x0E,
    DmaControl = 0x0F,
    DmaSource = 0x10,
    DmaDestination = 0x11,
    DmaLength = 0x12,
    SatbSource = 0x13,
}

impl VdcRegister {
    #[inline]
    pub const fn from_id(id: u8) -> Option<Self> {
        Some(match id {
            0x00 => Self::MemoryAddressWrite,
            0x01 => Self::MemoryAddressRead,
            0x02 => Self::VramData,
            0x05 => Self::Control,
            0x06 => Self::RasterCounter,
            0x07 => Self::BackgroundScrollX,
            0x08 => Self::BackgroundScrollY,
            0x09 => Self::MemoryWidth,
            0x0A => Self::HorizontalSync,
            0x0B => Self::HorizontalDisplay,
            0x0C => Self::VerticalSync,
            0x0D => Self::VerticalDisplay,
            0x0E => Self::VerticalDisplayEnd,
            0x0F => Self::DmaControl,
            0x10 => Self::DmaSource,
            0x11 => Self::DmaDestination,
            0x12 => Self::DmaLength,
            0x13 => Self::SatbSource,
            _ => return None,
        })
    }

    #[inline]
    const fn writable_mask(self) -> u16 {
        match self {
            Self::MemoryAddressWrite
            | Self::MemoryAddressRead
            | Self::VramData
            | Self::DmaSource
            | Self::DmaDestination
            | Self::DmaLength
            | Self::SatbSource => 0xFFFF,
            Self::Control => 0x1FFF,
            Self::RasterCounter | Self::BackgroundScrollX => 0x03FF,
            Self::BackgroundScrollY | Self::VerticalDisplay => 0x01FF,
            Self::MemoryWidth | Self::VerticalDisplayEnd => 0x00FF,
            Self::HorizontalSync => 0x7F1F,
            Self::HorizontalDisplay => 0x7F7F,
            Self::VerticalSync => 0xFF1F,
            Self::DmaControl => 0x001F,
        }
    }
}

#[derive(Debug)]
pub struct HuC6270 {
    vram: Box<[u16; VDC_VRAM_WORDS]>,
    satb: [u16; VDC_SATB_WORDS],
    registers: [u16; 0x14],
    selected_register_id: u8,
    vram_read_buffer: u16,
    status: VdcStatus,
    vram_dma_pending: Option<VramDmaState>,
    vram_dma_active: Option<VramDmaState>,
    satb_dma_pending: Option<VramSatbDmaState>,
    satb_dma_active: Option<VramSatbDmaState>,
    pub(super) horizontal_state: VdcHorizontalState,
    pub(super) scanline_state: VdcScanlineState,
}

impl Default for HuC6270 {
    fn default() -> Self {
        Self::new()
    }
}

impl HuC6270 {
    pub fn new() -> Self {
        Self {
            vram: Box::new([DETERMINISTIC_VDC_INITIAL_VRAM_WORD; VDC_VRAM_WORDS]),
            satb: [DETERMINISTIC_VDC_INITIAL_SATB_WORD; VDC_SATB_WORDS],
            registers: reset_registers(),
            selected_register_id: DETERMINISTIC_VDC_RESET_VALUE as u8,
            vram_read_buffer: DETERMINISTIC_VDC_RESET_VALUE,
            status: VdcStatus::empty(),
            vram_dma_pending: None,
            vram_dma_active: None,
            satb_dma_pending: None,
            satb_dma_active: None,
            horizontal_state: VdcHorizontalState::default(),
            scanline_state: VdcScanlineState::default(),
        }
    }

    pub fn reset(&mut self) {
        self.registers = reset_registers();
        self.selected_register_id = DETERMINISTIC_VDC_RESET_VALUE as u8;
        self.vram_read_buffer = DETERMINISTIC_VDC_RESET_VALUE;
        self.status = VdcStatus::empty();
        self.vram_dma_pending = None;
        self.vram_dma_active = None;
        self.satb_dma_pending = None;
        self.satb_dma_active = None;
        self.horizontal_state = VdcHorizontalState::default();
        self.scanline_state = VdcScanlineState::default();
        if DETERMINISTIC_VDC_RESET_CLEARS_SATB {
            self.satb.fill(DETERMINISTIC_VDC_INITIAL_SATB_WORD);
        }
    }

    pub fn debug_snapshot(&self) -> VdcDebugSnapshot {
        VdcDebugSnapshot {
            selected_register_id: self.selected_register_id,
            selected_register: self.selected_register(),
            registers: self.registers,
            vram_read_buffer: self.vram_read_buffer,
            status: self.status,
            irq_asserted: self.irq_level() == LineLevel::Low,
            pending_vram_dma: self.pending_vram_dma(),
            active_vram_dma: self.active_vram_dma(),
            pending_satb_dma: self.pending_satb_dma(),
            active_satb_dma: self.active_satb_dma(),
            horizontal_phase: self.horizontal_phase(),
            horizontal_pixels_remaining: self.horizontal_phase_pixels_remaining(),
            dma_pixel_remainder: self.dma_pixel_remainder(),
            frame_burst: self.frame_burst_enabled(),
            sync_output: self.sync_output(),
            vertical_phase: self.vertical_phase(),
            vertical_phase_line: self.vertical_phase_line(),
            vertical_phase_duration: self.current_vertical_phase_duration(),
            frame_line: self.frame_line(),
            raster_counter: self.raster_counter(),
            satb: self.satb,
        }
    }

    #[inline]
    pub fn vram(&self) -> &[u16; VDC_VRAM_WORDS] {
        &self.vram
    }

    #[inline]
    pub fn vram_mut(&mut self) -> &mut [u16; VDC_VRAM_WORDS] {
        &mut self.vram
    }

    #[inline]
    pub const fn satb(&self) -> &[u16; VDC_SATB_WORDS] {
        &self.satb
    }

    #[inline]
    pub const fn selected_register_id(&self) -> u8 {
        self.selected_register_id
    }

    #[inline]
    pub const fn selected_register(&self) -> Option<VdcRegister> {
        VdcRegister::from_id(self.selected_register_id)
    }

    #[inline]
    pub const fn register(&self, register: VdcRegister) -> u16 {
        self.registers[register as usize]
    }

    #[inline]
    pub const fn vram_read_buffer(&self) -> u16 {
        self.vram_read_buffer
    }

    #[inline]
    pub const fn status(&self) -> VdcStatus {
        self.status
    }

    #[inline]
    pub fn latch_status(&mut self, events: VdcStatus) {
        self.status |= events - VdcStatus::BUSY;
    }

    #[inline]
    pub fn set_busy(&mut self, busy: bool) {
        self.status.set(VdcStatus::BUSY, busy);
    }

    #[inline]
    pub fn irq_level(&self) -> LineLevel {
        let control = self.register(VdcRegister::Control);
        let dma_control = self.register(VdcRegister::DmaControl);
        if (control & 0x01 != 0 && self.status.contains(VdcStatus::SPRITE_COLLISION))
            || (control & 0x02 != 0 && self.status.contains(VdcStatus::SPRITE_OVERFLOW))
            || (control & 0x04 != 0 && self.status.contains(VdcStatus::RASTER_MATCH))
            || (control & 0x08 != 0 && self.status.contains(VdcStatus::VERTICAL_BLANK))
            || (dma_control & 0x01 != 0 && self.status.contains(VdcStatus::SATB_DMA_COMPLETE))
            || (dma_control & 0x02 != 0 && self.status.contains(VdcStatus::VRAM_DMA_COMPLETE))
        {
            LineLevel::Low
        } else {
            LineLevel::High
        }
    }

    #[inline]
    pub fn read_port(&mut self, port: VdcPort) -> u8 {
        match port {
            VdcPort::SelectOrStatus => self.read_status(),
            VdcPort::Unused => 0,
            VdcPort::DataLow => self.read_data(false),
            VdcPort::DataHigh => self.read_data(true),
        }
    }

    #[inline]
    pub fn write_port(&mut self, port: VdcPort, value: u8) -> VdcPortWriteResult {
        match port {
            VdcPort::SelectOrStatus => {
                self.selected_register_id = value & 0x1F;
                VdcPortWriteResult::Applied
            }
            VdcPort::Unused => VdcPortWriteResult::Applied,
            VdcPort::DataLow => self.write_data(false, value),
            VdcPort::DataHigh => self.write_data(true, value),
        }
    }

    #[inline]
    fn read_status(&mut self) -> u8 {
        let value = self.status.bits();
        self.status &= VdcStatus::BUSY;
        value
    }

    fn read_data(&mut self, high_byte: bool) -> u8 {
        if self.selected_register() != Some(VdcRegister::VramData) {
            return VDC_UNAVAILABLE_READ_VALUE;
        }

        let value = if high_byte {
            (self.vram_read_buffer >> 8) as u8
        } else {
            self.vram_read_buffer as u8
        };
        if high_byte {
            self.prefetch_vram();
        }
        value
    }

    fn write_data(&mut self, high_byte: bool, value: u8) -> VdcPortWriteResult {
        let Some(register) = self.selected_register() else {
            return VdcPortWriteResult::Applied;
        };

        self.write_register_byte(register, high_byte, value);
        match (register, high_byte) {
            (VdcRegister::MemoryAddressRead, true) => {
                self.prefetch_vram();
            }
            (VdcRegister::VramData, true) => self.commit_vram_write(),
            (VdcRegister::DmaLength, true) => {
                return VdcPortWriteResult::VramDma(self.queue_vram_dma_from_port());
            }
            (VdcRegister::SatbSource, true) => self.queue_satb_dma(),
            _ => {}
        }
        VdcPortWriteResult::Applied
    }

    #[inline]
    fn write_register_byte(&mut self, register: VdcRegister, high_byte: bool, value: u8) {
        let current = self.registers[register as usize];
        let updated = if high_byte {
            (current & 0x00FF) | (u16::from(value) << 8)
        } else {
            (current & 0xFF00) | u16::from(value)
        };
        self.registers[register as usize] = updated & register.writable_mask();
        if register == VdcRegister::BackgroundScrollY {
            self.scanline_state.mark_background_scroll_y_write();
        }
    }

    fn commit_vram_write(&mut self) {
        let address = self.register(VdcRegister::MemoryAddressWrite);
        let value = self.register(VdcRegister::VramData);
        self.write_logical_vram_word(address, value);
        self.increment_address(VdcRegister::MemoryAddressWrite);
    }

    fn prefetch_vram(&mut self) {
        let address = self.register(VdcRegister::MemoryAddressRead);
        self.vram_read_buffer = self.read_logical_vram_word(address);
        self.increment_address(VdcRegister::MemoryAddressRead);
    }

    #[inline]
    pub(super) fn read_logical_vram_word(&self, address: u16) -> u16 {
        self.vram[usize::from(address) & VDC_VRAM_WORD_ADDRESS_MASK]
    }

    #[inline]
    pub(super) fn write_logical_vram_word(&mut self, address: u16, value: u16) {
        if address & 0x8000 == 0 {
            self.vram[usize::from(address)] = value;
        }
    }

    #[inline]
    fn increment_address(&mut self, register: VdcRegister) {
        let increment = match (self.register(VdcRegister::Control) >> 11) & 3 {
            0 => 1,
            1 => 0x20,
            2 => 0x40,
            _ => 0x80,
        };
        self.registers[register as usize] = self.register(register).wrapping_add(increment);
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        for word in self.vram.iter().copied() {
            writer.write_u16(word);
        }
        for word in self.satb {
            writer.write_u16(word);
        }
        for register in self.registers {
            writer.write_u16(register);
        }
        writer.write_u8(self.selected_register_id);
        writer.write_u16(self.vram_read_buffer);
        writer.write_u8(self.status.bits());
        self.write_dma_state(writer);
        self.horizontal_state.write_state(writer);
        self.scanline_state.write_state(writer);
    }

    pub(super) fn read_state(&mut self, reader: &mut StateReader<'_>) -> anyhow::Result<()> {
        for word in self.vram.iter_mut() {
            *word = reader.read_u16()?;
        }
        for word in &mut self.satb {
            *word = reader.read_u16()?;
        }
        for (id, register) in self.registers.iter_mut().enumerate() {
            let value = reader.read_u16()?;
            match VdcRegister::from_id(id as u8) {
                Some(register_id) if value & !register_id.writable_mask() != 0 => {
                    bail!("invalid VDC register {id:#04X} in save-state: {value:#06X}");
                }
                None if value != 0 => {
                    bail!("nonzero unavailable VDC register {id:#04X} in save-state");
                }
                _ => {}
            }
            *register = value;
        }
        self.selected_register_id = reader.read_u8()?;
        if self.selected_register_id > 0x1F {
            bail!(
                "invalid VDC selected-register id in save-state: {}",
                self.selected_register_id
            );
        }
        self.vram_read_buffer = reader.read_u16()?;
        let status = reader.read_u8()?;
        let Some(status) = VdcStatus::from_bits(status) else {
            bail!("invalid VDC status bits in save-state: {status:#04X}");
        };
        self.status = status;
        self.read_dma_state(reader)?;
        self.horizontal_state = VdcHorizontalState::read_state(reader)?;
        self.scanline_state = VdcScanlineState::read_state(reader)?;
        Ok(())
    }
}

impl BaseBusDevices for HuC6270 {
    #[inline]
    fn read_vdc(&mut self, port: VdcPort) -> u8 {
        self.read_port(port)
    }

    #[inline]
    fn write_vdc(&mut self, port: VdcPort, value: u8) {
        let _ = self.write_port(port, value);
    }
}
