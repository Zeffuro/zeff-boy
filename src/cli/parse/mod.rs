use std::path::PathBuf;

use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use self::bus::parse_addr_range_list_arg;
use self::input::{parse_input_event_arg, parse_input_script, parse_zapper_event_arg};
use self::numbers::{
    parse_pc_range_arg, parse_u8_arg, parse_u16_arg, parse_u64_arg, parse_usize_arg,
};
use self::values::{
    parse_dmg_palette_arg, parse_gba_audio_mute_list_arg, parse_gba_bg_layer_list_arg,
    parse_memory_dump_arg, parse_pce_arcade_card_mode_arg, parse_pce_controller_mode_arg,
    parse_pce_memory_base_mode_arg, parse_region_dump_arg, parse_sega8_console_region_arg,
    parse_sega8_video_standard_arg,
};
use super::types::{CliArgs, HeadlessBusTraceAccess, HeadlessOptions};

mod bus;
mod input;
mod numbers;
#[cfg(test)]
mod tests;
mod values;

pub(crate) fn parse_args() -> anyhow::Result<CliArgs> {
    parse_args_from(std::env::args().skip(1))
}

pub(crate) fn parse_args_from(
    args: impl IntoIterator<Item = impl Into<String>>,
) -> anyhow::Result<CliArgs> {
    let args: Vec<String> = args.into_iter().map(Into::into).collect();
    let mut mode_override: Option<HardwareModePreference> = None;
    let mut rom_path: Option<String> = None;
    let mut headless_enabled = false;
    let mut headless = HeadlessOptions::default();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--mode requires one of: auto|dmg|sgb|cgb");
                };
                mode_override = Some(match value.as_str() {
                    "auto" => HardwareModePreference::Auto,
                    "dmg" => HardwareModePreference::ForceDmg,
                    "sgb" => HardwareModePreference::ForceSgb,
                    "cgb" => HardwareModePreference::ForceCgb,
                    _ => anyhow::bail!("invalid --mode value; expected auto|dmg|sgb|cgb"),
                });
                i += 2;
            }
            "--headless" => {
                headless_enabled = true;
                i += 1;
            }
            "--max-frames" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--max-frames requires a numeric value");
                };
                headless.max_frames = parse_u64_arg(value, "--max-frames")?;
                i += 2;
            }
            "--expect-serial" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--expect-serial requires a string value");
                };
                headless.expect_serial = Some(value.to_string());
                i += 2;
            }
            "--expect-ws-text" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--expect-ws-text requires a string value");
                };
                headless.expect_ws_text = Some(value.to_string());
                i += 2;
            }
            "--expect-ws-pass-fail-tiles" => {
                headless.expect_ws_pass_fail_tiles = true;
                i += 1;
            }
            "--ws-link-peer" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--ws-link-peer requires a peer ROM path or 'same'");
                };
                headless.ws_link_peer_path = Some(PathBuf::from(value));
                i += 2;
            }
            "--expect-ws-link-bytes" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--expect-ws-link-bytes requires a numeric value");
                };
                headless.expect_ws_link_bytes = parse_u64_arg(value, "--expect-ws-link-bytes")?;
                i += 2;
            }
            "--expect-sega8-sdsc" | "--expect-sdsc" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("{} requires a string value", args[i]);
                };
                headless.expect_sega8_sdsc = Some(value.to_string());
                i += 2;
            }
            "--expect-sega8-audio" => {
                headless.expect_sega8_audio = true;
                i += 1;
            }
            "--expect-test-pass" => {
                headless.expect_test_pass = true;
                i += 1;
            }
            "--trace-opcodes" => {
                headless.trace_opcodes = true;
                i += 1;
            }
            "--trace-opcode-limit" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--trace-opcode-limit requires a numeric value");
                };
                headless.trace_opcode_limit = parse_u64_arg(value, "--trace-opcode-limit")?;
                i += 2;
            }
            "--trace-max-ops" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--trace-max-ops requires a numeric value");
                };
                headless.trace_opcode_limit = parse_u64_arg(value, "--trace-max-ops")?;
                i += 2;
            }
            "--trace-start-t" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--trace-start-t requires a numeric value");
                };
                headless.trace_start_t = parse_u64_arg(value, "--trace-start-t")?;
                i += 2;
            }
            "--trace-pc-range" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--trace-pc-range requires start-end");
                };
                headless.trace_pc_range = Some(parse_pc_range_arg(value)?);
                i += 2;
            }
            "--trace-opcode" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--trace-opcode requires a value");
                };
                for raw in value.split(',') {
                    let opcode = parse_u8_arg(raw, "--trace-opcode")?;
                    if !headless.trace_opcode_filter.contains(&opcode) {
                        headless.trace_opcode_filter.push(opcode);
                    }
                }
                i += 2;
            }
            "--trace-watch-interrupts" => {
                headless.trace_watch_interrupts = true;
                i += 1;
            }
            "--trace-bus" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--trace-bus requires address ranges");
                };
                headless.trace_bus_filters.extend(parse_addr_range_list_arg(
                    value,
                    "--trace-bus",
                    HeadlessBusTraceAccess::ReadWrite,
                )?);
                i += 2;
            }
            "--trace-bus-read" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--trace-bus-read requires address ranges");
                };
                headless.trace_bus_filters.extend(parse_addr_range_list_arg(
                    value,
                    "--trace-bus-read",
                    HeadlessBusTraceAccess::Read,
                )?);
                i += 2;
            }
            "--trace-bus-write" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--trace-bus-write requires address ranges");
                };
                headless.trace_bus_filters.extend(parse_addr_range_list_arg(
                    value,
                    "--trace-bus-write",
                    HeadlessBusTraceAccess::Write,
                )?);
                i += 2;
            }
            "--trace-bus-limit" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--trace-bus-limit requires a numeric value");
                };
                headless.trace_bus_limit = parse_u64_arg(value, "--trace-bus-limit")?;
                i += 2;
            }
            "--dump-mem" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--dump-mem requires addr:len");
                };
                headless
                    .memory_dumps
                    .push(parse_memory_dump_arg(value, "--dump-mem")?);
                i += 2;
            }
            "--dump-region" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--dump-region requires region:offset:len");
                };
                headless
                    .region_dumps
                    .push(parse_region_dump_arg(value, "--dump-region")?);
                i += 2;
            }
            "--break-at" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--break-at requires an address value");
                };
                headless.break_at = Some(parse_u16_arg(value, "--break-at")?);
                i += 2;
            }
            "--no-apu" => {
                headless.no_apu = true;
                i += 1;
            }
            "--no-sram" | "--no-battery" => {
                headless.no_sram = true;
                i += 1;
            }
            "--apply-mods" => {
                headless.apply_mods = true;
                i += 1;
            }
            "--gb-dmg-palette" | "--dmg-palette" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!(
                        "{} requires one of: gray|dmg-green|pocket|mint|chocolate",
                        args[i]
                    );
                };
                headless.gb_dmg_palette_preset = Some(parse_dmg_palette_arg(value, &args[i])?);
                i += 2;
            }
            "--sega8-video-standard" | "--sega8-region" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("{} requires one of: auto|ntsc|pal|60hz|50hz", args[i]);
                };
                headless.sega8_video_standard = if value.eq_ignore_ascii_case("auto") {
                    None
                } else {
                    Some(parse_sega8_video_standard_arg(value, &args[i])?)
                };
                i += 2;
            }
            "--sega8-console-region" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!(
                        "{} requires one of: auto|export|international|japanese|japan|pbc|power-base",
                        args[i]
                    );
                };
                headless.sega8_console_region = if value.eq_ignore_ascii_case("auto") {
                    None
                } else {
                    Some(parse_sega8_console_region_arg(value, &args[i])?)
                };
                i += 2;
            }
            "--pce-controller" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!(
                        "--pce-controller requires auto, pad, six-button, mouse, or multitap"
                    );
                };
                headless.pce_controller_mode =
                    Some(parse_pce_controller_mode_arg(value, "--pce-controller")?);
                i += 2;
            }
            "--pce-memory-base" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--pce-memory-base requires auto, enabled, or disabled");
                };
                headless.pce_memory_base_mode =
                    Some(parse_pce_memory_base_mode_arg(value, "--pce-memory-base")?);
                i += 2;
            }
            "--pce-arcade-card" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--pce-arcade-card requires auto, enabled, or disabled");
                };
                headless.pce_arcade_card_mode =
                    Some(parse_pce_arcade_card_mode_arg(value, "--pce-arcade-card")?);
                i += 2;
            }
            "--detect-stuck" => {
                if headless.stuck_window_frames == 0 {
                    headless.stuck_window_frames = 120;
                }
                i += 1;
            }
            "--stuck-window" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--stuck-window requires a numeric value");
                };
                headless.stuck_window_frames = parse_u64_arg(value, "--stuck-window")?;
                i += 2;
            }
            "--stuck-pc-threshold" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--stuck-pc-threshold requires a numeric value");
                };
                headless.stuck_pc_threshold = parse_usize_arg(value, "--stuck-pc-threshold")?;
                i += 2;
            }
            "--fail-on-stuck" => {
                headless.fail_on_stuck = true;
                i += 1;
            }
            "--press" | "--input" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("{} requires an input spec", args[i]);
                };
                headless
                    .input_events
                    .extend(parse_input_event_arg(value, args[i].as_str())?);
                i += 2;
            }
            "--press-p2" | "--input-p2" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("{} requires an input spec", args[i]);
                };
                headless
                    .input_events_p2
                    .extend(parse_input_event_arg(value, args[i].as_str())?);
                i += 2;
            }
            "--press-p3" | "--input-p3" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("{} requires an input spec", args[i]);
                };
                headless
                    .input_events_p3
                    .extend(parse_input_event_arg(value, args[i].as_str())?);
                i += 2;
            }
            "--press-p4" | "--input-p4" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("{} requires an input spec", args[i]);
                };
                headless
                    .input_events_p4
                    .extend(parse_input_event_arg(value, args[i].as_str())?);
                i += 2;
            }
            "--press-p5" | "--input-p5" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("{} requires an input spec", args[i]);
                };
                headless
                    .input_events_p5
                    .extend(parse_input_event_arg(value, args[i].as_str())?);
                i += 2;
            }
            "--input-script" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--input-script requires a file path");
                };
                headless.input_events.extend(parse_input_script(value)?);
                i += 2;
            }
            "--input-script-p2" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--input-script-p2 requires a file path");
                };
                headless.input_events_p2.extend(parse_input_script(value)?);
                i += 2;
            }
            "--input-script-p3" | "--input-script-p4" | "--input-script-p5" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("{} requires a file path", args[i]);
                };
                let events = parse_input_script(value)?;
                match args[i].as_str() {
                    "--input-script-p3" => headless.input_events_p3.extend(events),
                    "--input-script-p4" => headless.input_events_p4.extend(events),
                    "--input-script-p5" => headless.input_events_p5.extend(events),
                    _ => unreachable!(),
                }
                i += 2;
            }
            "--zapper" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--zapper requires an event spec");
                };
                headless
                    .zapper_events
                    .extend(parse_zapper_event_arg(value, "--zapper")?);
                i += 2;
            }
            "--load-state" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--load-state requires a file path");
                };
                headless.load_state_path = Some(PathBuf::from(value));
                i += 2;
            }
            "--pce-save-state-out" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--pce-save-state-out requires a file path");
                };
                headless.pce_save_state_path = Some(PathBuf::from(value));
                i += 2;
            }
            "--replay" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--replay requires a replay file path");
                };
                headless.replay_path = Some(PathBuf::from(value));
                i += 2;
            }
            "--replay-peer" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--replay-peer requires a replay file path");
                };
                headless.replay_peer_path = Some(PathBuf::from(value));
                i += 2;
            }
            "--replay-peer-live-link" => {
                headless.replay_peer_live_link = true;
                i += 1;
            }
            "--replay-tail-frames" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--replay-tail-frames requires a numeric value");
                };
                headless.replay_tail_frames = parse_u64_arg(value, "--replay-tail-frames")?;
                i += 2;
            }
            "--expect-gb-link-events" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--expect-gb-link-events requires a numeric value");
                };
                headless.expect_gb_link_events = parse_u64_arg(value, "--expect-gb-link-events")?;
                i += 2;
            }
            "--allow-gb-link-replay-divergence" => {
                headless.allow_gb_link_replay_divergence = true;
                i += 1;
            }
            "--expect-replay-final-hash" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--expect-replay-final-hash requires a SHA-256 hex digest");
                };
                headless.expect_replay_final_hash = Some(value.to_string());
                i += 2;
            }
            "--screenshot" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--screenshot requires a file path");
                };
                headless.screenshot_path = Some(PathBuf::from(value));
                i += 2;
            }
            "--screenshot-frame" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--screenshot-frame requires a numeric value");
                };
                headless.screenshot_frame = Some(parse_u64_arg(value, "--screenshot-frame")?);
                i += 2;
            }
            "--screenshot-dir" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--screenshot-dir requires a directory path");
                };
                headless.screenshot_dir = Some(PathBuf::from(value));
                i += 2;
            }
            "--screenshot-every" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--screenshot-every requires a numeric frame interval");
                };
                headless.screenshot_every = parse_u64_arg(value, "--screenshot-every")?;
                i += 2;
            }
            "--debug-state" => {
                headless.print_debug_state = true;
                i += 1;
            }
            "--debug-state-out" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--debug-state-out requires a file path");
                };
                headless.debug_state_path = Some(PathBuf::from(value));
                i += 2;
            }
            "--audio-dump" | "--audio-out" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("{} requires a file path", args[i]);
                };
                headless.audio_dump_path = Some(PathBuf::from(value));
                i += 2;
            }
            "--break-on-gba-bad-state" => {
                headless.break_on_gba_bad_state = true;
                i += 1;
            }
            "--gba-mute-audio" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--gba-mute-audio requires a comma-separated channel list");
                };
                let mutes = parse_gba_audio_mute_list_arg(value, "--gba-mute-audio")?;
                for (dst, src) in headless.gba_audio_mutes.iter_mut().zip(mutes) {
                    *dst |= src;
                }
                i += 2;
            }
            "--gba-hide-bg" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--gba-hide-bg requires a comma-separated BG layer list");
                };
                let hidden = parse_gba_bg_layer_list_arg(value, "--gba-hide-bg")?;
                for (dst, src) in headless.gba_hidden_bg_layers.iter_mut().zip(hidden) {
                    *dst |= src;
                }
                i += 2;
            }
            "--gba-hide-sprites" => {
                headless.gba_hide_sprites = true;
                i += 1;
            }
            "--gba-dump-memory" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--gba-dump-memory requires a directory path");
                };
                headless.gba_dump_memory_dir = Some(PathBuf::from(value));
                i += 2;
            }
            other => {
                if rom_path.is_none() {
                    rom_path = Some(other.to_string());
                }
                i += 1;
            }
        }
    }

    if headless.replay_peer_path.is_some() && headless.replay_path.is_none() {
        anyhow::bail!("--replay-peer requires --replay");
    }
    if headless.replay_peer_live_link && headless.replay_peer_path.is_none() {
        anyhow::bail!("--replay-peer-live-link requires --replay-peer");
    }

    Ok(CliArgs {
        rom_path,
        mode_override,
        headless: if headless_enabled {
            Some(headless)
        } else {
            None
        },
    })
}
