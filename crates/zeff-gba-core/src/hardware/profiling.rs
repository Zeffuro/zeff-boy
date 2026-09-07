#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProfilingSnapshot {
    pub frames: u64,
    pub completed_instructions: u64,
    pub frame_cpu_runs: u64,
    pub frame_cpu_run_instructions: u64,
    pub frame_cpu_direct_runs: u64,
    pub frame_cpu_direct_instructions: u64,
    pub frame_cpu_direct_cycles: u64,
    pub frame_cpu_direct_kinds: [u64; 3],
    pub frame_scalar_arm: [u64; 11],
    pub frame_scalar_thumb: [u64; 17],
    pub frame_scalar_arm_halfword: [u64; 4],
    pub pure_opcode_requests: [u64; 2],
    pub pure_opcode_hits: [[u64; 2]; 3],
    pub cpu_phase_visits: [u64; 8],
    pub instruction_classes_arm: [u64; 11],
    pub instruction_classes_thumb: [u64; 17],
    pub frame_kernel_candidates: [u64; 4],
    pub frame_kernel_candidate_cycles: [u64; 4],
    pub frame_kernel_fetch_gates: [u64; 4],
    pub frame_kernel_fetch_eligible: [u64; 4],
    pub frame_kernel_fetch_eligible_cycles: u64,
    pub frame_kernel_quiet_instructions: u64,
    pub frame_kernel_quiet_runs: u64,
    pub frame_kernel_quiet_longest_run: u64,
    pub thumb_macro_counts: [u64; 7],
    pub thumb_macro_plain_halfwords: [u64; 2],
    pub thumb_macro_eligible: [u64; 7],
    pub thumb_macro_quiet: [u64; 7],
    pub thumb_macro_run_ends: [u64; 7],
    pub thumb_macro_neighbors: [[u64; 4]; 7],
    pub instruction_fetches: u64,
    pub cpu_generic_fetch_decode_calls: u64,
    pub cpu_gamepak_block_fetches: u64,
    pub cpu_ram_block_fetches: [u64; 2],
    pub instruction_fetch_modes: [u64; 2],
    pub instruction_fetch_accesses: [u64; 2],
    pub instruction_fetch_regions: [u64; 7],
    pub instruction_fetch_descriptor_compatible: u64,
    pub instruction_fetch_fallbacks: [u64; 5],
    pub instruction_fetch_waitcnt_changes: u64,
    pub bus_step_calls: u64,
    pub bus_requested_cycles: u64,
    pub bus_deferred_step_calls: u64,
    pub bus_deferred_cycles: u64,
    pub bus_service_entries: u64,
    pub bus_chunks: u64,
    pub apu_step_output_calls: u64,
    pub apu_step_output_cycles: u64,
    pub apu_non_observation_chunks: u64,
    pub apu_non_observation_cycles: u64,
    pub apu_ppu_only_non_observation_chunks: u64,
    pub apu_ppu_only_non_observation_cycles: u64,
    pub apu_deadline_materializations: u64,
    pub apu_timer_overflow_ordering_cases: u64,
    pub apu_timer_overflow_ordering_count: u64,
    pub frame_service_pending_spans: u64,
    pub frame_service_pending_cycles: u64,
    pub frame_service_pending_max_cycles: u32,
    pub frame_service_pending_span_buckets: [u64; 8],
    pub bus_deadline_hits: u64,
    pub bus_deadline_recomputes: u64,
    pub bus_deadline_expiries: [u64; 6],
    pub bus_deadline_invalidations: [u64; 3],
    pub visible_hblank_events: u64,
    pub vblank_events: u64,
    pub rendered_scanlines: u64,
    pub text_row_cache_requests: [u64; 2],
    pub text_row_cache_hits: [u64; 2],
    pub text_row_all_zero: [u64; 2],
    pub timer_overflows: [u64; 4],
    pub dma_starts: [u64; 4],
    pub dma_units: [u64; 4],
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CpuProfiling {
    pub pure_opcodes: PureOpcodeProbe,
    pub generic_fetch_decode_calls: u64,
    pub gamepak_block_fetches: u64,
    pub ram_block_fetches: [u64; 2],
    pub completed_instructions: u64,
    pub frame_runs: u64,
    pub frame_run_instructions: u64,
    pub frame_direct_runs: u64,
    pub frame_direct_instructions: u64,
    pub frame_direct_cycles: u64,
    pub frame_direct_kinds: [u64; 3],
    pub frame_scalar_arm: [u64; 11],
    pub frame_scalar_thumb: [u64; 17],
    pub frame_scalar_arm_halfword: [u64; 4],
    pub phase_visits: [u64; 8],
    pub instruction_classes_arm: [u64; 11],
    pub instruction_classes_thumb: [u64; 17],
    pub frame_kernel_candidates: [u64; 4],
    pub frame_kernel_candidate_cycles: [u64; 4],
    pub frame_kernel_fetch_gates: [u64; 4],
    pub frame_kernel_fetch_eligible: [u64; 4],
    pub frame_kernel_fetch_eligible_cycles: u64,
    pub frame_kernel_quiet_instructions: u64,
    pub frame_kernel_quiet_runs: u64,
    pub frame_kernel_quiet_longest_run: u64,
    pub frame_kernel_quiet_run_length: u64,
    pub frame_kernel_pending_eligible: bool,
    pub frame_kernel_entry_service_entries: u64,
    pub frame_kernel_last_service_entries: u64,
    pub thumb_macro_counts: [u64; 7],
    pub thumb_macro_plain_halfwords: [u64; 2],
    pub thumb_macro_eligible: [u64; 7],
    pub thumb_macro_quiet: [u64; 7],
    pub thumb_macro_run_ends: [u64; 7],
    pub thumb_macro_neighbors: [[u64; 4]; 7],
    pub thumb_macro_pending_kind: Option<usize>,
    pub thumb_macro_pending_eligible: bool,
    pub thumb_macro_entry_service_entries: u64,
    pub thumb_macro_previous_quiet: bool,
    pub thumb_macro_previous_service_entries: u64,
    pub thumb_macro_pending_neighbor: Option<(usize, usize)>,
    pub instruction_fetches: u64,
    pub instruction_fetch_modes: [u64; 2],
    pub instruction_fetch_accesses: [u64; 2],
    pub instruction_fetch_regions: [u64; 7],
    pub instruction_fetch_descriptor_compatible: u64,
    pub instruction_fetch_fallbacks: [u64; 5],
    pub instruction_fetch_waitcnt_changes: u64,
    pub last_instruction_fetch_waitcnt: Option<u16>,
}

/// Direct-mapped locality estimates, keyed by exact opcode and instruction set.
/// Heap storage stays bounded and is never architectural or serialized state.
#[derive(Clone, Debug)]
pub(crate) struct PureOpcodeProbe {
    tags: [Box<[u64]>; 3],
    pub requests: [u64; 2],
    pub hits: [[u64; 2]; 3],
}

impl Default for PureOpcodeProbe {
    fn default() -> Self {
        Self {
            tags: [64, 256, 1024].map(|size| vec![0; size].into_boxed_slice()),
            requests: [0; 2],
            hits: [[0; 2]; 3],
        }
    }
}

impl PureOpcodeProbe {
    pub fn record(&mut self, raw: u32, thumb: bool) {
        let isa = usize::from(thumb);
        // Zero denotes an unused slot; both raw zero and ARM/Thumb remain distinct.
        let key = (u64::from(raw) | (u64::from(thumb) << 32)) + 1;
        let hash = (key ^ (key >> 32)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.requests[isa] += 1;
        for (index, tags) in self.tags.iter_mut().enumerate() {
            let slot = (hash >> (64 - tags.len().ilog2())) as usize;
            self.hits[index][isa] += u64::from(tags[slot] == key);
            tags[slot] = key;
        }
    }
}

#[cfg(test)]
mod pure_opcode_tests {
    use super::PureOpcodeProbe;

    #[test]
    fn pure_opcode_probe_tracks_exact_tags_isa_and_independent_clones() {
        let mut probe = PureOpcodeProbe::default();
        probe.record(0, false);
        assert_eq!(probe.hits, [[0; 2]; 3]);
        probe.record(0, false);
        assert_eq!(probe.hits, [[1, 0]; 3]);
        probe.record(0, true);
        assert_eq!(probe.hits, [[1, 0]; 3]);
        probe.record(0, true);
        assert_eq!(probe.hits, [[1, 1]; 3]);
        let mut cloned = probe.clone();
        cloned.record(0, true);
        assert_eq!(probe.requests, [2, 2]);
        assert_eq!(cloned.requests, [2, 3]);
        assert_eq!(cloned.hits, [[1, 2]; 3]);
        probe = PureOpcodeProbe::default();
        probe.record(0, true);
        assert_eq!(probe.hits, [[0; 2]; 3]);
        assert_eq!(
            probe.tags.iter().map(|tags| tags.len()).sum::<usize>(),
            1344
        );
    }

    #[test]
    fn pure_opcode_probe_collisions_never_count_as_hits() {
        let mut probe = PureOpcodeProbe::default();
        for raw in 0..2048 {
            probe.record(raw, false);
        }
        assert_eq!(probe.requests, [2048, 0]);
        assert_eq!(probe.hits, [[0; 2]; 3]);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct BusProfiling {
    pub step_calls: u64,
    pub requested_cycles: u64,
    pub deferred_step_calls: u64,
    pub deferred_cycles: u64,
    pub service_entries: u64,
    pub chunks: u64,
    pub apu_step_output_calls: u64,
    pub apu_step_output_cycles: u64,
    pub apu_non_observation_chunks: u64,
    pub apu_non_observation_cycles: u64,
    pub apu_ppu_only_non_observation_chunks: u64,
    pub apu_ppu_only_non_observation_cycles: u64,
    pub apu_deadline_materializations: u64,
    pub apu_timer_overflow_ordering_cases: u64,
    pub apu_timer_overflow_ordering_count: u64,
    pub frame_service_pending_spans: u64,
    pub frame_service_pending_cycles: u64,
    pub frame_service_pending_max_cycles: u32,
    pub frame_service_pending_span_buckets: [u64; 8],
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
