use super::{PceHardwareTopology, PhysicalRegion, decode_physical_region_for};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PceProfilingSnapshot {
    pub cpu_boundaries: u64,
    pub cpu_instruction_boundaries: u64,
    pub cpu_interrupt_boundaries: u64,
    pub cpu_low_speed_cycles: u64,
    pub cpu_high_speed_cycles: u64,
    pub bus_reads: u64,
    pub bus_writes: u64,
    pub bus_dummy_reads: u64,
    pub bus_dummy_writes: u64,
    pub bus_idle_cycles: u64,
    pub bus_hucard_accesses: u64,
    pub bus_work_ram_accesses: u64,
    pub bus_vdc_accesses: u64,
    pub bus_vpc_accesses: u64,
    pub bus_vdc2_accesses: u64,
    pub bus_vce_accesses: u64,
    pub bus_psg_accesses: u64,
    pub bus_timer_accesses: u64,
    pub bus_controller_accesses: u64,
    pub bus_irq_accesses: u64,
    pub bus_unmapped_accesses: u64,
    pub device_advance_calls: u64,
    pub device_advance_chunks: u64,
    pub device_advance_master_ticks: u64,
    pub vdc_advance_calls: u64,
    pub vdc_pixel_clocks: u64,
    pub vdc_phase_transitions: u64,
    pub vdc_dma_slots: u64,
    pub vdc_dma_active_slots: u64,
    pub vdc_dma_idle_slots: u64,
    pub vce_line_transitions: u64,
    pub raster_base_lines: u64,
    pub raster_supergrafx_lines: u64,
    pub raster_active_lines: u64,
    pub raster_pixels: u64,
    pub psg_advance_calls: u64,
    pub psg_master_ticks: u64,
    pub psg_internal_clocks: u64,
    pub psg_oscillator_clocks: u64,
    pub psg_mix_scans: u64,
    pub psg_mixer_source_examinations: u64,
    pub psg_mixer_source_transitions: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PceProfiling {
    pub(crate) snapshot: PceProfilingSnapshot,
}

impl PceProfiling {
    pub(crate) fn record_bus_access(&mut self, topology: PceHardwareTopology, physical_addr: u32) {
        match decode_physical_region_for(topology, physical_addr) {
            PhysicalRegion::HuCard(_) => self.snapshot.bus_hucard_accesses += 1,
            PhysicalRegion::WorkRam(_) => self.snapshot.bus_work_ram_accesses += 1,
            PhysicalRegion::Vdc(_) => self.snapshot.bus_vdc_accesses += 1,
            PhysicalRegion::Vpc(_) => self.snapshot.bus_vpc_accesses += 1,
            PhysicalRegion::Vdc2(_) => self.snapshot.bus_vdc2_accesses += 1,
            PhysicalRegion::Vce(_) => self.snapshot.bus_vce_accesses += 1,
            PhysicalRegion::Psg(_) => self.snapshot.bus_psg_accesses += 1,
            PhysicalRegion::Timer(_) => self.snapshot.bus_timer_accesses += 1,
            PhysicalRegion::Controller => self.snapshot.bus_controller_accesses += 1,
            PhysicalRegion::Irq(_) => self.snapshot.bus_irq_accesses += 1,
            PhysicalRegion::Unmapped => self.snapshot.bus_unmapped_accesses += 1,
        }
    }

    pub(crate) fn record_vdc_advance(
        &mut self,
        pixel_clocks: u64,
        phase_transitions: u64,
        dma_slots: u64,
        active_dma_slots: u64,
    ) {
        self.snapshot.vdc_advance_calls += 1;
        self.snapshot.vdc_pixel_clocks += pixel_clocks;
        self.snapshot.vdc_phase_transitions += phase_transitions;
        self.snapshot.vdc_dma_slots += dma_slots;
        self.snapshot.vdc_dma_active_slots += active_dma_slots;
        self.snapshot.vdc_dma_idle_slots += dma_slots - active_dma_slots;
    }
}
