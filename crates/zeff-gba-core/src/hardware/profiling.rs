#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProfilingSnapshot {
    pub frames: u64,
    pub completed_instructions: u64,
    pub cpu_phase_visits: [u64; 8],
    pub instruction_fetches: u64,
    pub instruction_fetch_modes: [u64; 2],
    pub instruction_fetch_accesses: [u64; 2],
    pub instruction_fetch_regions: [u64; 7],
    pub instruction_fetch_descriptor_compatible: u64,
    pub instruction_fetch_fallbacks: [u64; 5],
    pub instruction_fetch_waitcnt_changes: u64,
    pub bus_step_calls: u64,
    pub bus_requested_cycles: u64,
    pub bus_chunks: u64,
    pub bus_deadline_hits: u64,
    pub bus_deadline_recomputes: u64,
    pub bus_deadline_expiries: [u64; 6],
    pub bus_deadline_invalidations: [u64; 3],
    pub visible_hblank_events: u64,
    pub vblank_events: u64,
    pub rendered_scanlines: u64,
    pub timer_overflows: [u64; 4],
    pub dma_starts: [u64; 4],
    pub dma_units: [u64; 4],
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CpuProfiling {
    pub completed_instructions: u64,
    pub phase_visits: [u64; 8],
    pub instruction_fetches: u64,
    pub instruction_fetch_modes: [u64; 2],
    pub instruction_fetch_accesses: [u64; 2],
    pub instruction_fetch_regions: [u64; 7],
    pub instruction_fetch_descriptor_compatible: u64,
    pub instruction_fetch_fallbacks: [u64; 5],
    pub instruction_fetch_waitcnt_changes: u64,
    pub last_instruction_fetch_waitcnt: Option<u16>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct BusProfiling {
    pub step_calls: u64,
    pub requested_cycles: u64,
    pub chunks: u64,
    pub deadline_hits: u64,
    pub deadline_recomputes: u64,
    pub deadline_expiries: [u64; 6],
    pub deadline_invalidations: [u64; 3],
    pub visible_hblank_events: u64,
    pub vblank_events: u64,
    pub rendered_scanlines: u64,
    pub timer_overflows: [u64; 4],
    pub dma_starts: [u64; 4],
    pub dma_units: [u64; 4],
}
