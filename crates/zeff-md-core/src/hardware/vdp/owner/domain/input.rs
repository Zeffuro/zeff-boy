use super::*;
use crate::hardware::vdp::control_core::RegisterViews;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SharedBus {
    pub address: u32,
    pub data: u16,
}

impl SharedBus {
    pub fn validate(self) -> Result<()> {
        ensure!(self.address <= 0x1ffff, "invalid shared VDP memory address");
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BoardInput {
    pub main: wiring::BoardPins,
    pub bus_grant: bool,
    pub bus_grant_acknowledge_level: bool,
    pub data_acknowledge_level: bool,
    pub reset_released: bool,
}

impl BoardInput {
    pub(super) fn validate(self) -> Result<()> {
        ensure!(self.reset_released, "runtime VDP reset owner is absent");
        ensure!(
            !self.main.sound_vdp_request && !self.main.test_mode,
            "unsupported VDP board wiring profile"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BoardView {
    pub half: u64,
    pub pass: u8,
    pub hsync: bool,
    pub csync: bool,
    pub edclk: bool,
    pub vclk: bool,
    pub zclk: bool,
    pub drives: wiring::Drives,
    #[cfg(test)]
    pub fifo_full: bool,
    pub data: Option<u16>,
    pub bus_request: bool,
    pub bus_grant_acknowledge: bool,
    pub interrupt: u8,
    pub sound_interrupt: bool,
}

pub(crate) trait BoardBoundary: Clone {
    const VDP_CAPTURE: bool = false;
    fn vdp(&mut self, _event: crate::sound_trace::physical::vdp::Event) {}
    #[cfg(test)]
    fn vdp_raw(&mut self, _event: crate::sound_trace::physical::vdp::raw::Event) {}
    // Own the MAIN phase/epoch and resolve all real drivers; no host-time phase adoption.
    #[cfg(test)]
    fn observe_dma_pressure(&mut self, _half: u64, _pass: u8, _source: u32) {}
    fn resolve(&mut self, view: BoardView) -> Result<BoardInput>;
    fn finish_pass(&mut self, view: BoardView, storage: &impl Storage) -> Result<Vec<Effect>>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RenderContext {
    pub mode128: bool,
    pub height: u16,
    pub phases: PhaseLevels,
    pub registers: RegisterViews,
    pub before_hv: super::super::irq::HvInputs,
    pub memory_active_before_hv: bool,
    pub frontend: frontend::MemoryDrives,
    pub horizontal: u16,
    pub vertical: u16,
    pub vertical_active: bool,
    pub vertical_display_before_hv: bool,
    pub vertical_display: bool,
    pub vertical_last: bool,
    pub memory_active: bool,
    pub input_bus: SharedBus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct RenderGeometry {
    pub mode5: bool,
    pub height: u16,
    pub horizontal: u16,
    pub vertical: u16,
    pub horizontal_active: bool,
    pub vertical_active: bool,
    pub display_enabled: bool,
    pub wide: bool,
}

pub(crate) trait RenderBus: Clone + std::fmt::Debug + PartialEq + Eq {
    fn software_origin(pal: bool) -> Self;
    fn begin_descriptor_edge(&mut self, horizontal_master: bool) -> Result<()>;
    fn finish_descriptor_edge(&mut self) -> Result<bool>;
    fn begin_sprite_half(
        &mut self,
        phases: PhaseLevels,
        mode5: bool,
        horizontal: u16,
    ) -> Result<()>;
    fn finish_sprite_half(&mut self) -> Result<bool>;
    fn write_state_tail(&self, writer: &mut StateWriter) -> Result<()>;
    fn read_state_tail(&mut self, reader: &mut StateReader<'_>) -> Result<()>;
    fn serial_input(&self) -> u8;
    fn sprite_signals(&self) -> super::super::status::SpriteSignals;
    fn palette_signals(&self) -> internal_ram::PaletteSignals;
    fn geometry(&self) -> RenderGeometry;
    fn planes(
        &mut self,
        context: RenderContext,
        vsram: internal_ram::PlanesOutput,
        bus: SharedBus,
        storage: &impl Storage,
    ) -> Result<SharedBus>;
    fn sprites(
        &mut self,
        context: RenderContext,
        bus: SharedBus,
        storage: &impl Storage,
    ) -> Result<SharedBus>;
    fn respond(
        &mut self,
        pins: ram::Pins,
        response: ram::Response,
        storage: &impl Storage,
    ) -> Result<()>;
    fn write_state(&self, writer: &mut StateWriter);
    fn read_state(reader: &mut StateReader<'_>, pal: bool) -> Result<Self>;
    fn validate_state(&self, registers: [u8; 24]) -> Result<()>;
    fn validate_response(&self, pins: ram::Pins, origin: bool) -> Result<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PinCache {
    pub levels: [bool; 15],
    pub ad: u8,
    pub rd: u8,
    pub sd: u8,
}

impl PinCache {
    pub fn software_origin() -> Self {
        Self {
            levels: [
                false, true, false, false, true, true, false, true, true, false, true, false,
                false, false, true,
            ],
            ad: 0,
            rd: 0,
            sd: 0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        board: BoardInput,
        d1: bool,
        d2: bool,
        cpu0: bool,
        cpu1: bool,
        hsync: bool,
        csync: bool,
        bus: ram::BusInputs,
        sd: u8,
    ) -> Self {
        let p = board.main;
        Self {
            levels: [
                d1,
                d2,
                cpu0,
                cpu1,
                hsync,
                csync,
                board.bus_grant,
                board.bus_grant_acknowledge_level,
                board.data_acknowledge_level,
                p.address_strobe,
                p.read,
                p.upper_strobe,
                p.lower_strobe,
                p.interrupt_acknowledge,
                board.reset_released,
            ],
            ad: bus.ad,
            rd: bus.rd,
            sd,
        }
    }
}

pub(super) fn profile(registers: [u8; 24], mode128: bool) -> Result<internal_ram::Profile> {
    ensure!(
        registers[0] & 1 == 0 && registers[11] & 128 == 0 && registers[12] & 6 == 0,
        "unsupported VDP sync/interlace/address profile"
    );
    let profile = internal_ram::Profile {
        memory: ram::Profile {
            registers,
            mode128,
            interlace_latch: [false; 2],
            test0: 0,
        },
    };
    profile.validate()?;
    Ok(profile)
}

pub(super) struct VdpCapture<'a, B>(pub &'a mut B);
impl<B: BoardBoundary> crate::sound_trace::physical::vdp::Capture for VdpCapture<'_, B> {
    fn vdp(&mut self, event: crate::sound_trace::physical::vdp::Event) {
        self.0.vdp(event);
    }
    #[cfg(test)]
    fn vdp_raw(&mut self, event: crate::sound_trace::physical::vdp::raw::Event) {
        self.0.vdp_raw(event);
    }
}
