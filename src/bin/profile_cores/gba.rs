use super::{print_accuracy_hashes, profile_frames_with_prepare};

pub(super) fn profile_synthetic(
    frames: u32,
    sample_generation_enabled: bool,
    instruction_trace_enabled: bool,
    suffix: &str,
) {
    for (label, rom) in [
        ("GBA synthetic", gba_rom()),
        ("GBA RAM writes", gba_write_rom()),
    ] {
        let mut gba =
            zeff_gba_core::emulator::Emulator::from_rom_data(&rom).expect("synthetic GBA ROM");
        gba.set_apu_sample_generation_enabled(sample_generation_enabled);
        gba.set_apu_debug_capture_enabled(false);
        gba.set_instruction_trace_enabled(instruction_trace_enabled);
        profile_frames(&format!("{label}{suffix}"), frames, &mut gba);

        let state = gba.encode_state().expect("encode synthetic GBA state");
        let mut audio = Vec::new();
        gba.drain_audio_samples_into(&mut audio);
        print_accuracy_hashes(gba.framebuffer(), &state, &audio);
    }
}

pub(super) fn profile_frames(
    label: &str,
    frames: u32,
    machine: &mut zeff_gba_core::emulator::Emulator,
) {
    profile_frames_with_prepare(label, frames, machine, |machine| machine.reset_profiling());
    let snapshot = machine.profiling_snapshot();
    println!(
        "  GBA direct pure opcode locality ARM/Thumb requests {:?}; direct-mapped 64/256/1024 hits {:?} (exact raw+ISA; excludes condition-failed non-pure opcodes)",
        snapshot.pure_opcode_requests, snapshot.pure_opcode_hits,
    );
    println!(
        "  GBA work: {} frames  {} instructions  {} CPU runs / {} CPU-run instructions  {} bus calls  {} deferred calls / {} deferred cycles  {} service entries  {} chunks  {} cycles",
        snapshot.frames,
        snapshot.completed_instructions,
        snapshot.frame_cpu_runs,
        snapshot.frame_cpu_run_instructions,
        snapshot.bus_step_calls,
        snapshot.bus_deferred_step_calls,
        snapshot.bus_deferred_cycles,
        snapshot.bus_service_entries,
        snapshot.bus_chunks,
        snapshot.bus_requested_cycles,
    );
    println!(
        "  GBA direct CPU: {} runs / {} instructions / {} cycles  pure/transfer/branch {:?}",
        snapshot.frame_cpu_direct_runs,
        snapshot.frame_cpu_direct_instructions,
        snapshot.frame_cpu_direct_cycles,
        snapshot.frame_cpu_direct_kinds,
    );
    println!(
        "  GBA phases: {:?}  scanlines {}  HBlank {}  VBlank {}  timer {:?}  DMA {:?}/{:?}",
        snapshot.cpu_phase_visits,
        snapshot.rendered_scanlines,
        snapshot.visible_hblank_events,
        snapshot.vblank_events,
        snapshot.timer_overflows,
        snapshot.dma_starts,
        snapshot.dma_units,
    );
    println!(
        "  GBA scalar-completed ARM classes {:?}  Thumb classes {:?}  ARM halfword subtype reserved/half/signed-byte/signed-half {:?}",
        snapshot.frame_scalar_arm, snapshot.frame_scalar_thumb, snapshot.frame_scalar_arm_halfword,
    );
    println!(
        "  GBA text-row hypothetical 32-slot cache requests/hits 4bpp {}/{}  8bpp {}/{}",
        snapshot.text_row_cache_requests[0],
        snapshot.text_row_cache_hits[0],
        snapshot.text_row_cache_requests[1],
        snapshot.text_row_cache_hits[1],
    );
    println!(
        "  GBA text-row all-zero decoded rows 4bpp {}  8bpp {}",
        snapshot.text_row_all_zero[0], snapshot.text_row_all_zero[1],
    );
    println!(
        "  GBA ARM classes BX/B/block/single/DP/mul/mull/swap/SWI/coproc/unknown: {:?}",
        snapshot.instruction_classes_arm,
    );
    println!(
        "  GBA Thumb classes shift/addsub/imm/ALU/hi/PCload/loadstore/half/SPload/address/SPadd/pushpop/multi/cond/B/BL/unknown: {:?}",
        snapshot.instruction_classes_thumb,
    );
    println!(
        "  GBA Thumb macros cond-taken/cond-untaken/SWI/reserved/B/LDRH/STRH: {:?}",
        snapshot.thumb_macro_counts,
    );
    println!(
        "  GBA Thumb macro gates: plain LDRH/STRH {:?}  fetch/data/refill-safe {:?}  observed quiet {:?}  direct-run stops {:?}",
        snapshot.thumb_macro_plain_halfwords,
        snapshot.thumb_macro_eligible,
        snapshot.thumb_macro_quiet,
        snapshot.thumb_macro_run_ends,
    );
    println!(
        "  GBA Thumb macro quiet neighbors none/left/right/both (same class order; pure-Thumb + eligible macros, coverage estimate): {:?}",
        snapshot.thumb_macro_neighbors,
    );
    println!(
        "  GBA stateless candidates failed-ARM/ARM-DP/ARM-regshift/Thumb-ALU: {:?}  cycles {:?}",
        snapshot.frame_kernel_candidates, snapshot.frame_kernel_candidate_cycles,
    );
    println!(
        "  GBA stateless fetch gates eligible/eager/cold/unsafe: {:?}  fetch-safe {:?} / {} cycles (upper bound before horizon/guard)",
        snapshot.frame_kernel_fetch_gates,
        snapshot.frame_kernel_fetch_eligible,
        snapshot.frame_kernel_fetch_eligible_cycles,
    );
    println!(
        "  GBA stateless quiet runs: {} instructions / {} runs  longest {}  upper-bound avoided cycle calls {}",
        snapshot.frame_kernel_quiet_instructions,
        snapshot.frame_kernel_quiet_runs,
        snapshot.frame_kernel_quiet_longest_run,
        snapshot
            .frame_kernel_quiet_instructions
            .saturating_sub(snapshot.frame_kernel_quiet_runs),
    );
    println!(
        "  GBA deadlines: {} hits  {} recomputes  expiries PPU/IRQ/T0/T1/T2/T3 {:?}  invalidations timer/IRQ/load {:?}",
        snapshot.bus_deadline_hits,
        snapshot.bus_deadline_recomputes,
        snapshot.bus_deadline_expiries,
        snapshot.bus_deadline_invalidations,
    );
    println!(
        "  GBA APU topology: {} step_output calls / {} cycles  {} non-observation chunks / {} cycles  {} PPU-only non-observation chunks / {} cycles  {} APU-deadline materializations  {} timer-overflow order cases / {} overflows",
        snapshot.apu_step_output_calls,
        snapshot.apu_step_output_cycles,
        snapshot.apu_non_observation_chunks,
        snapshot.apu_non_observation_cycles,
        snapshot.apu_ppu_only_non_observation_chunks,
        snapshot.apu_ppu_only_non_observation_cycles,
        snapshot.apu_deadline_materializations,
        snapshot.apu_timer_overflow_ordering_cases,
        snapshot.apu_timer_overflow_ordering_count,
    );
    println!(
        "  GBA deferred pending spans: {} spans / {} cycles  max {}  log2 buckets 1,2-3,4-7,8-15,16-31,32-63,64-127,>=128 {:?}",
        snapshot.frame_service_pending_spans,
        snapshot.frame_service_pending_cycles,
        snapshot.frame_service_pending_max_cycles,
        snapshot.frame_service_pending_span_buckets,
    );
    println!(
        "  GBA fetch: {} total  ARM/Thumb {:?}  nonseq/seq {:?}  regions BIOS/EWRAM/IWRAM/GP0/GP1/GP2/other {:?}",
        snapshot.instruction_fetches,
        snapshot.instruction_fetch_modes,
        snapshot.instruction_fetch_accesses,
        snapshot.instruction_fetch_regions,
    );
    println!(
        "  GBA fetch orchestration: {} generic calls  {} GamePak block fetches  EWRAM/IWRAM block fetches {:?}",
        snapshot.cpu_generic_fetch_decode_calls,
        snapshot.cpu_gamepak_block_fetches,
        snapshot.cpu_ram_block_fetches,
    );
    println!(
        "  GBA fetch gate: {} compatible  fallbacks EEPROM/RTC/open-bus/unsupported/debug {:?}  WAITCNT changes {}",
        snapshot.instruction_fetch_descriptor_compatible,
        snapshot.instruction_fetch_fallbacks,
        snapshot.instruction_fetch_waitcnt_changes,
    );
}

pub(super) fn profile_active_video(frames: u32, sample_generation_enabled: bool) {
    let mut gba =
        zeff_gba_core::emulator::Emulator::from_rom_data(&gba_rom()).expect("synthetic GBA ROM");
    let mut pattern = 0xA5A5_5A5A_u32;
    for offset in (0..0x4000_u32).step_by(2) {
        pattern = pattern.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        gba.cpu_write16(0x0600_0000 + offset, (pattern >> 16) as u16);
    }
    for index in 0..(32 * 32_u32) {
        let tile = (index * 13) & 0x01FF;
        let attributes = ((index & 0x0F) << 12) | ((index & 1) << 10) | ((index & 2) << 10);
        gba.cpu_write16(0x0600_4000 + index * 2, (tile | attributes) as u16);
    }
    for color in 0..256_u32 {
        let r = color & 0x1F;
        let g = (color * 3) & 0x1F;
        let b = (color * 7) & 0x1F;
        gba.cpu_write16(0x0500_0000 + color * 2, (r | (g << 5) | (b << 10)) as u16);
    }

    gba.cpu_write16(0x0200_0000, 0x03FF);
    gba.cpu_write32(0x0400_00B0, 0x0200_0000);
    gba.cpu_write32(0x0400_00B4, 0x0500_0002);
    gba.cpu_write16(0x0400_00B8, 1);
    gba.cpu_write16(0x0400_00BA, 0xA340);
    gba.cpu_write16(0x0400_0100, 0xFFC0);
    gba.cpu_write16(0x0400_0102, 0x0081);
    gba.cpu_write16(0x0400_0104, 0xFFF0);
    gba.cpu_write16(0x0400_0106, 0x0084);
    gba.cpu_write16(0x0400_0008, 8 << 8);
    gba.cpu_write16(0x0400_0000, 1 << 8);

    gba.set_apu_sample_generation_enabled(sample_generation_enabled);
    gba.set_apu_debug_capture_enabled(false);
    profile_frames(
        if sample_generation_enabled {
            "GBA active video + DMA + timers + audio"
        } else {
            "GBA active video + DMA + timers"
        },
        frames,
        &mut gba,
    );
    assert_eq!(
        gba.cpu_peek16(0x0500_0002),
        0x03FF,
        "synthetic GBA HBlank DMA did not update palette RAM"
    );
    let first_pixel = &gba.framebuffer()[..4];
    assert!(
        gba.framebuffer()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel != first_pixel),
        "synthetic GBA active-video fixture produced a flat frame"
    );
    let state = gba.encode_state().expect("encode GBA active-video state");
    let mut audio = Vec::new();
    gba.drain_audio_samples_into(&mut audio);
    print_accuracy_hashes(gba.framebuffer(), &state, &audio);
}

fn gba_rom() -> Vec<u8> {
    let mut rom = vec![0; 0xC0];
    rom[..4].copy_from_slice(&0xEAFF_FFFE_u32.to_le_bytes());
    rom[0xA0..0xA7].copy_from_slice(b"PROFILE");
    rom[0xB2] = 0x96;
    rom
}

fn gba_write_rom() -> Vec<u8> {
    let mut rom = gba_rom();
    for (offset, instruction) in [0xE3A0_0402_u32, 0xE580_1000, 0xE281_1001, 0xEAFF_FFFC]
        .into_iter()
        .enumerate()
    {
        let start = offset * 4;
        rom[start..start + 4].copy_from_slice(&instruction.to_le_bytes());
    }
    rom
}
