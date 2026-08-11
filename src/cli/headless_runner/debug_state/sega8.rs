use std::path::PathBuf;

use zeff_sega8_core::emulator::Emulator as Sega8Emulator;
use zeff_sega8_core::hardware::constants::{
    VDP_STATUS_SPRITE_COLLISION, VDP_STATUS_SPRITE_OVERFLOW, VDP_STATUS_VBLANK,
};

use crate::cli::types::HeadlessOptions;

use super::super::{AudioStats, InputMasks, StuckReport, framebuffer_fingerprint};
use super::{hex_bytes, input_json, input_schedule_json, screenshot_json, stuck_report_json};

pub(in crate::cli::headless_runner) struct Sega8DebugStateRequest<'a> {
    pub(in crate::cli::headless_runner) emulator: &'a Sega8Emulator,
    pub(in crate::cli::headless_runner) frames_run: u64,
    pub(in crate::cli::headless_runner) opts: &'a HeadlessOptions,
    pub(in crate::cli::headless_runner) input: InputMasks,
    pub(in crate::cli::headless_runner) input_p2: InputMasks,
    pub(in crate::cli::headless_runner) stuck: Option<&'a StuckReport>,
    pub(in crate::cli::headless_runner) screenshot: Option<&'a PathBuf>,
    pub(in crate::cli::headless_runner) audio_stats: AudioStats,
}

pub(in crate::cli::headless_runner) fn sega8_debug_state(
    request: Sega8DebugStateRequest<'_>,
) -> serde_json::Value {
    let emulator = request.emulator;
    let regs = emulator.cpu().regs();
    let vdp = emulator.bus().vdp();
    let mode4 = vdp.mode4_debug_snapshot();
    let mode4_json = mode4_debug_json(mode4);
    let psg = emulator.bus().apu().debug_snapshot();
    let gg_serial = emulator.bus().game_gear_serial().debug_snapshot();
    let mapper = emulator.bus().mapper();
    let cartridge = &emulator.bus().cartridge;
    let vram_sample = vdp.vram().iter().take(128).copied().collect::<Vec<_>>();
    let cram_sample = vdp.cram().iter().take(64).copied().collect::<Vec<_>>();
    let breakpoints = emulator.iter_breakpoints().collect::<Vec<_>>();
    let watchpoints = emulator
        .debug_watchpoints()
        .iter()
        .map(|watch| {
            serde_json::json!({
                "address": watch.address,
                "address_hex": format!("{:04X}", watch.address),
                "type": format!("{:?}", watch.watch_type),
                "last_value": watch.last_value,
            })
        })
        .collect::<Vec<_>>();
    let hit_watchpoint = emulator.debug_hit_watchpoint().map(|hit| {
        serde_json::json!({
            "address": hit.address,
            "address_hex": format!("{:04X}", hit.address),
            "old_value": hit.old_value,
            "new_value": hit.new_value,
            "type": format!("{:?}", hit.watch_type),
        })
    });
    let header = cartridge.header().map(|header| {
        serde_json::json!({
            "location": format!("{:?}", header.location),
            "checksum": header.checksum,
            "checksum_hex": format!("{:04X}", header.checksum),
            "product_code_bcd": header.product_code_bcd,
            "version": header.version,
            "region": format!("{:?}", header.region),
            "region_code": header.region.code(),
            "rom_size_code": header.rom_size_code,
        })
    });
    let codemasters_header = cartridge.codemasters_header().map(|header| {
        serde_json::json!({
            "checksum_bank_count": header.checksum_bank_count,
            "day_bcd": format!("{:02X}", header.day_bcd),
            "month_bcd": format!("{:02X}", header.month_bcd),
            "year_bcd": format!("{:02X}", header.year_bcd),
            "hour_bcd": format!("{:02X}", header.hour_bcd),
            "minute_bcd": format!("{:02X}", header.minute_bcd),
            "checksum": header.checksum,
            "checksum_hex": format!("{:04X}", header.checksum),
            "checksum_complement": header.checksum_complement,
            "checksum_complement_hex": format!("{:04X}", header.checksum_complement),
        })
    });

    let registers_json = serde_json::json!({
        "a": regs.a,
        "f": regs.f,
        "b": regs.b,
        "c": regs.c,
        "d": regs.d,
        "e": regs.e,
        "h": regs.h,
        "l": regs.l,
        "ix": regs.ix,
        "iy": regs.iy,
        "i": regs.i,
        "r": regs.r,
        "af_hex": format!("{:04X}", regs.af()),
        "bc_hex": format!("{:04X}", regs.bc()),
        "de_hex": format!("{:04X}", regs.de()),
        "hl_hex": format!("{:04X}", regs.hl()),
        "ix_hex": format!("{:04X}", regs.ix),
        "iy_hex": format!("{:04X}", regs.iy),
    });
    let cpu_json = serde_json::json!({
        "state": format!("{:?}", emulator.cpu().state()),
        "interrupt_mode": format!("{:?}", emulator.cpu().interrupt_mode()),
        "iff1": emulator.cpu().interrupts_enabled(),
        "iff2": emulator.cpu().saved_interrupts_enabled(),
        "last_opcode_pc": emulator.cpu().last_opcode_pc(),
        "last_opcode_pc_hex": format!("{:04X}", emulator.cpu().last_opcode_pc()),
        "last_opcode": emulator.cpu().last_opcode(),
        "last_opcode_hex": format!("{:02X}", emulator.cpu().last_opcode()),
        "trap": emulator.cpu_trap().map(|trap| format!("{trap:?}")),
        "suspended": emulator.is_suspended(),
    });
    let debug_json = serde_json::json!({
        "breakpoints": breakpoints,
        "breakpoints_hex": breakpoints
            .iter()
            .map(|addr| format!("{:04X}", addr))
            .collect::<Vec<_>>(),
        "watchpoints": watchpoints,
        "hit_breakpoint": emulator.debug_hit_breakpoint(),
        "hit_breakpoint_hex": emulator
            .debug_hit_breakpoint()
            .map(|addr| format!("{:04X}", addr)),
        "hit_watchpoint": hit_watchpoint,
        "recent_opcodes": emulator
            .recent_opcodes(16)
            .into_iter()
            .map(|(pc, opcode, cycles)| {
                serde_json::json!({
                    "pc": pc,
                    "pc_hex": format!("{:04X}", pc),
                    "opcode": opcode,
                    "opcode_hex": format!("{:02X}", opcode),
                    "cycles": cycles,
                })
            })
            .collect::<Vec<_>>(),
    });
    let mapper_json = serde_json::json!({
        "kind": mapper.kind_label(),
        "kind_debug": format!("{:?}", mapper.kind()),
        "frame_control": mapper.frame_control(),
        "frame_control_hex": format!("{:02X}", mapper.frame_control()),
        "slot_banks": mapper.slot_banks(),
        "slot2_cartridge_ram_enabled": mapper.slot2_cartridge_ram_enabled(),
        "cartridge_ram_bank": mapper.cartridge_ram_bank(),
        "cartridge_ram_nonzero": emulator
            .bus()
            .cartridge_ram()
            .iter()
            .filter(|&&byte| byte != 0)
            .count(),
    });
    let cartridge_json = serde_json::json!({
        "system": format!("{:?}", cartridge.system()),
        "mapper_kind": cartridge.mapper_kind().label(),
        "mapper_kind_debug": format!("{:?}", cartridge.mapper_kind()),
        "raw_len": cartridge.raw_len(),
        "normalized_len": cartridge.normalized_len(),
        "copier_header_stripped": cartridge.copier_header_stripped(),
        "rom_bank_count": cartridge.rom_bank_count(),
        "header": header,
        "codemasters_header": codemasters_header,
    });
    let vdp_json = serde_json::json!({
        "status": vdp.status(),
        "status_hex": format!("{:02X}", vdp.status()),
        "status_flags": {
            "vblank": vdp.status() & VDP_STATUS_VBLANK != 0,
            "sprite_overflow": vdp.status() & VDP_STATUS_SPRITE_OVERFLOW != 0,
            "sprite_collision": vdp.status() & VDP_STATUS_SPRITE_COLLISION != 0,
        },
        "address": vdp.address(),
        "address_hex": format!("{:04X}", vdp.address()),
        "code": vdp.code(),
        "v_counter": vdp.v_counter(),
        "h_counter": vdp.h_counter(),
        "scanline": vdp.scanline(),
        "total_scanlines": vdp.total_scanlines(),
        "cycles_per_frame": emulator.video_standard().cycles_per_frame(),
        "scanline_cycle": vdp.scanline_cycle(),
        "frame_interrupt_enabled": vdp.frame_interrupt_enabled(),
        "display_enabled": vdp.display_enabled(),
        "tms9918_mode": format!("{:?}", vdp.tms9918_mode()),
        "line_interrupt_enabled": vdp.line_interrupt_enabled(),
        "interrupt_pending": vdp.interrupt_pending(),
        "line_interrupt_pending": vdp.line_interrupt_pending(),
        "line_counter": vdp.line_counter(),
        "mode4": mode4_json,
        "registers": vdp.registers(),
        "registers_hex": hex_bytes(vdp.registers()),
        "vram_nonzero": vdp.vram().iter().filter(|&&byte| byte != 0).count(),
        "cram_nonzero": vdp.cram().iter().filter(|&&byte| byte != 0).count(),
        "vram_sample": vram_sample,
        "cram_sample": cram_sample,
    });
    let psg_json = serde_json::json!({
        "tone_period": psg.tone_period,
        "volume": psg.volume,
        "noise_control": psg.noise_control,
        "stereo_control": psg.stereo_control,
        "latched_register": psg.latched_register,
        "sample_rate": psg.sample_rate,
        "clock_hz_approx": emulator.video_standard().clock_hz_approx(),
        "sample_generation_enabled": psg.sample_generation_enabled,
        "buffered_samples": psg.buffered_samples,
        "drained_samples": request.audio_stats.sample_count,
        "drained_frames": request.audio_stats.frames_with_samples,
        "drained_nonzero_samples": request.audio_stats.nonzero_samples,
        "drained_peak_abs": request.audio_stats.peak_abs,
        "drained_mean_abs": request.audio_stats.mean_abs(),
        "channel_mutes": psg.channel_mutes,
        "last_write": psg.last_write,
        "write_count": psg.write_count,
    });
    let gg_serial_json = serde_json::json!({
        "ext_data": gg_serial.ext_data,
        "ext_data_hex": format!("{:02X}", gg_serial.ext_data),
        "ext_direction": gg_serial.ext_direction,
        "ext_direction_hex": format!("{:02X}", gg_serial.ext_direction),
        "tx_data": gg_serial.tx_data,
        "tx_data_hex": format!("{:02X}", gg_serial.tx_data),
        "rx_data": gg_serial.rx_data,
        "rx_data_hex": format!("{:02X}", gg_serial.rx_data),
        "control": gg_serial.control,
        "control_hex": format!("{:02X}", gg_serial.control),
        "status": gg_serial.status,
        "status_hex": format!("{:02X}", gg_serial.status),
        "flags": {
            "tx_full": gg_serial.status & 0x01 != 0,
            "rx_ready": gg_serial.status & 0x02 != 0,
            "error": gg_serial.status & 0x04 != 0,
        },
    });

    serde_json::json!({
        "system": format!("{:?}", emulator.system()),
        "video_standard": emulator.video_standard().label(),
        "video_standard_debug": format!("{:?}", emulator.video_standard()),
        "console_region": emulator.console_region().label(),
        "console_region_debug": format!("{:?}", emulator.console_region()),
        "frames": request.frames_run,
        "cycles": emulator.cpu().cycles(),
        "pc": regs.pc,
        "pc_hex": format!("{:04X}", regs.pc),
        "sp": regs.sp,
        "sp_hex": format!("{:04X}", regs.sp),
        "registers": registers_json,
        "cpu": cpu_json,
        "debug": debug_json,
        "mapper": mapper_json,
        "cartridge": cartridge_json,
        "vdp": vdp_json,
        "psg": psg_json,
        "game_gear_serial": gg_serial_json,
        "input": input_json(request.input),
        "input_p2": input_json(request.input_p2),
        "sega8_input": sega8_input_debug_json(emulator),
        "input_schedule": input_schedule_json(request.opts),
        "stuck": stuck_report_json(request.stuck),
        "screenshot": screenshot_json(request.screenshot),
        "framebuffer_dimensions": emulator.framebuffer_dimensions(),
        "framebuffer_hash": framebuffer_fingerprint(emulator.framebuffer()),
    })
}

fn sega8_input_debug_json(emulator: &Sega8Emulator) -> serde_json::Value {
    let input = emulator.bus().input();
    serde_json::json!({
        "io_control": input.io_control(),
        "io_control_hex": format!("{:02X}", input.io_control()),
        "controller_1_raw": input.read_controller(
            zeff_sega8_core::hardware::input::ControllerPort::One,
        ),
        "controller_2_raw": input.read_controller(
            zeff_sega8_core::hardware::input::ControllerPort::Two,
        ),
        "controller_2_effective": input.read_controller_for_bus(
            zeff_sega8_core::hardware::input::ControllerPort::Two,
            emulator.console_region(),
        ),
        "game_gear_start_port": input.read_game_gear_start(emulator.console_region()),
    })
}

fn mode4_debug_json(
    mode4: zeff_sega8_core::hardware::vdp::Mode4VdpDebugSnapshot,
) -> serde_json::Value {
    serde_json::json!({
        "enabled": mode4.enabled,
        "name_table_base": mode4.name_table_base,
        "name_table_base_hex": format!("{:04X}", mode4.name_table_base),
        "sprite_table_base": mode4.sprite_table_base,
        "sprite_table_base_hex": format!("{:04X}", mode4.sprite_table_base),
        "sprite_pattern_base": mode4.sprite_pattern_base,
        "sprite_pattern_base_hex": format!("{:04X}", mode4.sprite_pattern_base),
        "horizontal_scroll": mode4.horizontal_scroll,
        "vertical_scroll": mode4.vertical_scroll,
        "backdrop_color_index": mode4.backdrop_color_index,
        "sprite_height": mode4.sprite_height,
        "sprite_width": mode4.sprite_width,
        "max_sprites_per_line": mode4.max_sprites_per_line,
        "flags": {
            "horizontal_scroll_lock": mode4.horizontal_scroll_lock,
            "vertical_scroll_lock": mode4.vertical_scroll_lock,
            "hide_left_column": mode4.hide_left_column,
            "sprite_shift_left": mode4.sprite_shift_left,
            "sprite_magnified": mode4.sprite_magnified,
        },
    })
}
