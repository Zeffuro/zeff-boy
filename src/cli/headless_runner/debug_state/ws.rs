use std::path::PathBuf;

use zeff_ws_core::emulator::{Emulator as WsEmulator, WsOpcodeRecord};

use crate::cli::types::HeadlessOptions;

use super::super::{AudioStats, InputMasks, StuckReport, framebuffer_fingerprint};
use super::{hex_bytes, input_json, input_schedule_json, screenshot_json, stuck_report_json};

pub(in crate::cli::headless_runner) struct WsDebugStateRequest<'a> {
    pub(in crate::cli::headless_runner) emulator: &'a WsEmulator,
    pub(in crate::cli::headless_runner) frames_run: u64,
    pub(in crate::cli::headless_runner) opts: &'a HeadlessOptions,
    pub(in crate::cli::headless_runner) input: InputMasks,
    pub(in crate::cli::headless_runner) stuck: Option<&'a StuckReport>,
    pub(in crate::cli::headless_runner) screenshot: Option<&'a PathBuf>,
    pub(in crate::cli::headless_runner) audio_stats: AudioStats,
}

pub(in crate::cli::headless_runner) fn ws_debug_state(
    request: WsDebugStateRequest<'_>,
) -> serde_json::Value {
    let emulator = request.emulator;
    let regs = emulator.cpu_registers();
    let segments = emulator.cpu_segments();
    let footer = emulator.footer();
    let ppu = emulator.ppu_debug_snapshot();
    let apu = emulator.apu_debug_snapshot();
    let uart = emulator.uart_debug_snapshot();
    let ram_sample = emulator
        .system_ram()
        .iter()
        .take(128)
        .copied()
        .collect::<Vec<_>>();
    let breakpoints = emulator.iter_breakpoints().collect::<Vec<_>>();
    let watchpoints = emulator
        .debug_watchpoints()
        .iter()
        .map(|watch| {
            serde_json::json!({
                "address": watch.address,
                "address_hex": format!("{:05X}", watch.address),
                "end_address": watch.end_address,
                "end_address_hex": format!("{:05X}", watch.end_address),
                "type": format!("{:?}", watch.watch_type),
                "last_value": watch.last_value,
            })
        })
        .collect::<Vec<_>>();
    let hit_watchpoint = emulator.debug_hit_watchpoint().map(|hit| {
        serde_json::json!({
            "address": hit.address,
            "address_hex": format!("{:05X}", hit.address),
            "old_value": hit.old_value,
            "new_value": hit.new_value,
            "type": format!("{:?}", hit.watch_type),
        })
    });
    let apu_json = serde_json::json!({
        "sample_rate": apu.sample_rate,
        "sample_generation_enabled": apu.sample_generation_enabled,
        "buffered_samples": apu.buffered_samples,
        "drained_samples": request.audio_stats.sample_count,
        "drained_frames": request.audio_stats.frames_with_samples,
        "drained_nonzero_samples": request.audio_stats.nonzero_samples,
        "drained_peak_abs": request.audio_stats.peak_abs,
        "drained_mean_abs": request.audio_stats.mean_abs(),
        "period": apu.period,
        "volume": apu.volume,
        "voice_volume": apu.voice_volume,
        "sweep_step": apu.sweep_step,
        "sweep_value": apu.sweep_value,
        "noise_control": apu.noise_control,
        "control": apu.control,
        "output_control": apu.output_control,
        "sample_ram_pos": apu.sample_ram_pos,
        "sample_pos": apu.sample_pos,
        "nreg": apu.nreg,
        "hyper_voice_sample": apu.hyper_voice_sample,
        "hyper_voice_left_output": apu.hyper_voice_left_output,
        "hyper_voice_right_output": apu.hyper_voice_right_output,
        "hyper_voice_next_left": apu.hyper_voice_next_left,
        "sound_test": apu.sound_test,
        "hyper_voice_control": apu.hyper_voice_control,
        "hyper_voice_channel_control": apu.hyper_voice_channel_control,
        "channel_mutes": apu.channel_mutes,
    });
    let uart_json = serde_json::json!({
        "rx_data": uart.rx_data,
        "rx_data_hex": format!("{:02X}", uart.rx_data),
        "tx_data": uart.tx_data,
        "tx_data_hex": format!("{:02X}", uart.tx_data),
        "control": uart.control,
        "control_hex": format!("{:02X}", uart.control),
        "status": uart.status,
        "status_hex": format!("{:02X}", uart.status),
        "baud_bps": uart.baud_bps,
        "tx_cycles_remaining": uart.tx_cycles_remaining,
        "completed_tx": uart.completed_tx,
        "completed_tx_hex": uart.completed_tx.map(|value| format!("{value:02X}")),
        "flags": {
            "enabled": uart.control & 0x80 != 0,
            "fast_baud": uart.control & 0x40 != 0,
            "tx_empty": uart.status & 0x04 != 0,
            "rx_ready": uart.status & 0x01 != 0,
            "overrun": uart.status & 0x02 != 0,
        },
    });

    serde_json::json!({
        "system": "ws",
        "frames": request.frames_run,
        "cycles": emulator.cpu_cycles(),
        "pc": emulator.cpu_pc(),
        "pc_hex": format!("{:05X}", emulator.cpu_pc()),
        "ip": emulator.cpu_ip(),
        "ip_hex": format!("{:04X}", emulator.cpu_ip()),
        "flags": emulator.cpu_flags(),
        "flags_hex": format!("{:04X}", emulator.cpu_flags()),
        "registers": {
            "ax": regs[0],
            "cx": regs[1],
            "dx": regs[2],
            "bx": regs[3],
            "sp": regs[4],
            "bp": regs[5],
            "si": regs[6],
            "di": regs[7],
            "ax_hex": format!("{:04X}", regs[0]),
            "cx_hex": format!("{:04X}", regs[1]),
            "dx_hex": format!("{:04X}", regs[2]),
            "bx_hex": format!("{:04X}", regs[3]),
            "sp_hex": format!("{:04X}", regs[4]),
            "bp_hex": format!("{:04X}", regs[5]),
            "si_hex": format!("{:04X}", regs[6]),
            "di_hex": format!("{:04X}", regs[7]),
        },
        "segments": {
            "es": segments[0],
            "cs": segments[1],
            "ss": segments[2],
            "ds": segments[3],
            "es_hex": format!("{:04X}", segments[0]),
            "cs_hex": format!("{:04X}", segments[1]),
            "ss_hex": format!("{:04X}", segments[2]),
            "ds_hex": format!("{:04X}", segments[3]),
        },
        "cpu": {
            "state": format!("{:?}", emulator.cpu_state()),
            "last_opcode": emulator.cpu_last_opcode(),
            "last_opcode_hex": format!("{:02X}", emulator.cpu_last_opcode()),
            "last_fetch": last_fetch_json(emulator),
            "trap": emulator.last_trap().map(|trap| format!("{trap:?}")),
            "suspended": emulator.is_cpu_suspended(),
        },
        "debug": {
            "breakpoints": breakpoints,
            "breakpoints_hex": breakpoints
                .iter()
                .map(|addr| format!("{addr:05X}"))
                .collect::<Vec<_>>(),
            "watchpoints": watchpoints,
            "hit_breakpoint": emulator.debug_hit_breakpoint(),
            "hit_breakpoint_hex": emulator
                .debug_hit_breakpoint()
                .map(|addr| format!("{addr:05X}")),
            "hit_watchpoint": hit_watchpoint,
            "recent_opcodes": emulator
                .recent_opcodes(16)
                .into_iter()
                .map(ws_opcode_record_json)
                .collect::<Vec<_>>(),
        },
        "cartridge": {
            "rom_crc32": emulator.rom_crc32(),
            "rom_crc32_hex": format!("{:08X}", emulator.rom_crc32()),
            "rom_len": emulator.cartridge_rom_bytes().len(),
            "minimum_system": format!("{:?}", footer.minimum_system),
            "orientation": format!("{:?}", emulator.preferred_orientation()),
            "developer_id": footer.developer_id,
            "developer_id_hex": format!("{:02X}", footer.developer_id),
            "cartridge_id": footer.cartridge_id,
            "cartridge_id_hex": format!("{:02X}", footer.cartridge_id),
            "revision": footer.revision,
            "rom_size_code": footer.rom_size.code,
            "declared_rom_bytes": footer.rom_size.declared_bytes,
            "save_kind": format!("{:?}", footer.save_kind),
            "save_bytes": footer.save_kind.size(),
            "rtc_present": footer.rtc_present,
            "checksum": footer.checksum,
            "checksum_hex": format!("{:04X}", footer.checksum),
            "computed_checksum": footer.computed_checksum,
            "computed_checksum_hex": format!("{:04X}", footer.computed_checksum),
            "checksum_valid": footer.checksum_valid,
        },
        "memory": {
            "internal_ram_nonzero": emulator
                .system_ram()
                .iter()
                .filter(|&&byte| byte != 0)
                .count(),
            "internal_ram_sample": ram_sample,
            "internal_ram_sample_hex": hex_bytes(&ram_sample),
        },
        "ppu": {
            "vcount": ppu.vcount,
            "line_cycles": ppu.line_cycles,
            "in_vblank": ppu.in_vblank,
            "frame_ready": ppu.frame_ready,
        },
        "apu": apu_json,
        "uart": uart_json,
        "input": input_json(request.input),
        "input_schedule": input_schedule_json(request.opts),
        "stuck": stuck_report_json(request.stuck),
        "screenshot": screenshot_json(request.screenshot),
        "framebuffer_dimensions": emulator.framebuffer_dimensions(),
        "framebuffer_hash": framebuffer_fingerprint(emulator.framebuffer()),
    })
}

fn last_fetch_json(emulator: &WsEmulator) -> Option<serde_json::Value> {
    emulator.last_fetch().map(|fetch| {
        serde_json::json!({
            "cs": fetch.cs,
            "cs_hex": format!("{:04X}", fetch.cs),
            "ip": fetch.ip,
            "ip_hex": format!("{:04X}", fetch.ip),
            "pc": fetch.pc,
            "pc_hex": format!("{:05X}", fetch.pc),
            "opcode": fetch.opcode,
            "opcode_hex": format!("{:02X}", fetch.opcode),
            "cycles": fetch.cycles,
        })
    })
}

fn ws_opcode_record_json(record: WsOpcodeRecord) -> serde_json::Value {
    serde_json::json!({
        "cs": record.cs,
        "cs_hex": format!("{:04X}", record.cs),
        "ip": record.ip,
        "ip_hex": format!("{:04X}", record.ip),
        "pc": record.pc,
        "pc_hex": format!("{:05X}", record.pc),
        "opcode": record.opcode,
        "opcode_hex": format!("{:02X}", record.opcode),
        "cycles": record.cycles,
    })
}
