use super::vdc::{HuC6270, VdcRegister, VdcStatus};
use super::vdc_render::BackgroundRenderState;
use super::vdc_sprite_render::{SpriteRenderState, SpriteScanlineStatus};

const RASTER_COUNTER_MASK: u16 = 0x03FF;
const BACKGROUND_SCROLL_Y_MASK: u16 = 0x01FF;
const ACTIVE_DISPLAY_RASTER_START: u16 = 64;
pub const DETERMINISTIC_VDC_RESET_LATCHED_MEMORY_WIDTH: u16 = 0;
pub const PROVISIONAL_EXTERNAL_VCE_VERTICAL_PROFILE_LATCHED_AT_VSYNC: bool = true;
pub const PROVISIONAL_EXTERNAL_VDW_CAPPED_TO_VCE_FRAME: bool = true;
pub const PROVISIONAL_EXTERNAL_VSYNC_MARKER_RESTARTS_VERTICAL_PROGRESSION: bool = true;
pub const PROVISIONAL_STOCK_MACHINE_VCE_BOUNDARIES_DRIVE_VDC_HORIZONTAL_AND_VERTICAL_SYNC: bool =
    true;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdcSyncMode {
    ExternalHorizontalAndVertical,
    ExternalVertical,
    Invalid,
    Internal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdcVerticalPhase {
    VerticalSync,
    DisplayStart,
    ActiveDisplay,
    DisplayEnd,
}

impl VdcVerticalPhase {
    #[inline]
    const fn next(self) -> Self {
        match self {
            Self::VerticalSync => Self::DisplayStart,
            Self::DisplayStart => Self::ActiveDisplay,
            Self::ActiveDisplay => Self::DisplayEnd,
            Self::DisplayEnd => Self::VerticalSync,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdcScanlineAdvanceError {
    NonAutonomousSync {
        mode: VdcSyncMode,
    },
    ExternalVceSyncNeedsHorizontalScheduler,
    InvalidSyncMode,
    InternalSyncUsesAutonomousAdvance,
    InvalidExternalBoundaryCount {
        count: u8,
    },
    ExternalProfileNotStarted,
    ExternalVerticalBlankUnavailable {
        frame_lines: u16,
        vertical_sync: u16,
        display_start: u16,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VceFrameLength {
    Lines262,
    Lines263,
}

impl VceFrameLength {
    #[inline]
    pub const fn scanlines(self) -> u16 {
        match self {
            Self::Lines262 => 262,
            Self::Lines263 => 263,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdcExternalVceScanline {
    boundary_count: u8,
    vsync_started: bool,
    frame_length: VceFrameLength,
}

impl VdcExternalVceScanline {
    #[inline]
    pub const fn new(
        boundary_count: u8,
        vsync_started: bool,
        frame_length: VceFrameLength,
    ) -> Self {
        Self {
            boundary_count,
            vsync_started,
            frame_length,
        }
    }

    #[inline]
    pub const fn boundary_count(self) -> u8 {
        self.boundary_count
    }

    #[inline]
    pub const fn vsync_started(self) -> bool {
        self.vsync_started
    }

    #[inline]
    pub const fn frame_length(self) -> VceFrameLength {
        self.frame_length
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdcScanlineTransition {
    VerticalBlankStarted,
    PhaseStarted(VdcVerticalPhase),
    FrameStarted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdcActiveDisplayLine {
    display_line: u16,
    source_start: u16,
    source_width: u16,
    background: BackgroundRenderState,
    sprites: SpriteRenderState,
    sprite_collision_enabled: bool,
    sprite_overflow_enabled: bool,
}

impl VdcActiveDisplayLine {
    #[inline]
    pub const fn display_line(self) -> u16 {
        self.display_line
    }

    #[inline]
    pub const fn source_width(self) -> u16 {
        self.source_width
    }

    #[inline]
    pub const fn source_start(self) -> u16 {
        self.source_start
    }

    #[inline]
    pub const fn background(self) -> BackgroundRenderState {
        self.background
    }

    #[inline]
    pub const fn sprites(self) -> SpriteRenderState {
        self.sprites
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdcScanlineBoundary {
    phase: VdcVerticalPhase,
    entered_phase: Option<VdcVerticalPhase>,
    frame_started: bool,
    frame_line: u16,
    phase_line: u16,
    active_display: Option<VdcActiveDisplayLine>,
    raster_counter: u16,
    raster_match: bool,
    vertical_blank_started: bool,
    satb_dma_started: bool,
    vram_dma_aborted: bool,
    transitions: [Option<VdcScanlineTransition>; 3],
}

impl VdcScanlineBoundary {
    #[inline]
    pub const fn phase(self) -> VdcVerticalPhase {
        self.phase
    }

    #[inline]
    pub const fn entered_phase(self) -> Option<VdcVerticalPhase> {
        self.entered_phase
    }

    #[inline]
    pub const fn frame_started(self) -> bool {
        self.frame_started
    }

    #[inline]
    pub const fn frame_line(self) -> u16 {
        self.frame_line
    }

    #[inline]
    pub const fn phase_line(self) -> u16 {
        self.phase_line
    }

    #[inline]
    pub const fn active_display_line(self) -> Option<u16> {
        match self.active_display {
            Some(display) => Some(display.display_line),
            None => None,
        }
    }

    #[inline]
    pub const fn active_display(self) -> Option<VdcActiveDisplayLine> {
        self.active_display
    }

    #[inline]
    pub const fn raster_counter(self) -> u16 {
        self.raster_counter
    }

    #[inline]
    pub const fn raster_match(self) -> bool {
        self.raster_match
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
    pub const fn vram_dma_aborted(self) -> bool {
        self.vram_dma_aborted
    }

    #[inline]
    pub const fn transitions(self) -> [Option<VdcScanlineTransition>; 3] {
        self.transitions
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct VdcScanlineState {
    phase: VdcVerticalPhase,
    phase_line: u16,
    phase_duration: u16,
    frame_line: u16,
    raster_counter: u16,
    vertical_blank_pending: bool,
    latched_memory_width: u16,
    effective_background_scroll_y: u16,
    background_scroll_y_reload_pending: bool,
    external_profile: Option<ExternalVerticalProfile>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExternalVerticalProfile {
    vertical_sync: u16,
    display_start: u16,
    active_display: u16,
    display_end: u16,
}

impl ExternalVerticalProfile {
    #[inline]
    const fn phase_duration(self, phase: VdcVerticalPhase) -> u16 {
        match phase {
            VdcVerticalPhase::VerticalSync => self.vertical_sync,
            VdcVerticalPhase::DisplayStart => self.display_start,
            VdcVerticalPhase::ActiveDisplay => self.active_display,
            VdcVerticalPhase::DisplayEnd => self.display_end,
        }
    }
}

impl Default for VdcScanlineState {
    fn default() -> Self {
        Self {
            phase: VdcVerticalPhase::VerticalSync,
            phase_line: 0,
            phase_duration: 0,
            frame_line: 0,
            raster_counter: 0,
            vertical_blank_pending: false,
            latched_memory_width: DETERMINISTIC_VDC_RESET_LATCHED_MEMORY_WIDTH,
            effective_background_scroll_y: 0,
            background_scroll_y_reload_pending: false,
            external_profile: None,
        }
    }
}

impl VdcScanlineState {
    #[inline]
    pub(super) fn mark_background_scroll_y_write(&mut self) {
        self.background_scroll_y_reload_pending = true;
    }

    #[inline]
    pub(super) const fn current_phase(&self) -> VdcVerticalPhase {
        self.phase
    }
}

impl HuC6270 {
    #[inline]
    pub fn sync_mode(&self) -> VdcSyncMode {
        match (self.register(VdcRegister::Control) >> 4) & 3 {
            0 => VdcSyncMode::ExternalHorizontalAndVertical,
            1 => VdcSyncMode::ExternalVertical,
            2 => VdcSyncMode::Invalid,
            _ => VdcSyncMode::Internal,
        }
    }

    pub fn advance_scanline_boundary(
        &mut self,
    ) -> Result<VdcScanlineBoundary, VdcScanlineAdvanceError> {
        let sync_mode = self.sync_mode();
        if sync_mode != VdcSyncMode::Internal {
            return Err(VdcScanlineAdvanceError::NonAutonomousSync { mode: sync_mode });
        }

        self.scanline_state.external_profile = None;
        Ok(self.advance_vertical_boundary(None))
    }

    pub fn advance_external_vce_scanline(
        &mut self,
        input: VdcExternalVceScanline,
    ) -> Result<VdcScanlineBoundary, VdcScanlineAdvanceError> {
        match self.sync_mode() {
            VdcSyncMode::ExternalHorizontalAndVertical => {}
            VdcSyncMode::ExternalVertical => {
                return Err(VdcScanlineAdvanceError::ExternalVceSyncNeedsHorizontalScheduler);
            }
            VdcSyncMode::Invalid => return Err(VdcScanlineAdvanceError::InvalidSyncMode),
            VdcSyncMode::Internal => {
                return Err(VdcScanlineAdvanceError::InternalSyncUsesAutonomousAdvance);
            }
        }
        self.advance_vce_scanline_core(input)
    }

    pub(crate) fn advance_machine_vce_scanline(
        &mut self,
        input: VdcExternalVceScanline,
    ) -> Result<VdcScanlineBoundary, VdcScanlineAdvanceError> {
        self.validate_machine_vce_scanline(input)?;
        self.advance_vce_scanline_core(input)
    }

    pub(crate) fn validate_machine_vce_scanline(
        &self,
        input: VdcExternalVceScanline,
    ) -> Result<(), VdcScanlineAdvanceError> {
        match self.sync_mode() {
            VdcSyncMode::ExternalHorizontalAndVertical | VdcSyncMode::Internal => {}
            VdcSyncMode::ExternalVertical => {
                return Err(VdcScanlineAdvanceError::ExternalVceSyncNeedsHorizontalScheduler);
            }
            VdcSyncMode::Invalid => return Err(VdcScanlineAdvanceError::InvalidSyncMode),
        }
        if input.boundary_count != 1 {
            return Err(VdcScanlineAdvanceError::InvalidExternalBoundaryCount {
                count: input.boundary_count,
            });
        }
        if input.vsync_started {
            self.capture_external_profile(input.frame_length)?;
        } else if self.scanline_state.external_profile.is_none() {
            return Err(VdcScanlineAdvanceError::ExternalProfileNotStarted);
        }
        Ok(())
    }

    fn advance_vce_scanline_core(
        &mut self,
        input: VdcExternalVceScanline,
    ) -> Result<VdcScanlineBoundary, VdcScanlineAdvanceError> {
        if input.boundary_count != 1 {
            return Err(VdcScanlineAdvanceError::InvalidExternalBoundaryCount {
                count: input.boundary_count,
            });
        }
        let profile = if input.vsync_started {
            let profile = self.capture_external_profile(input.frame_length)?;
            self.scanline_state.phase = VdcVerticalPhase::VerticalSync;
            self.scanline_state.phase_line = 0;
            self.scanline_state.phase_duration = profile.vertical_sync;
            self.scanline_state.frame_line = 0;
            self.scanline_state.external_profile = Some(profile);
            profile
        } else {
            let Some(profile) = self.scanline_state.external_profile else {
                return Err(VdcScanlineAdvanceError::ExternalProfileNotStarted);
            };
            profile
        };

        Ok(self.advance_vertical_boundary(Some(profile)))
    }

    fn advance_vertical_boundary(
        &mut self,
        external_profile: Option<ExternalVerticalProfile>,
    ) -> VdcScanlineBoundary {
        if self.scanline_state.phase_duration == 0 {
            self.scanline_state.phase_duration =
                self.vertical_phase_duration(self.scanline_state.phase, external_profile);
        }

        let phase = self.scanline_state.phase;
        let phase_line = self.scanline_state.phase_line;
        let phase_started = phase_line == 0;
        if phase == VdcVerticalPhase::ActiveDisplay && phase_started {
            self.scanline_state.raster_counter = ACTIVE_DISPLAY_RASTER_START;
        }
        let vertical_blank_started = self.scanline_state.vertical_blank_pending;
        self.scanline_state.vertical_blank_pending = false;
        let raster_counter = self.scanline_state.raster_counter;
        let raster_match = raster_counter == self.register(VdcRegister::RasterCounter);
        let control = self.register(VdcRegister::Control);

        if vertical_blank_started {
            self.scanline_state.latched_memory_width = self.register(VdcRegister::MemoryWidth);
            self.latch_frame_burst_for_next_frame();
        }
        if raster_match && control & 0x04 != 0 {
            self.latch_status(VdcStatus::RASTER_MATCH);
        }
        if vertical_blank_started && control & 0x08 != 0 {
            self.latch_status(VdcStatus::VERTICAL_BLANK);
        }

        let satb_dma_started = vertical_blank_started && self.start_satb_dma_for_vertical_blank();
        let vram_dma_aborted = phase == VdcVerticalPhase::ActiveDisplay
            && phase_started
            && self.should_abort_vram_dma_for_active_display()
            && self.abort_vram_dma_for_active_display();
        let active_display = if phase == VdcVerticalPhase::ActiveDisplay {
            let background_scroll_y = self.background_scroll_y_for_active_line(phase_line);
            let active_control = self.active_line_control(control);
            Some(VdcActiveDisplayLine {
                display_line: phase_line,
                source_start: self.active_line_source_start(),
                source_width: self.active_line_source_width(),
                background: BackgroundRenderState::from_register_values(
                    active_control,
                    self.scanline_state.latched_memory_width,
                    self.register(VdcRegister::BackgroundScrollX),
                    background_scroll_y,
                ),
                sprites: SpriteRenderState::from_register_values(
                    active_control,
                    self.scanline_state.latched_memory_width,
                ),
                sprite_collision_enabled: control & 0x01 != 0,
                sprite_overflow_enabled: control & 0x02 != 0,
            })
        } else {
            None
        };
        let mut transitions = [None; 3];
        let mut transition_count = 0;
        if vertical_blank_started {
            transitions[transition_count] = Some(VdcScanlineTransition::VerticalBlankStarted);
            transition_count += 1;
        }
        if phase_started {
            transitions[transition_count] = Some(VdcScanlineTransition::PhaseStarted(phase));
            transition_count += 1;
        }
        if phase == VdcVerticalPhase::VerticalSync && phase_started {
            transitions[transition_count] = Some(VdcScanlineTransition::FrameStarted);
        }
        let boundary = VdcScanlineBoundary {
            phase,
            entered_phase: phase_started.then_some(phase),
            frame_started: phase == VdcVerticalPhase::VerticalSync && phase_started,
            frame_line: self.scanline_state.frame_line,
            phase_line,
            active_display,
            raster_counter,
            raster_match,
            vertical_blank_started,
            satb_dma_started,
            vram_dma_aborted,
            transitions,
        };
        self.advance_vertical_state(external_profile);
        boundary
    }

    pub(super) fn latch_full_active_span_sprite_status(
        &mut self,
        display: VdcActiveDisplayLine,
        status: SpriteScanlineStatus,
    ) {
        let SpriteScanlineStatus::Rendered(events) = status else {
            return;
        };
        let mut latched = VdcStatus::empty();
        if display.sprite_collision_enabled && events.collision_within_output() {
            latched |= VdcStatus::SPRITE_COLLISION;
        }
        if display.sprite_overflow_enabled && events.overflow() {
            latched |= VdcStatus::SPRITE_OVERFLOW;
        }
        self.latch_status(latched);
    }

    fn advance_vertical_state(&mut self, external_profile: Option<ExternalVerticalProfile>) {
        let mut state = self.scanline_state;
        state.raster_counter = state.raster_counter.wrapping_add(1) & RASTER_COUNTER_MASK;
        if state.phase_line + 1 < state.phase_duration {
            state.phase_line += 1;
            state.frame_line = state.frame_line.wrapping_add(1);
            self.scanline_state = state;
            return;
        }

        let previous = state.phase;
        let mut next = previous.next();
        state.phase_line = 0;
        state.frame_line = if next == VdcVerticalPhase::VerticalSync {
            0
        } else {
            state.frame_line.wrapping_add(1)
        };
        if previous == VdcVerticalPhase::ActiveDisplay {
            state.vertical_blank_pending = true;
        }

        let mut duration = self.vertical_phase_duration(next, external_profile);
        if next == VdcVerticalPhase::DisplayEnd && duration == 0 {
            next = VdcVerticalPhase::VerticalSync;
            duration = self.vertical_phase_duration(next, external_profile);
            state.frame_line = 0;
        }
        state.phase = next;
        state.phase_duration = duration;
        if next == VdcVerticalPhase::ActiveDisplay {
            state.raster_counter = ACTIVE_DISPLAY_RASTER_START;
        }
        self.scanline_state = state;
    }

    fn background_scroll_y_for_active_line(&mut self, display_line: u16) -> u16 {
        let raw = self.register(VdcRegister::BackgroundScrollY);
        if display_line == 0 {
            self.scanline_state.effective_background_scroll_y = raw;
            self.scanline_state.background_scroll_y_reload_pending = false;
        } else if self.scanline_state.background_scroll_y_reload_pending {
            self.scanline_state.effective_background_scroll_y =
                raw.wrapping_add(1) & BACKGROUND_SCROLL_Y_MASK;
            self.scanline_state.background_scroll_y_reload_pending = false;
        } else {
            self.scanline_state.effective_background_scroll_y = self
                .scanline_state
                .effective_background_scroll_y
                .wrapping_add(1)
                & BACKGROUND_SCROLL_Y_MASK;
        }

        self.scanline_state
            .effective_background_scroll_y
            .wrapping_sub(display_line)
            & BACKGROUND_SCROLL_Y_MASK
    }

    #[inline]
    fn phase_duration(&self, phase: VdcVerticalPhase) -> u16 {
        match phase {
            VdcVerticalPhase::VerticalSync => (self.register(VdcRegister::VerticalSync) & 0x1F) + 1,
            VdcVerticalPhase::DisplayStart => (self.register(VdcRegister::VerticalSync) >> 8) + 2,
            VdcVerticalPhase::ActiveDisplay => {
                (self.register(VdcRegister::VerticalDisplay) & 0x01FF) + 1
            }
            VdcVerticalPhase::DisplayEnd => self.register(VdcRegister::VerticalDisplayEnd) & 0x00FF,
        }
    }

    #[inline]
    fn vertical_phase_duration(
        &self,
        phase: VdcVerticalPhase,
        external_profile: Option<ExternalVerticalProfile>,
    ) -> u16 {
        external_profile.map_or_else(
            || self.phase_duration(phase),
            |profile| profile.phase_duration(phase),
        )
    }

    fn capture_external_profile(
        &self,
        frame_length: VceFrameLength,
    ) -> Result<ExternalVerticalProfile, VdcScanlineAdvanceError> {
        let vertical_sync = self.phase_duration(VdcVerticalPhase::VerticalSync);
        let display_start = self.phase_duration(VdcVerticalPhase::DisplayStart);
        let frame_lines = frame_length.scanlines();
        let non_active_prefix = vertical_sync + display_start;
        let Some(max_active_display) = frame_lines
            .checked_sub(1)
            .and_then(|lines| lines.checked_sub(non_active_prefix))
            .filter(|&lines| lines != 0)
        else {
            return Err(VdcScanlineAdvanceError::ExternalVerticalBlankUnavailable {
                frame_lines,
                vertical_sync,
                display_start,
            });
        };

        Ok(ExternalVerticalProfile {
            vertical_sync,
            display_start,
            active_display: self
                .phase_duration(VdcVerticalPhase::ActiveDisplay)
                .min(max_active_display),
            display_end: self.phase_duration(VdcVerticalPhase::DisplayEnd),
        })
    }
}
