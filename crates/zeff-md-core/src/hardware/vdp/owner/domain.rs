mod input;
mod observation;
mod primitive;
mod rollback_memory;
mod state;
mod storage;

use super::{
    clock::ClockCone, counter::Counter, frontend, hv::HvCone, internal_ram, irq::IrqCone, ram,
    status, wiring,
};
use crate::hardware::vdp::control_core::{ControlCore, PhaseLevels, RegisterStage};
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Context, Result, ensure};
use input::PinCache;
pub(in crate::hardware::vdp) use input::RenderGeometry;
pub(crate) use input::{BoardBoundary, BoardInput, BoardView};
pub(super) use input::{RenderBus, RenderContext, SharedBus};
use storage::Overlay;
pub(super) use storage::Receipt;
pub(crate) use storage::{Effect, Storage};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Domain<R> {
    pal: bool,
    half: u64,
    mode128: bool,
    clocks: ClockCone,
    hv: HvCone,
    irq: IrqCone,
    status: status::Status,
    counter: Counter,
    control: ControlCore,
    frontend: frontend::Frontend,
    wiring: wiring::Wiring,
    ram: ram::Ram,
    internal: internal_ram::InternalRam,
    render: R,
    old_dclk: [bool; 3],
    feedback: [bool; 3],
    zclk_feedback: bool,
    board_clocks: [bool; 2],
    render_dirty: bool,
    input_cache: PinCache,
    shared: SharedBus,
    ram_bus: ram::BusInputs,
    io_data: u16,
    memory_active: bool,
    fifo_full: bool,
    pending: Option<Receipt>,
}

impl<R: RenderBus> Domain<R> {
    pub fn software_origin(pal: bool) -> Self {
        Self {
            pal,
            half: 0,
            mode128: false,
            clocks: ClockCone::software_origin(pal),
            hv: HvCone::new(pal),
            irq: IrqCone::default(),
            status: status::Status::default(),
            counter: Counter::default(),
            control: ControlCore::software_origin(),
            frontend: frontend::Frontend::software_origin(),
            wiring: wiring::Wiring::software_origin(),
            ram: ram::Ram::software_origin(),
            internal: internal_ram::InternalRam::software_origin(),
            render: R::software_origin(pal),
            old_dclk: [false, true, true],
            feedback: [true, false, false],
            zclk_feedback: false,
            board_clocks: [false; 2],
            render_dirty: false,
            input_cache: PinCache::software_origin(),
            shared: SharedBus {
                address: 0,
                data: 0,
            },
            ram_bus: ram::BusInputs { ad: 0, rd: 0 },
            io_data: 0,
            memory_active: false,
            fifo_full: false,
            pending: None,
        }
    }

    pub(super) fn advance_half_owned<B: BoardBoundary>(
        mut self,
        mut boundary: B,
        storage: &impl Storage,
    ) -> Result<(Self, B, Receipt)> {
        ensure!(self.pending.is_none(), "unretired VDP physical effects");
        self.half = self
            .half
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("VDP half clock overflow"))?;
        let mut effects = Vec::new();
        let mut dma_activity = false;
        let descriptor_sample = self.begin_render_half()?;
        for pass in 0..2 {
            self.board_pass(
                pass,
                &mut boundary,
                storage,
                &mut effects,
                &mut dma_activity,
            )
            .with_context(|| {
                format!(
                    "VDP half={} pass={pass} descriptor_sample={descriptor_sample}",
                    self.half
                )
            })?;
        }
        self.finish_render_half(descriptor_sample)?;
        self.ram.validate_frontier()?;
        ensure!(
            effects.len() <= storage::MAX_EFFECTS,
            "VDP half-cycle effect capacity"
        );
        let receipt = Receipt {
            half: self.half,
            effects,
            dma_activity,
            pixel: self.internal.pixel(),
        };
        self.pending = Some(receipt.clone());
        Ok((self, boundary, receipt))
    }

    fn begin_render_half(&mut self) -> Result<bool> {
        let cycle = self
            .half
            .checked_add(3)
            .ok_or_else(|| anyhow::anyhow!("VDP phase origin overflow"))?;
        let descriptor_sample = cycle & 1 != 0;
        if descriptor_sample {
            self.render.begin_descriptor_edge(self.clocks.h1())?;
        }
        self.render
            .begin_sprite_half(
                PhaseLevels {
                    data_master: self.input_cache.levels[0],
                    data_slave: self.input_cache.levels[1],
                    horizontal_master: self.clocks.h1(),
                    horizontal_slave: self.clocks.h2(),
                },
                self.registers()[1] & 4 != 0,
                self.hv.counters().0,
            )
            .with_context(|| format!("VDP sprite sample half={}", self.half))?;
        Ok(descriptor_sample)
    }

    fn finish_render_half(&mut self, descriptor_sample: bool) -> Result<()> {
        if descriptor_sample {
            self.render_dirty |= self.render.finish_descriptor_edge()?;
        }
        self.render_dirty |= self.render.finish_sprite_half()?;
        Ok(())
    }

    pub fn retire(&mut self, receipt: &Receipt) -> Result<()> {
        ensure!(
            self.pending.as_ref() == Some(receipt),
            "stale VDP physical effect retirement"
        );
        self.pending = None;
        Ok(())
    }

    pub fn pending(&self) -> Option<&Receipt> {
        self.pending.as_ref()
    }
    pub fn half(&self) -> u64 {
        self.half
    }
    pub fn internal_sequence(&self) -> u64 {
        self.internal.sequence()
    }
    pub fn raster_field(&self) -> u64 {
        self.internal.raster_field()
    }
    pub fn pixel(&self) -> Option<internal_ram::VideoPixel> {
        self.internal.pixel()
    }
    pub fn board_clocks(&self) -> [bool; 2] {
        self.board_clocks
    }
    pub fn registers(&self) -> [u8; 24] {
        self.control.registers().logical()
    }

    fn status_word(&self) -> u16 {
        self.status.word(status::Inputs {
            pal: self.pal,
            dma_active: self.frontend.dma_active(),
            hblank: self.hv.hblank(),
            vblank: !self.memory_active,
            odd_field: false,
            vint_pending: self.irq.vp,
            fifo_full: self.fifo_full,
            fifo_empty: self.frontend.fifo_empty(),
        })
    }

    pub fn view(&self, pass: u8) -> Result<BoardView> {
        let external = self.frontend.external_feedback();
        let dma = self.control.registers().dma();
        let drives = self.wiring.drives(
            u32::from(dma.source_low()) | u32::from(dma.source_high()) << 16,
            external,
        )?;
        Ok(BoardView {
            half: self.half,
            pass,
            hsync: self.feedback[0],
            edclk: self.feedback[1],
            vclk: self.feedback[2],
            zclk: self.zclk_feedback,
            csync: self.hv.csync_drive() != 0,
            drives,
            #[cfg(test)]
            fifo_full: self.fifo_full,
            data: (drives.levels.data_read || drives.levels.status_read || drives.levels.hv_read)
                .then_some(self.io_data),
            bus_request: self.frontend.bus_request(),
            bus_grant_acknowledge: external.bus_grant_acknowledge,
            interrupt: self.irq.level() as u8,
            sound_interrupt: self.irq.sound,
        })
    }

    fn board_pass<B: BoardBoundary>(
        &mut self,
        pass: u8,
        boundary: &mut B,
        storage: &impl Storage,
        effects: &mut Vec<Effect>,
        dma_activity: &mut bool,
    ) -> Result<()> {
        let view = self.view(pass)?;
        self.board_clocks = [view.vclk, view.zclk];
        let mut board = boundary.resolve(view)?;
        board.validate()?;
        if let Some(word) = view.data {
            self.io_data = word;
            if B::VDP_CAPTURE {
                boundary.vdp(crate::sound_trace::physical::vdp::Event::Readback);
            }
            #[cfg(test)]
            boundary.vdp_raw(crate::sound_trace::physical::vdp::raw::Event::Readback);
        } else {
            self.io_data = board.main.data;
        }
        board.main.data = self.io_data;
        let dma = self.control.registers().dma();
        if B::VDP_CAPTURE {
            self.wiring.capture_bus_physical(
                board.main,
                self.frontend.external_feedback(),
                u32::from(dma.source_low()) | u32::from(dma.source_high()) << 16,
                &mut input::VdpCapture(boundary),
            )?;
        } else {
            self.wiring.capture_bus(
                board.main,
                self.frontend.external_feedback(),
                u32::from(dma.source_low()) | u32::from(dma.source_high()) << 16,
            )?;
        }

        let [hsync, edclk, vclk] = self.feedback;
        self.feedback = [
            self.hv.hsync_drive() != 0,
            self.clocks.edclk(),
            self.clocks.cpu_clk1(),
        ];
        self.zclk_feedback = self.clocks.cpu_clk0();
        let cycle = self
            .half
            .checked_add(3)
            .ok_or_else(|| anyhow::anyhow!("VDP phase origin overflow"))?;
        let divided_mclk_level = cycle & 1 != 0;
        self.clocks.eval_edclk(divided_mclk_level, hsync);
        self.clocks
            .eval_mclk(divided_mclk_level, edclk, vclk, self.registers()[12]);
        let dclk = self.clocks.dclk();
        self.old_dclk[(cycle % 3) as usize] = dclk;
        let changed = dclk != self.old_dclk[((cycle % 3 + 1) % 3) as usize];
        let d1 = changed && dclk;
        let d2 = changed && !dclk;
        let input = PinCache::new(
            board,
            d1,
            d2,
            self.clocks.cpu_clk0(),
            vclk,
            hsync,
            view.csync,
            self.ram_bus,
            self.render.serial_input(),
        );
        if input != self.input_cache || self.render_dirty {
            for ordinal in 0..if d1 || d2 { 2 } else { 1 } {
                self.primitive(
                    primitive::Input {
                        board,
                        levels: [d1, d2, vclk],
                        pass,
                        ordinal,
                    },
                    boundary,
                    storage,
                    effects,
                    dma_activity,
                )?;
            }
            self.input_cache = input;
            self.render_dirty = false;
        }
        let overlay = Overlay { storage, effects };
        let response = self
            .ram
            .respond(self.ram_bus, |address| overlay.vram(address))?;
        self.ram_bus = response.bus;
        if let Some(write) = response.write {
            effects.push(Effect::Vram(write));
        }
        self.render
            .respond(self.ram.pins(), response, &Overlay { storage, effects })?;
        let levels = self.view(pass)?.drives.levels;
        if levels.status_read {
            self.io_data = (self.io_data & !0x3ff) | self.status_word();
        }
        let counter = self.counter.drive(levels.hv_read, levels.hv_read);
        self.io_data = (self.io_data & !counter.mask) | counter.word;
        if levels.data_read {
            self.io_data = self.frontend.read_word();
        }
        let writes = boundary.finish_pass(self.view(pass)?, &Overlay { storage, effects })?;
        ensure!(
            writes.len() <= 2
                && writes
                    .iter()
                    .all(|effect| matches!(effect, Effect::WorkRam { .. })),
            "invalid MAIN memory effects"
        );
        ensure!(pass == 1 || writes.is_empty(), "early MAIN memory write");
        effects.extend(writes);
        Ok(())
    }
}
