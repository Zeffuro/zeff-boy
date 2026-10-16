use super::*;
use crate::hardware::vdp::VdpDmaSource;
use crate::hardware::vdp::mode::{HorizontalMode, RasterGeometry};
use crate::hardware::vdp::render::horizontal_mode;
use crate::hardware::vdp::{VdpDmaError, VdpRenderError, VdpSpritePattern};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdpLineError {
    Schedule(VdpScheduleError),
    Render(VdpRenderError),
}

impl From<VdpScheduleError> for VdpLineError {
    fn from(error: VdpScheduleError) -> Self {
        Self::Schedule(error)
    }
}

impl From<VdpRenderError> for VdpLineError {
    fn from(error: VdpRenderError) -> Self {
        Self::Render(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VdpScanline {
    at: MasterTicks,
    row: u16,
    colors: [u16; H40_SCREEN_WIDTH],
    width: usize,
}

impl VdpScanline {
    pub fn at(&self) -> MasterTicks {
        self.at
    }

    pub fn row(&self) -> u16 {
        self.row
    }

    /// Packed RGB levels (0..14 per nibble); normal intensity matches CRAM encoding.
    pub fn colors(&self) -> &[u16] {
        &self.colors[..self.width]
    }
}

impl Vdp {
    pub(crate) fn prepare_raster_line(
        &self,
        row: u16,
        region: Region,
    ) -> Result<VdpScanline, VdpLineError> {
        self.state
            .interrupts
            .timing_next_edge(region, &self.state.registers)?;
        let mode = self.state.registers.registers()[usize::from(MODE_REGISTER)]
            .ok_or(VdpRenderError::UnknownRegister(MODE_REGISTER))?;
        if mode & VRAM_128K_ENABLE != 0 {
            return Err(VdpRenderError::UnsupportedRenderMode(mode).into());
        }
        compose_raster_line(&self.state, &self.memory, row, region)
    }

    pub(crate) fn advance_with_line_from(
        &mut self,
        at: MasterTicks,
        capture_at: MasterTicks,
        acknowledge: bool,
        region: Region,
        source: Option<VdpDmaSource<'_>>,
    ) -> Result<Result<VdpScanline, VdpLineError>, VdpScheduleError> {
        let (state, mut writes, _) =
            match self
                .state
                .prepare_to_from(at, region, acknowledge, &self.memory, source)
            {
                Err(VdpScheduleError::Dma(VdpDmaError::BatchFull)) => {
                    return self.advance_dma_with_line_from(
                        at,
                        capture_at,
                        acknowledge,
                        region,
                        source,
                    );
                }
                result => result?,
            };
        let captured =
            match self
                .state
                .prepare_to_from(capture_at, region, false, &self.memory, source)
            {
                Ok((prefix, prefix_writes, _)) => {
                    let count = prefix_writes.iter().flatten().count();
                    self.publish(prefix, prefix_writes);
                    writes[..count].fill(None);
                    self.prepare_line(region)
                }
                Err(error) => Err(error.into()),
            };
        self.publish(state, writes);
        Ok(captured)
    }

    /// HC-zero logical row snapshot, without physical fetch or output timing.
    pub fn prepare_line(&self, region: Region) -> Result<VdpScanline, VdpLineError> {
        prepare_line(&self.state, &self.memory, region)
    }

    /// Explicit logical row from current inputs, without physical fetch or output timing.
    pub fn prepare_logical_line(
        &self,
        row: u16,
        region: Region,
    ) -> Result<VdpScanline, VdpLineError> {
        self.state.validate_memory_configuration(region)?;
        self.state
            .interrupts
            .display_line(region, &self.state.registers)?;
        compose_line(&self.state, &self.memory, row, region)
    }

    /// Captures after publications through the requested tick; failure is atomic.
    pub fn capture_line_at(
        &mut self,
        at: MasterTicks,
        region: Region,
    ) -> Result<VdpScanline, VdpLineError> {
        let (state, writes, _) = match self.state.prepare_to(at, region, false, &self.memory) {
            Err(VdpScheduleError::Dma(VdpDmaError::BatchFull)) => {
                return self.capture_dma_line_at(at, region);
            }
            result => result?,
        };
        let mut undo = [None; WRITE_BATCH_CAPACITY];
        for (slot, (entry, phase)) in writes.into_iter().flatten().enumerate() {
            undo[slot] = Some(self.memory.publish_transfer(entry, phase));
        }
        let result = prepare_line(&state, &self.memory, region);
        if result.is_ok() {
            self.state = state;
        } else {
            for write in undo.into_iter().rev().flatten() {
                self.memory.undo_transfer(write);
            }
        }
        result
    }
}

fn prepare_line(
    state: &State,
    memory: &VdpMemory,
    region: Region,
) -> Result<VdpScanline, VdpLineError> {
    state.validate_memory_mode(region)?;
    let row = state.interrupts.display_line(region, &state.registers)?;
    compose_line(state, memory, row, region)
}

fn compose_line(
    state: &State,
    memory: &VdpMemory,
    row: u16,
    region: Region,
) -> Result<VdpScanline, VdpLineError> {
    if usize::from(row) >= state.registers.display_height(region) {
        return Err(VdpRenderError::CoordinateOutOfBounds { x: 0, y: row }.into());
    }
    if !state.fifo.is_empty() || state.read_phase.is_some() || state.dma.is_some() {
        return Err(VdpRenderError::PendingLineInputs.into());
    }
    let mode = horizontal_mode(&state.registers)?;
    let field = state
        .interrupts
        .field_position(region, &state.registers)
        .map_err(|error| VdpScheduleError::Interrupt(error.into()))?;
    let geometry = RasterGeometry::new(row, field.active_interlaced, field.active_odd);
    let selected = state
        .sprites
        .select_complete_row_with_geometry(geometry, mode)?;
    let sprites = if selected.indices().is_empty() {
        memory.compose_sprite_line_in_mode(&[], mode)?
    } else {
        let prepare = |slot| -> Result<VdpSpritePattern, VdpRenderError> {
            selected
                .prepare_sprite(&state.sprites, memory, &state.registers, slot)?
                .ok_or(VdpRenderError::UnsupportedSpriteRow)
        };
        let mut patterns = [prepare(0)?; H40_SPRITES_PER_LINE as usize];
        for (slot, pattern) in patterns
            .iter_mut()
            .enumerate()
            .take(selected.indices().len())
            .skip(1)
        {
            *pattern = prepare(slot as u8)?;
        }
        memory.compose_sprite_line_in_mode(&patterns[..selected.indices().len()], mode)?
    };
    let mut colors = [0; H40_SCREEN_WIDTH];
    for (x, color) in colors[..mode.width()].iter_mut().enumerate() {
        *color = memory.sample_sprite_line_with_geometry(
            &state.registers,
            &sprites,
            x as u16,
            geometry,
        )?;
    }
    Ok(VdpScanline {
        at: state.now(),
        row,
        colors,
        width: mode.width(),
    })
}

fn compose_raster_line(
    state: &State,
    memory: &VdpMemory,
    row: u16,
    region: Region,
) -> Result<VdpScanline, VdpLineError> {
    if usize::from(row) >= state.registers.display_height(region) {
        return Err(VdpRenderError::CoordinateOutOfBounds { x: 0, y: row }.into());
    }
    let mode = horizontal_mode(&state.registers)?;
    let mut line = VdpScanline {
        at: state.now(),
        row,
        colors: [0; H40_SCREEN_WIDTH],
        width: mode.width(),
    };
    let enabled = state.registers.registers()[usize::from(MODE_REGISTER)]
        .ok_or(VdpRenderError::UnknownRegister(MODE_REGISTER))?
        & DISPLAY_ENABLE
        != 0;
    if !enabled {
        let backdrop = state.registers.registers()[usize::from(BACKDROP_REGISTER)]
            .ok_or(VdpRenderError::UnknownRegister(BACKDROP_REGISTER))?;
        line.colors[..line.width].fill(memory.cram()[usize::from(backdrop & BACKDROP_COLOR_MASK)]);
        return Ok(line);
    }
    let field = state
        .interrupts
        .field_position(region, &state.registers)
        .map_err(|error| VdpScheduleError::Interrupt(error.into()))?;
    let geometry = RasterGeometry::new(row, field.active_interlaced, field.active_odd);
    let sprites = raster_sprites(state, memory, geometry, mode)?;
    memory.render_raster_sprite_line_with_geometry(
        &state.registers,
        &sprites,
        &mut line.colors[..line.width],
        geometry,
    )?;
    Ok(line)
}

fn raster_sprites(
    state: &State,
    memory: &VdpMemory,
    geometry: RasterGeometry,
    mode: HorizontalMode,
) -> Result<crate::hardware::vdp::VdpSpriteLine, VdpRenderError> {
    let mut patterns = [None; H40_SPRITES_PER_LINE as usize];
    let mut len = 0;
    let mut selected = 0;
    let mut index = 0;
    let mut visited = 0u128;
    let mut unmasked = false;
    loop {
        if index >= mode.sprite_count() || visited & (1u128 << index) != 0 {
            break;
        }
        visited |= 1u128 << index;
        let sprite = state.sprites.raster_sprite_entry_with_geometry(
            memory,
            &state.registers,
            index,
            geometry,
        )?;
        if let Some(pattern) = sprite.pattern_row_with_geometry(geometry) {
            selected += 1;
            if sprite.x == -SPRITE_ORIGIN {
                if unmasked {
                    break;
                }
            } else {
                unmasked = true;
                patterns[len] = Some(pattern);
                len += 1;
            }
            if selected == mode.sprites_per_line() {
                break;
            }
        }
        if sprite.link == 0 {
            break;
        }
        index = sprite.link;
    }
    let Some(first) = patterns[0] else {
        return memory.compose_raster_sprite_line(&[], mode);
    };
    let mut ordered = [first; H40_SPRITES_PER_LINE as usize];
    for (pattern, prepared) in ordered[..len].iter_mut().zip(patterns) {
        *pattern = prepared.ok_or(VdpRenderError::UnsupportedSpriteRow)?;
    }
    memory.compose_raster_sprite_line(&ordered[..len], mode)
}
