use super::vdc::{HuC6270, VdcDmaChannel, VdcDmaError, VdcDmaProgress, VdcRegister};
use super::vdc_scanline::VdcVerticalPhase;
use anyhow::bail;
use zeff_emu_common::save_state::{StateReader, StateWriter};

pub const VDC_DMA_PIXELS_PER_WORD: u8 = 4;
pub const DETERMINISTIC_VDC_RESET_FRAME_BURST: bool = false;
pub const PROVISIONAL_VDC_DMA_SATB_FIRST: bool = true;
pub const PROVISIONAL_VDC_REJECTS_ACTIVE_NONBURST_DMA_TRIGGER: bool = false;
pub const PROVISIONAL_VDC_REJECTS_DMA_TRIGGER_WHILE_ACTIVE: bool = true;
pub const PROVISIONAL_VCE_CLOCK_DIVIDER_PRESERVES_MASTER_PHASE: bool = true;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdcHorizontalPhase {
    DisplayStart,
    ActiveDisplay,
    DisplayEnd,
    Sync,
}

impl VdcHorizontalPhase {
    #[inline]
    const fn next(self) -> Self {
        match self {
            Self::DisplayStart => Self::ActiveDisplay,
            Self::ActiveDisplay => Self::DisplayEnd,
            Self::DisplayEnd => Self::Sync,
            Self::Sync => Self::DisplayStart,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdcVramDmaTriggerResult {
    Queued,
    RejectedOutsideTransferWindow,
    RejectedWhilePending,
    RejectedWhileActive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdcPortWriteResult {
    Applied,
    VramDma(VdcVramDmaTriggerResult),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VdcHorizontalAdvance {
    pixel_clocks: u64,
    phase_transitions: u64,
    vertical_blank_started: bool,
    satb_dma_started: bool,
    dma_slots: u64,
    satb_words: u64,
    vram_words: u64,
    dma_completions: u64,
}

impl VdcHorizontalAdvance {
    #[inline]
    pub const fn pixel_clocks(self) -> u64 {
        self.pixel_clocks
    }

    #[inline]
    pub const fn phase_transitions(self) -> u64 {
        self.phase_transitions
    }

    #[inline]
    pub const fn vertical_blank_started(self) -> bool {
        self.vertical_blank_started
    }

    #[inline]
    pub const fn satb_dma_started(self) -> bool {
        self.satb_dma_started
    }

    #[inline]
    pub const fn dma_slots(self) -> u64 {
        self.dma_slots
    }

    #[inline]
    pub const fn satb_words(self) -> u64 {
        self.satb_words
    }

    #[inline]
    pub const fn vram_words(self) -> u64 {
        self.vram_words
    }

    #[inline]
    pub const fn dma_completions(self) -> u64 {
        self.dma_completions
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct VdcHorizontalState {
    phase: VdcHorizontalPhase,
    phase_pixels_remaining: u16,
    dma_pixel_remainder: u8,
    frame_burst: bool,
    latched_display_start_pixels: u16,
    latched_display_control: u16,
    latched_active_display_pixels: u16,
    display_control_latched_for_line: bool,
}

impl Default for VdcHorizontalState {
    fn default() -> Self {
        Self {
            phase: VdcHorizontalPhase::DisplayStart,
            phase_pixels_remaining: 8,
            dma_pixel_remainder: 0,
            frame_burst: DETERMINISTIC_VDC_RESET_FRAME_BURST,
            latched_display_start_pixels: 8,
            latched_display_control: 0,
            latched_active_display_pixels: 8,
            display_control_latched_for_line: false,
        }
    }
}

impl VdcHorizontalState {
    pub(super) fn write_state(self, writer: &mut StateWriter) {
        writer.write_u8(match self.phase {
            VdcHorizontalPhase::DisplayStart => 0,
            VdcHorizontalPhase::ActiveDisplay => 1,
            VdcHorizontalPhase::DisplayEnd => 2,
            VdcHorizontalPhase::Sync => 3,
        });
        writer.write_u16(self.phase_pixels_remaining);
        writer.write_u8(self.dma_pixel_remainder);
        writer.write_bool(self.frame_burst);
        writer.write_u16(self.latched_display_start_pixels);
        writer.write_u16(self.latched_display_control);
        writer.write_u16(self.latched_active_display_pixels);
        writer.write_bool(self.display_control_latched_for_line);
    }

    pub(super) fn read_state(reader: &mut StateReader<'_>) -> anyhow::Result<Self> {
        let phase = match reader.read_u8()? {
            0 => VdcHorizontalPhase::DisplayStart,
            1 => VdcHorizontalPhase::ActiveDisplay,
            2 => VdcHorizontalPhase::DisplayEnd,
            3 => VdcHorizontalPhase::Sync,
            tag => bail!("invalid VDC horizontal-phase tag in save-state: {tag}"),
        };
        let phase_pixels_remaining = reader.read_u16()?;
        if phase_pixels_remaining == 0 {
            bail!("VDC horizontal phase has no remaining pixels in save-state");
        }
        let dma_pixel_remainder = reader.read_u8()?;
        if dma_pixel_remainder >= VDC_DMA_PIXELS_PER_WORD {
            bail!("invalid VDC DMA pixel remainder in save-state: {dma_pixel_remainder}");
        }
        Ok(Self {
            phase,
            phase_pixels_remaining,
            dma_pixel_remainder,
            frame_burst: reader.read_bool()?,
            latched_display_start_pixels: reader.read_u16()?,
            latched_display_control: reader.read_u16()?,
            latched_active_display_pixels: reader.read_u16()?,
            display_control_latched_for_line: reader.read_bool()?,
        })
    }
}

impl HuC6270 {
    #[inline]
    pub const fn horizontal_phase(&self) -> VdcHorizontalPhase {
        self.horizontal_state.phase
    }

    #[inline]
    pub const fn horizontal_phase_pixels_remaining(&self) -> u16 {
        self.horizontal_state.phase_pixels_remaining
    }

    #[inline]
    pub const fn dma_pixel_remainder(&self) -> u8 {
        self.horizontal_state.dma_pixel_remainder
    }

    #[inline]
    pub const fn frame_burst_enabled(&self) -> bool {
        self.horizontal_state.frame_burst
    }

    pub fn begin_external_horizontal_line(&mut self) {
        self.horizontal_state.phase = VdcHorizontalPhase::DisplayStart;
        self.horizontal_state.phase_pixels_remaining =
            self.horizontal_phase_pixels(VdcHorizontalPhase::DisplayStart);
        self.horizontal_state.latched_display_start_pixels =
            self.horizontal_state.phase_pixels_remaining;
        self.horizontal_state.display_control_latched_for_line = false;
    }

    pub fn advance_horizontal_pixels(
        &mut self,
        mut pixel_clocks: u64,
    ) -> Result<VdcHorizontalAdvance, VdcDmaError> {
        let mut advance = VdcHorizontalAdvance {
            pixel_clocks,
            ..VdcHorizontalAdvance::default()
        };
        while pixel_clocks != 0 {
            let pixels_to_dma =
                u64::from(VDC_DMA_PIXELS_PER_WORD - self.horizontal_state.dma_pixel_remainder);
            let pixels_to_raster_event = self.pixels_to_raster_event().unwrap_or(u16::MAX);
            let phase_pixels_before = self.horizontal_state.phase_pixels_remaining;
            let elapsed = pixel_clocks
                .min(u64::from(self.horizontal_state.phase_pixels_remaining))
                .min(pixels_to_dma)
                .min(u64::from(pixels_to_raster_event));
            pixel_clocks -= elapsed;
            self.horizontal_state.phase_pixels_remaining -= elapsed as u16;
            self.horizontal_state.dma_pixel_remainder += elapsed as u8;

            if self.horizontal_state.phase == VdcHorizontalPhase::ActiveDisplay
                && phase_pixels_before > self.raster_event_remaining_pixels()
                && self.horizontal_state.phase_pixels_remaining
                    == self.raster_event_remaining_pixels()
            {
                self.latch_horizontal_raster_match();
            }

            if self.horizontal_state.phase_pixels_remaining == 0 {
                if let Some(satb_dma_started) = self.enter_next_horizontal_phase() {
                    advance.vertical_blank_started = true;
                    advance.satb_dma_started |= satb_dma_started;
                }
                advance.phase_transitions += 1;
            }
            if self.horizontal_state.dma_pixel_remainder == VDC_DMA_PIXELS_PER_WORD {
                self.horizontal_state.dma_pixel_remainder = 0;
                advance.dma_slots += 1;
                self.service_scheduled_dma_slot(&mut advance)?;
            }
        }
        Ok(advance)
    }

    fn enter_next_horizontal_phase(&mut self) -> Option<bool> {
        let next = self.horizontal_state.phase.next();
        self.horizontal_state.phase = next;
        self.horizontal_state.phase_pixels_remaining = self.horizontal_phase_pixels(next);
        if next == VdcHorizontalPhase::DisplayStart {
            self.horizontal_state.latched_display_start_pixels =
                self.horizontal_state.phase_pixels_remaining;
        }
        if next == VdcHorizontalPhase::ActiveDisplay {
            self.horizontal_state.latched_display_control =
                self.register(VdcRegister::Control) & 0x00C0;
            self.horizontal_state.latched_active_display_pixels =
                self.horizontal_state.phase_pixels_remaining;
            self.horizontal_state.display_control_latched_for_line = true;
            if self.horizontal_state.latched_active_display_pixels == 8 {
                self.latch_horizontal_raster_match();
            }
            return self.start_pending_vertical_blank();
        }
        None
    }

    fn pixels_to_raster_event(&self) -> Option<u16> {
        if self.horizontal_state.phase != VdcHorizontalPhase::ActiveDisplay {
            return None;
        }
        self.horizontal_state
            .phase_pixels_remaining
            .checked_sub(self.raster_event_remaining_pixels())
            .filter(|&pixels| pixels != 0)
    }

    fn raster_event_remaining_pixels(&self) -> u16 {
        if self.horizontal_state.latched_active_display_pixels == 8 {
            8
        } else {
            14
        }
    }

    pub(super) fn horizontal_raster_event_reached(&self) -> bool {
        match self.horizontal_state.phase {
            VdcHorizontalPhase::DisplayStart => {
                self.horizontal_state.display_control_latched_for_line
            }
            VdcHorizontalPhase::ActiveDisplay => {
                self.horizontal_state.phase_pixels_remaining <= self.raster_event_remaining_pixels()
            }
            VdcHorizontalPhase::DisplayEnd | VdcHorizontalPhase::Sync => true,
        }
    }

    fn service_scheduled_dma_slot(
        &mut self,
        advance: &mut VdcHorizontalAdvance,
    ) -> Result<(), VdcDmaError> {
        let channel = if self.active_satb_dma().is_some() {
            Some(VdcDmaChannel::Satb)
        } else if self.vram_transfer_window_open() {
            self.activate_pending_vram_dma();
            self.active_vram_dma().map(|_| VdcDmaChannel::Vram)
        } else {
            None
        };
        let Some(channel) = channel else {
            return Ok(());
        };
        match self.service_dma_slot(channel)? {
            VdcDmaProgress::Idle => {}
            progress @ (VdcDmaProgress::Transferred { .. } | VdcDmaProgress::Complete) => {
                match channel {
                    VdcDmaChannel::Satb => advance.satb_words += 1,
                    VdcDmaChannel::Vram => advance.vram_words += 1,
                }
                if progress == VdcDmaProgress::Complete {
                    advance.dma_completions += 1;
                }
            }
        }
        Ok(())
    }

    #[inline]
    fn horizontal_phase_pixels(&self, phase: VdcHorizontalPhase) -> u16 {
        let field = match phase {
            VdcHorizontalPhase::DisplayStart => {
                (self.register(VdcRegister::HorizontalSync) >> 8) & 0x7F
            }
            VdcHorizontalPhase::ActiveDisplay => {
                self.register(VdcRegister::HorizontalDisplay) & 0x7F
            }
            VdcHorizontalPhase::DisplayEnd => {
                (self.register(VdcRegister::HorizontalDisplay) >> 8) & 0x7F
            }
            VdcHorizontalPhase::Sync => self.register(VdcRegister::HorizontalSync) & 0x1F,
        };
        8 * (field + 1)
    }

    #[inline]
    pub(super) fn latch_frame_burst_for_next_frame(&mut self) {
        self.horizontal_state.frame_burst = self.register(VdcRegister::Control) & 0x00C0 == 0;
    }

    #[inline]
    pub(super) const fn active_line_control(&self, control: u16) -> u16 {
        if self.horizontal_state.display_control_latched_for_line {
            (control & !0x00C0) | self.horizontal_state.latched_display_control
        } else {
            control
        }
    }

    #[inline]
    pub(super) fn active_line_source_width(&self) -> u16 {
        if self.horizontal_state.display_control_latched_for_line {
            self.horizontal_state.latched_active_display_pixels
        } else {
            ((self.register(VdcRegister::HorizontalDisplay) & 0x7F) + 1) * 8
        }
    }

    #[inline]
    pub(super) const fn active_line_source_start(&self) -> u16 {
        self.horizontal_state.latched_display_start_pixels
    }

    #[inline]
    pub(super) fn vram_transfer_window_open(&self) -> bool {
        self.scanline_state.current_phase() != VdcVerticalPhase::ActiveDisplay
            || self.horizontal_state.frame_burst
    }

    pub(super) fn queue_vram_dma_from_port(&mut self) -> VdcVramDmaTriggerResult {
        if self.active_vram_dma().is_some() {
            return VdcVramDmaTriggerResult::RejectedWhileActive;
        }
        if self.pending_vram_dma().is_some() {
            return VdcVramDmaTriggerResult::RejectedWhilePending;
        }
        self.queue_vram_dma();
        VdcVramDmaTriggerResult::Queued
    }

    #[inline]
    pub(crate) fn dma_owns_vram_slots(&self) -> bool {
        self.active_satb_dma().is_some()
            || self.active_vram_dma().is_some()
            || (self.pending_vram_dma().is_some() && self.vram_transfer_window_open())
    }
}
