use super::Bus;
use crate::hardware::constants::RAM_MIRROR_MASK;
use zeff_emu_common::time::MasterTicks;

const PPU_DOTS_AFTER_CPU_ACCESS: u8 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PeripheralTickEvents {
    pub nmi_raised: bool,
    pub first_nmi_cpu_cycle: Option<u64>,
    pub irq_pending: bool,
    pub first_irq_cpu_cycle: Option<u64>,
}

impl Bus {
    fn clock_ppu_dots(&mut self, dots: u8) {
        for _ in 0..dots {
            self.ppu_render_dot();
            self.ppu.tick();
            self.ppu_cycles += 1;
        }
    }

    pub(super) fn begin_timed_cpu_cycle(&mut self) {
        let ppu_dots = self.ppu_clock.next_cpu_cycle_ppu_dots();
        self.clock_ppu_dots(ppu_dots - PPU_DOTS_AFTER_CPU_ACCESS);
    }

    pub(super) fn finish_timed_cpu_cycle(&mut self, stolen: bool) {
        self.clock_ppu_dots(PPU_DOTS_AFTER_CPU_ACCESS);

        let event_cycle = self.cpu_step_elapsed_cycles + 1;
        let nmi_raised = !self.cpu_nmi_line_sampled && self.ppu.nmi_output;
        self.cpu_nmi_line_sampled = self.ppu.nmi_output;
        if nmi_raised {
            self.cpu_step_events.nmi_raised = true;
            self.cpu_step_events
                .first_nmi_cpu_cycle
                .get_or_insert(event_cycle);
        }

        self.clock_cpu_rate_peripherals(event_cycle);
        self.cpu_step_elapsed_cycles += 1;
        if stolen {
            self.dma_stall_cycles += 1;
        }
    }

    fn clock_cpu_rate_peripherals(&mut self, next_elapsed: u64) {
        self.apu.expansion_audio = self.cartridge.audio_output();
        self.apu.tick();
        let dmc_load_was_pending = self.dma.dmc_load_is_pending();
        self.dma.clock_dmc_load_delay();

        if self.apu.dmc.needs_dma() && !self.dma.dmc_load_is_pending() {
            let next_cycle_is_put = !self.cpu_cycle_is_odd(next_elapsed);
            if dmc_load_was_pending || next_cycle_is_put {
                self.dma.request_dmc();
            }
        }

        self.cartridge.clock_cpu();
        if self.apu.irq_pending() || self.cartridge.irq_pending() {
            self.cpu_step_events.irq_pending = true;
            self.cpu_step_events
                .first_irq_cpu_cycle
                .get_or_insert(next_elapsed);
        }
    }

    fn dmc_dma_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & RAM_MIRROR_MASK) as usize],
            0x4020..=0xFFFF => self.cartridge.cpu_read(addr),
            _ => 0,
        }
    }

    fn dma_access_tick(&self) -> Option<MasterTicks> {
        self.cpu_step_start_tick
            .map(|start| MasterTicks::new(start.get().wrapping_add(self.cpu_step_elapsed_cycles)))
    }

    fn dma_dummy_read(&mut self, addr: u16, controller_read_clocked: &mut bool) {
        if matches!(addr, 0x4016 | 0x4017) {
            if *controller_read_clocked {
                return;
            }
            *controller_read_clocked = true;
        }
        self.cpu_read_for_dma_with_trace(addr, self.dma_access_tick(), false);
    }

    fn run_dma_cycle(&mut self, halted_read_addr: u16, controller_read_clocked: &mut bool) {
        self.begin_timed_cpu_cycle();
        let get_cycle = !self.cpu_cycle_is_odd(self.cpu_step_elapsed_cycles.wrapping_add(1));
        if get_cycle && self.dma.dmc_is_ready() {
            self.dma.consume_cycle_setup();
            let addr = self.apu.dmc.dma_address();
            let byte = self.dmc_dma_read(addr);
            self.apu.dmc.fill_sample_buffer(byte);
            self.dma.take_dmc();
        } else if get_cycle {
            if let Some(addr) = self.dma.oam_read_address() {
                self.dma.consume_cycle_setup();
                let byte = self.cpu_read_for_dma(addr, self.dma_access_tick());
                self.dma.store_oam_read(byte);
            } else {
                self.dma.consume_cycle_setup();
                self.dma_dummy_read(halted_read_addr, controller_read_clocked);
            }
        } else if let Some(byte) = self.dma.take_oam_write() {
            self.dma.consume_cycle_setup();
            self.write_oam_data(byte);
        } else {
            self.dma.consume_cycle_setup();
            self.dma_dummy_read(halted_read_addr, controller_read_clocked);
        }
        self.finish_timed_cpu_cycle(true);
    }

    pub(super) fn service_pending_dma(&mut self, halted_read_addr: u16) {
        if !self.dma.needs_halt() {
            return;
        }

        let mut controller_read_clocked = false;
        self.begin_timed_cpu_cycle();
        self.dma.consume_cycle_setup();
        self.dma_dummy_read(halted_read_addr, &mut controller_read_clocked);
        self.finish_timed_cpu_cycle(true);

        while self.dma.is_active() {
            self.run_dma_cycle(halted_read_addr, &mut controller_read_clocked);
        }
    }

    pub fn tick_peripherals(&mut self, cpu_cycles: u64) -> PeripheralTickEvents {
        let saved_events = std::mem::take(&mut self.cpu_step_events);
        let saved_elapsed = self.cpu_step_elapsed_cycles;
        self.cpu_step_elapsed_cycles = 0;

        for _ in 0..cpu_cycles {
            self.begin_timed_cpu_cycle();
            self.finish_timed_cpu_cycle(false);
        }

        let events = std::mem::replace(&mut self.cpu_step_events, saved_events);
        self.cpu_step_elapsed_cycles = saved_elapsed;
        events
    }

    pub(crate) fn take_nmi_edge_for_vector(&mut self) -> bool {
        if self.cpu_step_events.nmi_raised {
            self.cpu_step_events.nmi_raised = false;
            self.cpu_step_events.first_nmi_cpu_cycle = None;
            true
        } else {
            false
        }
    }

    pub(crate) fn begin_cpu_step_timing(&mut self, start_tick: MasterTicks) {
        self.cpu_step_elapsed_cycles = 0;
        self.cpu_step_events = PeripheralTickEvents::default();
        self.begin_cpu_access_timing(start_tick);
    }

    pub(crate) fn begin_cpu_access_timing(&mut self, start_tick: MasterTicks) {
        self.cpu_step_start_tick = Some(start_tick);
        self.cpu_access_elapsed_cycles = 0;
    }

    pub(crate) fn prepare_cpu_instruction_accesses(&mut self) {
        if self.cpu_step_start_tick.is_none() {
            self.cpu_access_elapsed_cycles = 0;
        }
    }

    pub(crate) fn advance_cpu_access_timing_to(&mut self, elapsed_cycles: u64) {
        self.cpu_access_elapsed_cycles = self.cpu_access_elapsed_cycles.max(elapsed_cycles);
    }

    pub(crate) fn next_cpu_access_tick(&mut self) -> Option<MasterTicks> {
        let elapsed = self.cpu_step_elapsed_cycles;
        self.cpu_access_elapsed_cycles = self.cpu_access_elapsed_cycles.wrapping_add(1);
        self.cpu_step_start_tick
            .map(|start| MasterTicks::new(start.get().wrapping_add(elapsed)))
    }

    pub(crate) fn cpu_cycle_is_odd(&self, elapsed_cycles: u64) -> bool {
        self.cpu_step_start_tick
            .map(|start| start.get().wrapping_add(elapsed_cycles) & 1 != 0)
            .unwrap_or(self.cpu_odd_cycle)
    }

    pub(crate) fn finish_cpu_instruction_accesses(&mut self, total_cycles: u64, pc: u16) {
        while self.cpu_access_elapsed_cycles < total_cycles {
            let _ = self.cpu_read_timed(pc);
        }
        debug_assert_eq!(self.cpu_access_elapsed_cycles, total_cycles);
        self.service_pending_dma(pc);
    }

    pub(crate) fn finish_cpu_step_timing(&mut self, total_cycles: u64) -> PeripheralTickEvents {
        debug_assert_eq!(self.cpu_step_elapsed_cycles, total_cycles);
        let events = self.cpu_step_events;
        self.cpu_step_events = PeripheralTickEvents::default();
        self.cpu_step_elapsed_cycles = 0;
        self.cpu_step_start_tick = None;
        self.cpu_access_elapsed_cycles = 0;
        events
    }
}

#[cfg(test)]
mod tests {
    use super::Bus;
    use crate::hardware::cartridge::Cartridge;
    use crate::hardware::timing::NesTiming;

    fn test_bus(timing: NesTiming) -> Bus {
        let mut rom = vec![0_u8; 16 + 0x4000 + 0x2000];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[5] = 1;
        let cartridge = Cartridge::load(&rom).expect("test ROM should load");
        Bus::new_with_timing(cartridge, 48_000.0, timing)
    }

    #[test]
    fn bus_applies_each_region_cpu_to_ppu_clock_ratio() {
        let mut ntsc = test_bus(NesTiming::Ntsc);
        let mut pal = test_bus(NesTiming::Pal);
        let mut dendy = test_bus(NesTiming::Dendy);

        ntsc.tick_peripherals(5);
        pal.tick_peripherals(5);
        dendy.tick_peripherals(5);

        assert_eq!(ntsc.ppu_cycles, 15);
        assert_eq!(pal.ppu_cycles, 16);
        assert_eq!(dendy.ppu_cycles, 15);
        assert_eq!(ntsc.ppu.pre_render_scanline(), 261);
        assert_eq!(pal.ppu.pre_render_scanline(), 311);
        assert_eq!(dendy.ppu.pre_render_scanline(), 311);
    }
}
