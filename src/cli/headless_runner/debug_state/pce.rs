use std::path::PathBuf;

use crate::cli::types::HeadlessOptions;
use crate::emu_backend::PceBackend;
use crate::emu_core_trait::EmulatorCore;
use zeff_pce_core::hardware::{CdAdpcmDebugSnapshot, CdDisc, VdcDebugSnapshot};

use super::super::{InputMasks, StuckReport, framebuffer_fingerprint};
use super::{input_json, input_schedule_json, screenshot_json, stuck_report_json};

pub(in crate::cli::headless_runner) struct PceDebugStateRequest<'a> {
    pub(in crate::cli::headless_runner) backend: &'a PceBackend,
    pub(in crate::cli::headless_runner) frames_run: u64,
    pub(in crate::cli::headless_runner) opts: &'a HeadlessOptions,
    pub(in crate::cli::headless_runner) input: InputMasks,
    pub(in crate::cli::headless_runner) input_p2: InputMasks,
    pub(in crate::cli::headless_runner) input_p3: InputMasks,
    pub(in crate::cli::headless_runner) input_p4: InputMasks,
    pub(in crate::cli::headless_runner) input_p5: InputMasks,
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
    let video = serde_json::json!({
        "vdc": vdc_debug_json(&hardware.vdc),
        "vdc2": hardware.vdc2.as_ref().map(vdc_debug_json),
        "background_lines": background_lines_json(request.backend),
    });
    let cd = request.backend.cdrom2().map(|cdrom| {
        let audio = cdrom.audio_debug_snapshot();
        let adpcm = cdrom.adpcm_debug_snapshot();
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
            "adpcm": adpcm_debug_json(adpcm),
            "command_count": commands.len(),
            "commands": commands,
            "bram_unlocked": cdrom.bram_unlocked(),
            "tracks": cd_tracks_json(cdrom.disc()),
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
            "canonical_title": request.backend.canonical_title_metadata().map(|title| serde_json::json!({
                "id": title.id,
                "title": title.title,
                "region": title.region,
                "normalized_disc_sha256": title.normalized_disc_sha256.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
                "controller_mode": format!("{:?}", title.controller_mode),
                "memory_base_128": title.memory_base_128,
                "arcade_card": title.arcade_card,
                "minimum_system_card": title.minimum_system_card.map(|tier| format!("{tier:?}")),
            })),
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
            "video": video,
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
        "input_p3": input_json(request.input_p3),
        "input_p4": input_json(request.input_p4),
        "input_p5": input_json(request.input_p5),
        "input_schedule": input_schedule_json(request.opts),
        "stuck": stuck_report_json(request.stuck),
        "screenshot": screenshot_json(request.screenshot),
    })
}

fn adpcm_debug_json(adpcm: CdAdpcmDebugSnapshot) -> serde_json::Value {
    serde_json::json!({
        "address_latch": adpcm.address_latch,
        "address_latch_hex": format!("{:04X}", adpcm.address_latch),
        "address": adpcm.read_address,
        "address_hex": format!("{:04X}", adpcm.read_address),
        "write_address": adpcm.write_address,
        "write_address_hex": format!("{:04X}", adpcm.write_address),
        "remaining_length": adpcm.remaining_length,
        "playback_rate": adpcm.playback_rate,
        "playing": adpcm.playing,
    })
}

fn background_lines_json(backend: &PceBackend) -> serde_json::Value {
    serde_json::Value::Array(
        backend
            .debug_presented_frame()
            .rows()
            .iter()
            .enumerate()
            .filter_map(|(line, row)| {
                let background = row.background()?;
                Some(serde_json::json!({
                    "line": line,
                    "scroll_x": background.scroll_x(),
                    "virtual_y": background.virtual_y(),
                    "first_bat_word": background.first_bat_word(),
                    "first_bat_word_hex": format!("{:04X}", background.first_bat_word()),
                }))
            })
            .collect(),
    )
}

fn cd_tracks_json(disc: &CdDisc) -> serde_json::Value {
    serde_json::Value::Array(
        disc.tracks()
            .iter()
            .map(|track| {
                serde_json::json!({
                    "number": track.number(),
                    "mode": format!("{:?}", track.mode()),
                    "index0_lba": track.index0_lba(),
                    "index1_lba": track.index1_lba(),
                    "stored_start_lba": track.stored_start_lba(),
                    "frames": track.sector_count(),
                    "payload_sha256": track.payload_hash().iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
                })
            })
            .collect(),
    )
}

fn vdc_debug_json(vdc: &VdcDebugSnapshot) -> serde_json::Value {
    serde_json::json!({
        "selected_register": vdc.selected_register.map(|register| format!("{register:?}")),
        "selected_register_id": vdc.selected_register_id,
        "status": vdc.status.bits(),
        "status_hex": format!("{:02X}", vdc.status.bits()),
        "irq_asserted": vdc.irq_asserted,
        "registers": vdc.registers,
        "registers_hex": vdc.registers.map(|value| format!("{value:04X}")),
        "satb": vdc.satb.as_slice(),
        "satb_hex": vdc.satb.iter().map(|value| format!("{value:04X}")).collect::<Vec<_>>(),
        "horizontal": {
            "phase": format!("{:?}", vdc.horizontal_phase),
            "pixels_remaining": vdc.horizontal_pixels_remaining,
        },
        "vertical": {
            "phase": format!("{:?}", vdc.vertical_phase),
            "phase_line": vdc.vertical_phase_line,
            "phase_duration": vdc.vertical_phase_duration,
            "frame_line": vdc.frame_line,
            "raster_counter": vdc.raster_counter,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeff_pce_core::hardware::{CdTrack, CdTrackMode, HuC6270};

    #[test]
    fn vdc_debug_json_includes_timing_and_register_state() {
        let value = vdc_debug_json(&HuC6270::new().debug_snapshot());

        assert_eq!(value["status_hex"], "00");
        assert_eq!(value["registers"].as_array().unwrap().len(), 0x14);
        assert_eq!(value["registers_hex"].as_array().unwrap().len(), 0x14);
        assert_eq!(value["satb"].as_array().unwrap().len(), 0x100);
        assert_eq!(value["satb_hex"].as_array().unwrap().len(), 0x100);
        assert_eq!(value["horizontal"]["phase"], "DisplayStart");
        assert_eq!(value["vertical"]["phase"], "VerticalSync");
    }

    #[test]
    fn cd_tracks_json_includes_track_identity_and_layout() {
        let disc = CdDisc::new(vec![
            CdTrack::from_stored_data(
                1,
                4,
                Some(0),
                1,
                CdTrackMode::Mode1_2352,
                vec![0; 2 * 2_352],
            )
            .unwrap(),
        ])
        .unwrap();
        let tracks = cd_tracks_json(&disc);

        assert_eq!(tracks[0]["number"], 1);
        assert_eq!(tracks[0]["mode"], "Mode1_2352");
        assert_eq!(tracks[0]["index0_lba"], 0);
        assert_eq!(tracks[0]["index1_lba"], 1);
        assert_eq!(tracks[0]["frames"], 2);
        assert_eq!(tracks[0]["payload_sha256"].as_str().unwrap().len(), 64);
    }

    #[test]
    fn adpcm_debug_json_reports_compact_playback_progress() {
        let value = adpcm_debug_json(CdAdpcmDebugSnapshot {
            address_latch: 0x1234,
            read_address: 0x1235,
            write_address: 0x5678,
            remaining_length: 0x4321,
            playback_rate: 0x0F,
            playing: true,
        });

        assert_eq!(value["address_latch_hex"], "1234");
        assert_eq!(value["address"], 0x1235);
        assert_eq!(value["write_address_hex"], "5678");
        assert_eq!(value["remaining_length"], 0x4321);
        assert_eq!(value["playback_rate"], 0x0F);
        assert_eq!(value["playing"], true);
    }
}
