use std::path::PathBuf;

use crate::cli::types::HeadlessOptions;
use crate::emu_backend::PceBackend;
use crate::emu_core_trait::EmulatorCore;

use super::super::{InputMasks, StuckReport, framebuffer_fingerprint};
use super::{input_json, input_schedule_json, screenshot_json, stuck_report_json};

pub(in crate::cli::headless_runner) struct PceDebugStateRequest<'a> {
    pub(in crate::cli::headless_runner) backend: &'a PceBackend,
    pub(in crate::cli::headless_runner) frames_run: u64,
    pub(in crate::cli::headless_runner) opts: &'a HeadlessOptions,
    pub(in crate::cli::headless_runner) input: InputMasks,
    pub(in crate::cli::headless_runner) input_p2: InputMasks,
    pub(in crate::cli::headless_runner) stuck: Option<&'a StuckReport>,
    pub(in crate::cli::headless_runner) screenshot: Option<&'a PathBuf>,
    pub(in crate::cli::headless_runner) audio_samples: u64,
    pub(in crate::cli::headless_runner) audio_nonzero_samples: u64,
    pub(in crate::cli::headless_runner) audio_peak_abs: f32,
}

pub(in crate::cli::headless_runner) fn pce_debug_state(
    request: PceDebugStateRequest<'_>,
) -> serde_json::Value {
    let snapshot = request.backend.debug_cpu_snapshot();
    let registers = snapshot.registers();
    let hardware = request.backend.debug_hardware_snapshot();
    let memory_base = hardware.controller.memory_base;
    let arcade_card = hardware.arcade_card.map(|arcade| {
        serde_json::json!({
            "present": true,
            "mode": format!("{:?}", request.backend.arcade_card_mode()),
            "ports": arcade.ports.map(|port| serde_json::json!({
                "base": port.base,
                "offset": port.offset,
                "increment": port.increment,
                "control": port.control,
                "effective_address": port.effective_address,
            })),
            "value": arcade.value,
            "shift": arcade.shift,
            "rotate": arcade.rotate,
        })
    });
    let cd = request.backend.cdrom2().map(|cdrom| {
        let audio = cdrom.audio_debug_snapshot();
        let commands = cdrom
            .command_trace()
            .iter()
            .map(|command| {
                command
                    .bytes()
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "present": true,
            "phase": format!("{:?}", cdrom.phase()),
            "audio_status": format!("{:?}", cdrom.audio_status()),
            "audio_transport": {
                "start_lba": audio.start_lba,
                "end_lba": audio.end_lba,
                "current_lba": audio.current_lba,
                "current_sample": audio.current_sample,
                "remaining_sectors": audio.end_lba.saturating_sub(audio.current_lba),
                "end_mode": format!("{:?}", audio.end_mode),
                "tick_accumulator": audio.tick_accumulator,
                "queued_source_frames": audio.queued_source_frames,
            },
            "fade": {
                "control": audio.fade_control,
                "control_hex": format!("{:02X}", audio.fade_control),
                "target": audio.fade_target.map(|target| format!("{target:?}")),
                "level_q16": audio.fade_level_q16,
                "step_ticks": audio.fade_step_ticks,
                "ticks_to_next": audio.fade_ticks_to_next,
            },
            "command_count": commands.len(),
            "commands": commands,
            "bram_unlocked": cdrom.bram_unlocked(),
        })
    });

    serde_json::json!({
        "system": "pce",
        "frames": request.frames_run,
        "master_ticks": snapshot.master_ticks(),
        "pc": registers.pc,
        "pc_hex": format!("{:04X}", registers.pc),
        "suspended": snapshot.faulted(),
        "cpu": {
            "a": registers.a,
            "a_hex": format!("{:02X}", registers.a),
            "x": registers.x,
            "x_hex": format!("{:02X}", registers.x),
            "y": registers.y,
            "y_hex": format!("{:02X}", registers.y),
            "sp": registers.sp,
            "sp_hex": format!("{:02X}", registers.sp),
            "status": registers.status.bits(),
            "status_hex": format!("{:02X}", registers.status.bits()),
            "mpr": snapshot.mapping_registers(),
            "mpr_hex": snapshot.mapping_registers().map(|byte| format!("{byte:02X}")),
            "speed": format!("{:?}", snapshot.speed_mode()),
            "timer": {
                "counter": snapshot.timer_counter(),
                "reload": snapshot.timer_reload(),
                "running": snapshot.timer_running(),
                "prescaler_ticks": snapshot.timer_prescaler_ticks(),
            },
            "irq": {
                "disable": snapshot.irq_disable(),
                "request": snapshot.irq_request(),
                "sampled": snapshot.sampled_interrupt().map(|source| format!("{source:?}")),
            },
            "vce_line": snapshot.vce_line_index(),
        },
        "hardware": {
            "topology": format!("{:?}", request.backend.hardware_topology()),
            "console_wiring": format!("{:?}", request.backend.console_wiring()),
            "hucard_board": format!("{:?}", request.backend.hucard_board()),
            "controller_mode": format!("{:?}", request.backend.controller_mode()),
            "normalized_disc_sha256": request.backend.normalized_disc_hash().map(|hash| hash.iter().map(|byte| format!("{byte:02x}")).collect::<String>()),
            "memory_base_128": {
                "mode": format!("{:?}", request.backend.memory_base_mode()),
                "connected": memory_base.connected,
                "active": memory_base.active,
                "phase": format!("{:?}", memory_base.phase),
                "address": memory_base.address,
                "remaining_bits": memory_base.remaining_bits,
                "dirty": memory_base.dirty,
            },
            "content_sha256": request.backend.rom_hash().iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
            "source_crc32": request.backend.source_crc32().map(|crc| format!("{crc:08x}")),
            "source_normalized_disc_sha256": request.backend.source_disc_hash().map(|hash| hash.iter().map(|byte| format!("{byte:02x}")).collect::<String>()),
        },
        "cdrom2": cd.unwrap_or_else(|| serde_json::json!({ "present": false })),
        "arcade_card": arcade_card.unwrap_or_else(|| serde_json::json!({
            "present": false,
            "mode": format!("{:?}", request.backend.arcade_card_mode()),
        })),
        "audio": {
            "samples": request.audio_samples,
            "nonzero_samples": request.audio_nonzero_samples,
            "peak_abs": request.audio_peak_abs,
        },
        "framebuffer": {
            "width": crate::emu_backend::pce::PCE_PRESENTED_WIDTH,
            "height": crate::emu_backend::pce::PCE_PRESENTED_HEIGHT,
            "fingerprint": format!("{:016X}", framebuffer_fingerprint(request.backend.framebuffer())),
        },
        "input": input_json(request.input),
        "input_p2": input_json(request.input_p2),
        "input_schedule": input_schedule_json(request.opts),
        "stuck": stuck_report_json(request.stuck),
        "screenshot": screenshot_json(request.screenshot),
    })
}
