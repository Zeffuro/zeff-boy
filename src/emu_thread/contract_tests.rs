use crate::audio_tooling::AudioChannelId;

use super::FrameResult;

pub(super) const SMS_AUDIO_ROM: &[u8] = &[
    0x3E, 0x80, 0xD3, 0x7F, // Tone 0 low period.
    0x3E, 0x04, 0xD3, 0x7F, // Tone 0 high period.
    0x3E, 0x90, 0xD3, 0x7F, // Tone 0 full volume.
    0x76, // Halt while the PSG continues.
];

pub(super) fn gba_test_rom() -> Vec<u8> {
    let mut rom = vec![0; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xAC..0xB0].copy_from_slice(b"BPEE");
    rom[0xB0..0xB2].copy_from_slice(b"01");
    rom[0xB2] = 0x96;
    rom.extend_from_slice(b"SRAM_V113");
    rom
}

pub(super) fn gba_rtc() -> zeff_gba_core::hardware::cartridge::RtcDateTime {
    zeff_gba_core::hardware::cartridge::RtcDateTime::new(2032, 9, 17, 5, [21, 43, 19]).unwrap()
}

pub(super) fn gba_sram_bytes(len: usize) -> Vec<u8> {
    (0..len)
        .map(|index| (index as u8).wrapping_mul(17).wrapping_add(3))
        .collect()
}

pub(super) fn assert_frame_results_match(left: &FrameResult, right: &FrameResult) {
    assert_eq!(left.advanced_frames, right.advanced_frames);
    assert_eq!(left.delivery_merged, right.delivery_merged);
    assert_eq!(left.replay_events, right.replay_events);
    assert_eq!(left.replay_error, right.replay_error);
    assert_eq!(left.runtime_fault, right.runtime_fault);
    assert_eq!(left.rumble, right.rumble);
    assert_f32_equal("PCM", &left.audio_samples, &right.audio_samples);
    assert_eq!(left.audio_playback_speed, right.audio_playback_speed);
    assert_eq!(left.is_mbc7, right.is_mbc7);
    assert_eq!(left.is_pocket_camera, right.is_pocket_camera);
    assert_eq!(left.game_boy_serial_device, right.game_boy_serial_device);
    assert_eq!(
        left.game_boy_printer_jobs.len(),
        right.game_boy_printer_jobs.len()
    );
    assert_eq!(left.media_slot_snapshot, right.media_slot_snapshot);
    assert_eq!(left.rewind_fill.to_bits(), right.rewind_fill.to_bits());
    assert_eq!(left.audio_semantic_frames, right.audio_semantic_frames);
    assert_eq!(
        left.audio_timeline_discontinuities,
        right.audio_timeline_discontinuities
    );
    assert_eq!(left.ui_data.core_features, right.ui_data.core_features);
    assert_eq!(
        left.ui_data.cpu_debug.is_some(),
        right.ui_data.cpu_debug.is_some()
    );
    assert_eq!(
        left.ui_data.perf_info.is_some(),
        right.ui_data.perf_info.is_some()
    );
    assert_eq!(
        left.ui_data.apu_debug.is_some(),
        right.ui_data.apu_debug.is_some()
    );
    assert_eq!(
        left.ui_data.oam_debug.is_some(),
        right.ui_data.oam_debug.is_some()
    );
    assert_eq!(
        left.ui_data.palette_debug.is_some(),
        right.ui_data.palette_debug.is_some()
    );
    assert_eq!(
        left.ui_data.rom_debug.is_some(),
        right.ui_data.rom_debug.is_some()
    );
    assert_eq!(
        left.ui_data.input_debug.is_some(),
        right.ui_data.input_debug.is_some()
    );
    assert_eq!(
        left.ui_data.graphics_data.is_some(),
        right.ui_data.graphics_data.is_some()
    );
    assert_eq!(
        left.ui_data.disassembly_view.is_some(),
        right.ui_data.disassembly_view.is_some()
    );
    assert_eq!(left.ui_data.memory_page, right.ui_data.memory_page);
    assert_eq!(
        left.ui_data.memory_search_results.is_some(),
        right.ui_data.memory_search_results.is_some()
    );
    assert_eq!(left.ui_data.rom_page, right.ui_data.rom_page);
    assert_eq!(left.ui_data.rom_size, right.ui_data.rom_size);
    assert_eq!(
        left.ui_data.rom_search_results.is_some(),
        right.ui_data.rom_search_results.is_some()
    );
    assert_eq!(
        left.ui_data.instruction_trace.is_some(),
        right.ui_data.instruction_trace.is_some()
    );
}

pub(super) fn assert_bytes_equal(label: &str, left: &[u8], right: &[u8]) {
    assert_eq!(left.len(), right.len(), "{label} length");
    assert!(
        left == right,
        "{label} content differs: left={:016X} right={:016X}",
        byte_signature(left),
        byte_signature(right)
    );
}

pub(super) fn byte_signature(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

pub(super) fn assert_f32_equal(label: &str, left: &[f32], right: &[f32]) {
    assert_eq!(left.len(), right.len(), "{label} length");
    let exact = left
        .iter()
        .zip(right)
        .all(|(left, right)| left.to_bits() == right.to_bits());
    assert!(
        exact,
        "{label} content differs: left={:016X} right={:016X}",
        f32_signature(left),
        f32_signature(right)
    );
}

pub(super) fn f32_signature(samples: &[f32]) -> u64 {
    samples.iter().fold(0xcbf2_9ce4_8422_2325, |hash, sample| {
        (hash ^ u64::from(sample.to_bits())).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

pub(super) fn assert_rich_ui_data_match(left: &FrameResult, right: &FrameResult) {
    assert!(left.ui_data.core_features.is_some());
    let left_cpu = left.ui_data.cpu_debug.as_ref().expect("CPU debug data");
    let right_cpu = right.ui_data.cpu_debug.as_ref().expect("CPU debug data");
    assert_eq!(left_cpu.register_lines, right_cpu.register_lines);
    assert!(!left_cpu.register_lines.is_empty());
    assert_eq!(left_cpu.flags, right_cpu.flags);
    assert_eq!(left_cpu.status_text, right_cpu.status_text);
    assert_eq!(left_cpu.cpu_state, right_cpu.cpu_state);
    assert_eq!(left_cpu.pc, right_cpu.pc);
    assert_eq!(left_cpu.cycles, right_cpu.cycles);
    assert_eq!(left_cpu.last_opcode_line, right_cpu.last_opcode_line);
    assert_eq!(left_cpu.sections, right_cpu.sections);
    assert_eq!(left_cpu.io_registers, right_cpu.io_registers);
    assert_eq!(
        left_cpu.recent_opcodes.len(),
        right_cpu.recent_opcodes.len()
    );
    assert!(!left_cpu.recent_opcodes.is_empty());
    for (left_op, right_op) in left_cpu
        .recent_opcodes
        .iter()
        .zip(&right_cpu.recent_opcodes)
    {
        assert_eq!(left_op.address, right_op.address);
        assert_eq!(left_op.storage_offset, right_op.storage_offset);
        assert_bytes_equal("recent opcode", &left_op.bytes, &right_op.bytes);
        assert_eq!(left_op.detail, right_op.detail);
        assert_eq!(left_op.repeat_count, right_op.repeat_count);
        assert_eq!(left_op.thumb, right_op.thumb);
    }
    assert_eq!(left_cpu.call_stack.len(), right_cpu.call_stack.len());
    for (left_call, right_call) in left_cpu.call_stack.iter().zip(&right_cpu.call_stack) {
        assert_eq!(left_call.target, right_call.target);
        assert_eq!(left_call.return_address, right_call.return_address);
        assert_eq!(left_call.target_rom_offset, right_call.target_rom_offset);
        assert_eq!(left_call.return_rom_offset, right_call.return_rom_offset);
        assert_eq!(left_call.kind, right_call.kind);
    }
    assert_eq!(
        left_cpu.call_stack_available,
        right_cpu.call_stack_available
    );
    assert_eq!(left_cpu.breakpoints, right_cpu.breakpoints);
    assert_eq!(
        left_cpu.one_shot_breakpoints,
        right_cpu.one_shot_breakpoints
    );
    assert_eq!(
        left_cpu.breakpoint_hit_conditions,
        right_cpu.breakpoint_hit_conditions
    );
    assert_eq!(left_cpu.supported_events, right_cpu.supported_events);
    assert_eq!(left_cpu.event_breakpoints, right_cpu.event_breakpoints);
    assert_eq!(left_cpu.rom_breakpoints, right_cpu.rom_breakpoints);
    assert_eq!(left_cpu.watchpoints.len(), right_cpu.watchpoints.len());
    for (left_watch, right_watch) in left_cpu.watchpoints.iter().zip(&right_cpu.watchpoints) {
        assert_eq!(left_watch.address, right_watch.address);
        assert_eq!(left_watch.end_address, right_watch.end_address);
        assert_eq!(left_watch.watch_type, right_watch.watch_type);
    }
    assert_eq!(left_cpu.hit_breakpoint, right_cpu.hit_breakpoint);
    assert_eq!(left_cpu.hit_rom_breakpoint, right_cpu.hit_rom_breakpoint);
    match (&left_cpu.hit_watchpoint, &right_cpu.hit_watchpoint) {
        (None, None) => {}
        (Some(left_hit), Some(right_hit)) => {
            assert_eq!(left_hit.address, right_hit.address);
            assert_eq!(left_hit.old_value, right_hit.old_value);
            assert_eq!(left_hit.new_value, right_hit.new_value);
            assert_eq!(left_hit.watch_type, right_hit.watch_type);
        }
        _ => panic!("watchpoint hit presence differs"),
    }
    assert_eq!(left_cpu.hit_event, right_cpu.hit_event);

    let left_apu = left.ui_data.apu_debug.as_ref().expect("APU data");
    let right_apu = right.ui_data.apu_debug.as_ref().expect("APU data");
    assert_eq!(left_apu.extra_sections, right_apu.extra_sections);

    let left_perf = left.ui_data.perf_info.as_ref().expect("performance data");
    let right_perf = right.ui_data.perf_info.as_ref().expect("performance data");
    assert_eq!(left_perf.fps.to_bits(), right_perf.fps.to_bits());
    assert_eq!(
        left_perf.target_fps.to_bits(),
        right_perf.target_fps.to_bits()
    );
    assert_eq!(left_perf.speed_mode_label, right_perf.speed_mode_label);
    assert_eq!(left_perf.frames_in_flight, right_perf.frames_in_flight);
    assert_eq!(left_perf.cycles, right_perf.cycles);
    assert_eq!(left_perf.platform_name, right_perf.platform_name);
    assert_eq!(left_perf.hardware_label, right_perf.hardware_label);
    assert_eq!(
        left_perf.hardware_pref_label,
        right_perf.hardware_pref_label
    );

    let left_oam = left.ui_data.oam_debug.as_ref().expect("OAM data");
    let right_oam = right.ui_data.oam_debug.as_ref().expect("OAM data");
    assert_eq!(left_oam.headers, right_oam.headers);
    assert_eq!(left_oam.rows, right_oam.rows);
    assert!(!left_oam.rows.is_empty());

    let left_palette = left.ui_data.palette_debug.as_ref().expect("palette data");
    let right_palette = right.ui_data.palette_debug.as_ref().expect("palette data");
    assert_eq!(left_palette.groups.len(), right_palette.groups.len());
    assert!(!left_palette.groups.is_empty());
    for (left_group, right_group) in left_palette.groups.iter().zip(&right_palette.groups) {
        assert_eq!(left_group.title, right_group.title);
        assert_eq!(left_group.rows.len(), right_group.rows.len());
        assert!(!left_group.rows.is_empty());
        for (left_row, right_row) in left_group.rows.iter().zip(&right_group.rows) {
            assert_eq!(left_row.label, right_row.label);
            assert_eq!(left_row.colors, right_row.colors);
            assert!(!left_row.colors.is_empty());
        }
    }

    let left_rom = left.ui_data.rom_debug.as_ref().expect("ROM data");
    let right_rom = right.ui_data.rom_debug.as_ref().expect("ROM data");
    assert_eq!(left_rom.sections.len(), right_rom.sections.len());
    assert!(!left_rom.sections.is_empty());
    for (left_section, right_section) in left_rom.sections.iter().zip(&right_rom.sections) {
        assert_eq!(left_section.heading, right_section.heading);
        assert_eq!(left_section.fields, right_section.fields);
        assert!(!left_section.fields.is_empty());
    }

    match (
        left.ui_data.input_debug.as_ref(),
        right.ui_data.input_debug.as_ref(),
    ) {
        (Some(left_input), Some(right_input)) => {
            assert_eq!(left_input.sections, right_input.sections);
            assert!(!left_input.sections.is_empty());
            assert_eq!(
                left_input.progress_bars.len(),
                right_input.progress_bars.len()
            );
            for ((left_name, left_value), (right_name, right_value)) in left_input
                .progress_bars
                .iter()
                .zip(&right_input.progress_bars)
            {
                assert_eq!(left_name, right_name);
                assert_eq!(left_value.to_bits(), right_value.to_bits());
            }
        }
        (None, None) => {}
        _ => panic!("input data presence differs"),
    }

    match (
        left.ui_data.graphics_data.as_ref(),
        right.ui_data.graphics_data.as_ref(),
    ) {
        (
            Some(crate::debug::ConsoleGraphicsData::Sega8(left)),
            Some(crate::debug::ConsoleGraphicsData::Sega8(right)),
        ) => {
            assert_eq!(left.system, right.system);
            assert_bytes_equal("VRAM", &left.vram, &right.vram);
            assert_bytes_equal("CRAM", &left.cram, &right.cram);
            assert_bytes_equal("OAM", &left.oam, &right.oam);
            assert!(!left.vram.is_empty());
            assert!(!left.cram.is_empty());
            assert!(!left.oam.is_empty());
            assert_eq!(left.registers, right.registers);
            assert_eq!(left.status, right.status);
            assert_eq!(left.address, right.address);
            assert_eq!(left.code, right.code);
            assert_eq!(left.v_counter, right.v_counter);
            assert_eq!(left.h_counter, right.h_counter);
            assert_eq!(left.scanline, right.scanline);
            assert_eq!(left.scanline_cycle, right.scanline_cycle);
            assert_eq!(left.line_counter, right.line_counter);
            assert_eq!(left.frame_interrupt_enabled, right.frame_interrupt_enabled);
            assert_eq!(left.line_interrupt_enabled, right.line_interrupt_enabled);
            assert_eq!(left.interrupt_pending, right.interrupt_pending);
            assert_eq!(left.line_interrupt_pending, right.line_interrupt_pending);
            assert_eq!(left.display_enabled, right.display_enabled);
            assert_eq!(left.tms9918_mode, right.tms9918_mode);
            assert_eq!(left.sprite_table_base, right.sprite_table_base);
            assert_eq!(left.mode4, right.mode4);
            assert_eq!(left.tms9918, right.tms9918);
        }
        (
            Some(crate::debug::ConsoleGraphicsData::Gba(left)),
            Some(crate::debug::ConsoleGraphicsData::Gba(right)),
        ) => {
            assert_bytes_equal("GBA VRAM", &left.vram, &right.vram);
            assert_bytes_equal("GBA palette RAM", &left.palette_ram, &right.palette_ram);
            assert_bytes_equal("GBA OAM", &left.oam, &right.oam);
            assert!(!left.vram.is_empty());
            assert!(!left.palette_ram.is_empty());
            assert!(!left.oam.is_empty());
            assert_eq!(left.ppu, right.ppu);
        }
        _ => panic!("graphics data family differs"),
    }

    let left_disassembly = left
        .ui_data
        .disassembly_view
        .as_ref()
        .expect("disassembly data");
    let right_disassembly = right
        .ui_data
        .disassembly_view
        .as_ref()
        .expect("disassembly data");
    assert_eq!(left_disassembly.pc, right_disassembly.pc);
    assert_eq!(left_disassembly.mapping, right_disassembly.mapping);
    assert_eq!(
        left_disassembly.is_navigation_target,
        right_disassembly.is_navigation_target
    );
    assert_eq!(
        left_disassembly.is_static_target,
        right_disassembly.is_static_target
    );
    assert_eq!(
        left_disassembly.location_symbol,
        right_disassembly.location_symbol
    );
    assert_eq!(left_disassembly.lines.len(), right_disassembly.lines.len());
    assert!(!left_disassembly.lines.is_empty());
    for (left_line, right_line) in left_disassembly.lines.iter().zip(&right_disassembly.lines) {
        assert_eq!(left_line.address, right_line.address);
        assert_eq!(left_line.storage_offset, right_line.storage_offset);
        assert_eq!(left_line.bank, right_line.bank);
        assert_eq!(left_line.symbol, right_line.symbol);
        assert_eq!(left_line.control_target, right_line.control_target);
        assert_eq!(
            left_line.control_target_storage,
            right_line.control_target_storage
        );
        assert_eq!(
            left_line.control_target_bank,
            right_line.control_target_bank
        );
        assert_eq!(
            left_line.control_target_symbol,
            right_line.control_target_symbol
        );
        assert_eq!(left_line.source, right_line.source);
        assert_bytes_equal("disassembly bytes", &left_line.bytes, &right_line.bytes);
        assert_eq!(left_line.mnemonic, right_line.mnemonic);
    }
    assert_eq!(left_disassembly.breakpoints, right_disassembly.breakpoints);
    assert_eq!(
        left_disassembly.one_shot_breakpoints,
        right_disassembly.one_shot_breakpoints
    );
    assert_eq!(
        left_disassembly.rom_breakpoints,
        right_disassembly.rom_breakpoints
    );
    assert_eq!(
        left_disassembly.hit_rom_breakpoint,
        right_disassembly.hit_rom_breakpoint
    );

    let left_memory = left.ui_data.memory_page.as_ref().expect("memory page");
    let right_memory = right.ui_data.memory_page.as_ref().expect("memory page");
    assert_eq!(left_memory, right_memory);
    assert!(!left_memory.is_empty());
    let left_memory_search = left
        .ui_data
        .memory_search_results
        .as_ref()
        .expect("memory search");
    let right_memory_search = right
        .ui_data
        .memory_search_results
        .as_ref()
        .expect("memory search");
    assert_eq!(left_memory_search.len(), right_memory_search.len());
    assert!(!left_memory_search.is_empty());
    for (left_match, right_match) in left_memory_search.iter().zip(right_memory_search) {
        assert_eq!(left_match.address, right_match.address);
        assert_bytes_equal(
            "memory search match",
            &left_match.matched_bytes,
            &right_match.matched_bytes,
        );
    }

    let left_rom_page = left.ui_data.rom_page.as_ref().expect("ROM page");
    let right_rom_page = right.ui_data.rom_page.as_ref().expect("ROM page");
    assert_eq!(left_rom_page, right_rom_page);
    assert!(!left_rom_page.is_empty());
    assert_eq!(left.ui_data.rom_size, right.ui_data.rom_size);
    assert!(left.ui_data.rom_size > 0);
    let left_rom_search = left
        .ui_data
        .rom_search_results
        .as_ref()
        .expect("ROM search");
    let right_rom_search = right
        .ui_data
        .rom_search_results
        .as_ref()
        .expect("ROM search");
    assert_eq!(left_rom_search.len(), right_rom_search.len());
    assert!(!left_rom_search.is_empty());
    for (left_match, right_match) in left_rom_search.iter().zip(right_rom_search) {
        assert_eq!(left_match.offset, right_match.offset);
        assert_bytes_equal(
            "ROM search match",
            &left_match.matched_bytes,
            &right_match.matched_bytes,
        );
    }

    let left_trace = left
        .ui_data
        .instruction_trace
        .as_ref()
        .expect("instruction trace");
    let right_trace = right
        .ui_data
        .instruction_trace
        .as_ref()
        .expect("instruction trace");
    assert!(left_trace.enabled);
    assert_eq!(left_trace.enabled, right_trace.enabled);
    assert_eq!(left_trace.capacity, right_trace.capacity);
    assert_eq!(left_trace.retained, right_trace.retained);
    assert!(left_trace.retained > 0);
    assert_eq!(left_trace.oldest_sequence, right_trace.oldest_sequence);
    assert_eq!(left_trace.newest_sequence, right_trace.newest_sequence);
    assert_eq!(left_trace.entries, right_trace.entries);
    assert!(!left_trace.entries.is_empty());
}

pub(super) fn assert_active_audio_results_match(left: &FrameResult, right: &FrameResult) {
    assert_frame_results_match(left, right);
    assert_rich_ui_data_match(left, right);
    assert!(left.ui_data.input_debug.is_some());
    assert!(!left.audio_samples.is_empty());
    assert_eq!(left.audio_samples.len() % 2, 0);
    assert!(left.audio_samples.iter().any(|sample| sample.abs() > 0.05));
    assert_eq!(left.audio_semantic_frames.len(), 1);
    let tone_0 = left.audio_semantic_frames[0]
        .voices
        .iter()
        .find(|voice| voice.channel == AudioChannelId(0))
        .expect("semantic frame should contain PSG tone 0");
    assert!(tone_0.active);
    assert!(tone_0.pitch_hz.is_some_and(|pitch| pitch > 0.0));
    assert!(tone_0.level.is_some_and(|level| level > 0.0));
    assert!(left.audio_timeline_discontinuities.is_empty());

    let left_apu = left
        .ui_data
        .apu_debug
        .as_ref()
        .expect("active APU capture should publish debug data");
    let right_apu = right
        .ui_data
        .apu_debug
        .as_ref()
        .expect("active APU capture should publish debug data");
    assert_eq!(left_apu.master_lines, right_apu.master_lines);
    assert_f32_equal(
        "APU master waveform",
        &left_apu.master_waveform,
        &right_apu.master_waveform,
    );
    assert_eq!(left_apu.master_waveform.len(), 512);
    assert!(
        left_apu
            .master_waveform
            .iter()
            .any(|sample| sample.abs() > 0.05)
    );
    assert_eq!(left_apu.channels.len(), right_apu.channels.len());
    for (left_channel, right_channel) in left_apu.channels.iter().zip(&right_apu.channels) {
        assert_eq!(left_channel.name, right_channel.name);
        assert_eq!(left_channel.enabled, right_channel.enabled);
        assert_eq!(left_channel.muted, right_channel.muted);
        assert_eq!(left_channel.register_lines, right_channel.register_lines);
        assert_eq!(left_channel.detail_line, right_channel.detail_line);
        assert_f32_equal(
            "APU channel waveform",
            &left_channel.waveform,
            &right_channel.waveform,
        );
    }
    assert!(left_apu.channels[0].enabled);
    assert_eq!(left_apu.channels[0].waveform.len(), 512);
    assert!(
        left_apu.channels[0]
            .waveform
            .iter()
            .any(|sample| sample.abs() > 0.05)
    );
}

pub(super) fn assert_gba_results_match(left: &FrameResult, right: &FrameResult) {
    assert_frame_results_match(left, right);
    assert_rich_ui_data_match(left, right);
    assert!(left.ui_data.input_debug.is_none());
    assert!(!left.audio_samples.is_empty());
    assert_eq!(left.audio_samples.len() % 2, 0);
    assert_eq!(left.audio_semantic_frames.len(), 1);
    assert!(left.audio_timeline_discontinuities.is_empty());

    let left_apu = left.ui_data.apu_debug.as_ref().expect("GBA APU data");
    let right_apu = right.ui_data.apu_debug.as_ref().expect("GBA APU data");
    assert_eq!(left_apu.master_lines, right_apu.master_lines);
    assert_f32_equal(
        "GBA APU master waveform",
        &left_apu.master_waveform,
        &right_apu.master_waveform,
    );
    assert_eq!(left_apu.master_waveform.len(), 512);
    assert_eq!(left_apu.channels.len(), right_apu.channels.len());
    assert_eq!(left_apu.channels.len(), 6);
    for (left_channel, right_channel) in left_apu.channels.iter().zip(&right_apu.channels) {
        assert_eq!(left_channel.name, right_channel.name);
        assert_eq!(left_channel.enabled, right_channel.enabled);
        assert_eq!(left_channel.muted, right_channel.muted);
        assert_eq!(left_channel.register_lines, right_channel.register_lines);
        assert_eq!(left_channel.detail_line, right_channel.detail_line);
        assert_f32_equal(
            "GBA APU channel waveform",
            &left_channel.waveform,
            &right_channel.waveform,
        );
        assert_eq!(left_channel.waveform.len(), 512);
    }
}
