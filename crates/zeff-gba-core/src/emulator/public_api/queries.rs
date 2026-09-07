use crate::emulator::Emulator;
use crate::hardware::cartridge::{BackupKind, RomHeader, SensorKind, TiltState};
use crate::hardware::cpu::{CpuMode, CpuState, FetchedInstruction};
use zeff_emu_common::time::{
    ClockRate, FrameLifecycle, MachineTiming, MasterTicks, Reset, TimingSnapshot,
};

const MASTER_CLOCK_RATE: ClockRate =
    ClockRate::from_hz(crate::hardware::constants::CPU_CLOCK_HZ as u64);

impl Emulator {
    pub fn framebuffer(&self) -> &[u8] {
        self.bus.ppu.framebuffer()
    }

    pub fn framebuffer_dimensions(&self) -> (usize, usize) {
        self.bus.ppu.dimensions()
    }

    pub fn frame_ready(&self) -> bool {
        self.bus.ppu.frame_ready
    }

    pub fn clear_frame_ready(&mut self) {
        self.bus.ppu.frame_ready = false;
    }

    pub fn apu_debug_snapshot(&self) -> crate::hardware::apu::ApuDebugSnapshot {
        self.bus.apu.debug_snapshot()
    }

    pub fn dma_channels_snapshot(&self) -> [crate::hardware::dma::DmaChannel; 4] {
        self.bus.dma.channels()
    }

    pub fn cpu_state(&self) -> CpuState {
        self.cpu.state
    }

    pub fn cpu_pc(&self) -> u32 {
        self.cpu.pc()
    }

    pub fn cpu_registers(&self) -> [u32; 16] {
        self.cpu.regs
    }

    pub fn cpu_cpsr(&self) -> u32 {
        self.cpu.cpsr
    }

    pub fn cpu_mode(&self) -> CpuMode {
        self.cpu.mode()
    }

    pub fn cpu_thumb_state(&self) -> bool {
        self.cpu.thumb_state()
    }

    pub fn cpu_visible_pc(&self) -> u32 {
        self.cpu.visible_pc()
    }

    pub fn last_fetch(&self) -> Option<FetchedInstruction> {
        self.cpu.last_fetch
    }

    pub fn cpu_cycles(&self) -> u64 {
        self.cpu.cycles
    }

    pub fn timing_snapshot(&self) -> TimingSnapshot {
        <Self as MachineTiming>::timing_snapshot(self)
    }

    pub fn cartridge_header(&self) -> &RomHeader {
        self.bus.cartridge.header()
    }

    pub fn cartridge_rom_bytes(&self) -> &[u8] {
        self.bus.cartridge.rom()
    }

    pub fn backup_kind(&self) -> BackupKind {
        self.bus.cartridge.backup_kind()
    }

    pub fn sensor_kind(&self) -> SensorKind {
        self.bus.cartridge.sensor_kind()
    }

    pub fn tilt_state(&self) -> Option<TiltState> {
        self.bus.cartridge.tilt_state()
    }

    pub fn rom_hash(&self) -> [u8; 32] {
        self.rom_hash
    }

    pub fn has_external_bios(&self) -> bool {
        self.bus.has_external_bios()
    }

    pub fn rom_offset_for_cpu_address(&self, address: u32) -> Option<usize> {
        let offset = match address {
            0x0800_0000..=0x09FF_FFFF => address - 0x0800_0000,
            0x0A00_0000..=0x0BFF_FFFF => address - 0x0A00_0000,
            0x0C00_0000..=0x0DFF_FFFF => address - 0x0C00_0000,
            _ => return None,
        };
        ((offset as usize) < self.bus.cartridge.rom().len()).then_some(offset as usize)
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    pub fn system_ram(&self) -> (&[u8], &[u8]) {
        self.bus.system_ram()
    }

    pub fn vram_snapshot(&self) -> &[u8] {
        &self.bus.vram
    }

    pub fn video_ram_snapshot(&self) -> &[u8] {
        self.vram_snapshot()
    }

    pub fn palette_ram_snapshot(&self) -> &[u8] {
        &self.bus.palette_ram
    }

    pub fn io_snapshot(&self) -> &[u8] {
        &self.bus.io
    }

    pub fn oam_snapshot(&self) -> &[u8] {
        &self.bus.oam
    }

    pub fn ppu_debug_snapshot(&self) -> crate::hardware::ppu::PpuDebugSnapshot {
        self.bus.ppu_debug_snapshot()
    }

    #[cfg(feature = "profiling")]
    pub fn profiling_snapshot(&self) -> crate::hardware::profiling::ProfilingSnapshot {
        crate::hardware::profiling::ProfilingSnapshot {
            frames: self.profiling_frames,
            completed_instructions: self.cpu.profiling.completed_instructions,
            frame_cpu_runs: self.cpu.profiling.frame_runs,
            frame_cpu_run_instructions: self.cpu.profiling.frame_run_instructions,
            frame_cpu_direct_runs: self.cpu.profiling.frame_direct_runs,
            frame_cpu_direct_instructions: self.cpu.profiling.frame_direct_instructions,
            frame_cpu_direct_cycles: self.cpu.profiling.frame_direct_cycles,
            frame_cpu_direct_kinds: self.cpu.profiling.frame_direct_kinds,
            frame_scalar_arm: self.cpu.profiling.frame_scalar_arm,
            frame_scalar_thumb: self.cpu.profiling.frame_scalar_thumb,
            frame_scalar_arm_halfword: self.cpu.profiling.frame_scalar_arm_halfword,
            pure_opcode_requests: self.cpu.profiling.pure_opcodes.requests,
            pure_opcode_hits: self.cpu.profiling.pure_opcodes.hits,
            cpu_phase_visits: self.cpu.profiling.phase_visits,
            instruction_classes_arm: self.cpu.profiling.instruction_classes_arm,
            instruction_classes_thumb: self.cpu.profiling.instruction_classes_thumb,
            frame_kernel_candidates: self.cpu.profiling.frame_kernel_candidates,
            frame_kernel_candidate_cycles: self.cpu.profiling.frame_kernel_candidate_cycles,
            frame_kernel_fetch_gates: self.cpu.profiling.frame_kernel_fetch_gates,
            frame_kernel_fetch_eligible: self.cpu.profiling.frame_kernel_fetch_eligible,
            frame_kernel_fetch_eligible_cycles: self
                .cpu
                .profiling
                .frame_kernel_fetch_eligible_cycles,
            frame_kernel_quiet_instructions: self.cpu.profiling.frame_kernel_quiet_instructions,
            frame_kernel_quiet_runs: self.cpu.profiling.frame_kernel_quiet_runs,
            frame_kernel_quiet_longest_run: self.cpu.profiling.frame_kernel_quiet_longest_run,
            thumb_macro_counts: self.cpu.profiling.thumb_macro_counts,
            thumb_macro_plain_halfwords: self.cpu.profiling.thumb_macro_plain_halfwords,
            thumb_macro_eligible: self.cpu.profiling.thumb_macro_eligible,
            thumb_macro_quiet: self.cpu.profiling.thumb_macro_quiet,
            thumb_macro_run_ends: self.cpu.profiling.thumb_macro_run_ends,
            thumb_macro_neighbors: self.cpu.profiling.thumb_macro_neighbors,
            instruction_fetches: self.cpu.profiling.instruction_fetches,
            cpu_generic_fetch_decode_calls: self.cpu.profiling.generic_fetch_decode_calls,
            cpu_gamepak_block_fetches: self.cpu.profiling.gamepak_block_fetches,
            cpu_ram_block_fetches: self.cpu.profiling.ram_block_fetches,
            instruction_fetch_modes: self.cpu.profiling.instruction_fetch_modes,
            instruction_fetch_accesses: self.cpu.profiling.instruction_fetch_accesses,
            instruction_fetch_regions: self.cpu.profiling.instruction_fetch_regions,
            instruction_fetch_descriptor_compatible: self
                .cpu
                .profiling
                .instruction_fetch_descriptor_compatible,
            instruction_fetch_fallbacks: self.cpu.profiling.instruction_fetch_fallbacks,
            instruction_fetch_waitcnt_changes: self.cpu.profiling.instruction_fetch_waitcnt_changes,
            bus_step_calls: self.bus.profiling.step_calls,
            bus_requested_cycles: self.bus.profiling.requested_cycles,
            bus_deferred_step_calls: self.bus.profiling.deferred_step_calls,
            bus_deferred_cycles: self.bus.profiling.deferred_cycles,
            bus_service_entries: self.bus.profiling.service_entries,
            bus_chunks: self.bus.profiling.chunks,
            apu_step_output_calls: self.bus.profiling.apu_step_output_calls,
            apu_step_output_cycles: self.bus.profiling.apu_step_output_cycles,
            apu_non_observation_chunks: self.bus.profiling.apu_non_observation_chunks,
            apu_non_observation_cycles: self.bus.profiling.apu_non_observation_cycles,
            apu_ppu_only_non_observation_chunks: self
                .bus
                .profiling
                .apu_ppu_only_non_observation_chunks,
            apu_ppu_only_non_observation_cycles: self
                .bus
                .profiling
                .apu_ppu_only_non_observation_cycles,
            apu_deadline_materializations: self.bus.profiling.apu_deadline_materializations,
            apu_timer_overflow_ordering_cases: self.bus.profiling.apu_timer_overflow_ordering_cases,
            apu_timer_overflow_ordering_count: self.bus.profiling.apu_timer_overflow_ordering_count,
            frame_service_pending_spans: self.bus.profiling.frame_service_pending_spans,
            frame_service_pending_cycles: self.bus.profiling.frame_service_pending_cycles,
            frame_service_pending_max_cycles: self.bus.profiling.frame_service_pending_max_cycles,
            frame_service_pending_span_buckets: self
                .bus
                .profiling
                .frame_service_pending_span_buckets,
            bus_deadline_hits: self.bus.profiling.deadline_hits,
            bus_deadline_recomputes: self.bus.profiling.deadline_recomputes,
            bus_deadline_expiries: self.bus.profiling.deadline_expiries,
            bus_deadline_invalidations: self.bus.profiling.deadline_invalidations,
            visible_hblank_events: self.bus.profiling.visible_hblank_events,
            vblank_events: self.bus.profiling.vblank_events,
            rendered_scanlines: self.bus.profiling.rendered_scanlines,
            text_row_cache_requests: self.bus.ppu.profiling.text_row_cache_requests,
            text_row_cache_hits: self.bus.ppu.profiling.text_row_cache_hits,
            text_row_all_zero: self.bus.ppu.profiling.text_row_all_zero,
            timer_overflows: self.bus.profiling.timer_overflows,
            dma_starts: self.bus.profiling.dma_starts,
            dma_units: self.bus.profiling.dma_units,
        }
    }

    #[cfg(feature = "profiling")]
    pub fn reset_profiling(&mut self) {
        self.profiling_frames = 0;
        self.cpu.profiling = crate::hardware::profiling::CpuProfiling::default();
        self.bus.profiling = crate::hardware::profiling::BusProfiling::default();
        self.bus.ppu.profiling = crate::hardware::ppu::PpuProfiling::default();
    }
}

impl MachineTiming for Emulator {
    fn timing_snapshot(&self) -> TimingSnapshot {
        TimingSnapshot::new(MasterTicks::new(self.cpu.cycles), MASTER_CLOCK_RATE)
    }
}

impl Reset for Emulator {
    #[inline]
    fn reset(&mut self) {
        Emulator::reset(self);
    }
}

impl FrameLifecycle for Emulator {
    #[inline]
    fn step_frame(&mut self) {
        Emulator::step_frame(self);
    }

    #[inline]
    fn frame_count(&self) -> u64 {
        Emulator::frame_count(self)
    }
}
