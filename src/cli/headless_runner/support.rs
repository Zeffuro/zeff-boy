use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::emu_backend::ActiveSystem;

use super::HeadlessOptions;

pub(super) fn load_headless_rom(path: &Path) -> anyhow::Result<(PathBuf, Vec<u8>, ActiveSystem)> {
    let (rom_path, preloaded_data, system) = crate::app::detect_and_extract_rom(path)?;
    let rom_data = match preloaded_data {
        Some(data) => data,
        None => std::fs::read(path)?,
    };
    Ok((rom_path, rom_data, system))
}

pub(super) fn ensure_system_headless_options(
    system: &str,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    if opts.expect_serial.is_some() {
        anyhow::bail!("--expect-serial is only supported for GB/GBC headless runs");
    }
    if opts.expect_ws_text.is_some() && system != "ws" {
        anyhow::bail!("--expect-ws-text is only supported for WonderSwan headless runs");
    }
    if opts.expect_ws_pass_fail_tiles && system != "ws" {
        anyhow::bail!("--expect-ws-pass-fail-tiles is only supported for WonderSwan headless runs");
    }
    if opts.expect_sega8_sdsc.is_some() && !matches!(system, "sms" | "gg" | "sg") {
        anyhow::bail!("--expect-sega8-sdsc is only supported for Sega 8-bit headless runs");
    }
    if opts.expect_sega8_audio && !matches!(system, "sms" | "gg" | "sg") {
        anyhow::bail!("--expect-sega8-audio is only supported for Sega 8-bit headless runs");
    }
    if !opts.memory_dumps.is_empty() && system != "ws" {
        anyhow::bail!("--dump-mem is only supported for GB/GBC and WonderSwan headless runs");
    }
    if opts.break_at.is_some() {
        anyhow::bail!("--break-at is only supported for GB/GBC headless runs");
    }

    if system == "gba" {
        let unsupported_gba_trace =
            !opts.trace_opcode_filter.is_empty() || opts.trace_watch_interrupts;
        if unsupported_gba_trace {
            anyhow::bail!(
                "GBA headless tracing supports --trace-opcodes, --trace-opcode-limit, --trace-start-t, --trace-pc-range, and --trace-bus/--trace-bus-read/--trace-bus-write"
            );
        }
    }
    if system == "nes" && opts.trace_watch_interrupts {
        anyhow::bail!("--trace-watch-interrupts is only supported for GB/GBC headless runs");
    }
    if system != "gba"
        && (opts.gba_hidden_bg_layers.iter().any(|&hidden| hidden) || opts.gba_hide_sprites)
    {
        anyhow::bail!(
            "--gba-hide-bg and --gba-hide-sprites are only supported for GBA headless runs"
        );
    }
    if system != "gba" && opts.gba_dump_memory_dir.is_some() {
        anyhow::bail!("--gba-dump-memory is only supported for GBA headless runs");
    }
    if system != "gba" && opts.gba_audio_mutes.iter().any(|&muted| muted) {
        anyhow::bail!("--gba-mute-audio is only supported for GBA headless runs");
    }
    if !matches!(system, "gba" | "ws" | "sms" | "gg" | "sg") && opts.audio_dump_path.is_some() {
        anyhow::bail!(
            "--audio-dump is currently only supported for GBA, WonderSwan, and Sega 8-bit headless runs"
        );
    }
    if opts.gb_dmg_palette_preset.is_some() {
        anyhow::bail!("--gb-dmg-palette/--dmg-palette is only supported for GB/GBC headless runs");
    }

    log::info!("Running {system} headless smoke test");
    Ok(())
}

pub(super) fn read_headless_state_if_requested(
    opts: &HeadlessOptions,
) -> anyhow::Result<Option<Vec<u8>>> {
    let Some(path) = &opts.load_state_path else {
        return Ok(None);
    };
    fs::read(path)
        .map(Some)
        .map_err(|err| anyhow::anyhow!("failed to read save state {}: {err}", path.display()))
}

pub(super) fn ensure_no_reset_events(system: &str, opts: &HeadlessOptions) -> anyhow::Result<()> {
    if opts.input_events.iter().any(|event| event.reset) {
        anyhow::bail!("reset input events are not supported for {system} headless runs yet");
    }
    Ok(())
}

pub(super) fn print_perf(system: &str, frames_run: u64, start: Instant) {
    let elapsed = start.elapsed();
    let fps = if elapsed.is_zero() {
        0.0
    } else {
        frames_run as f64 / elapsed.as_secs_f64()
    };
    println!(
        "[headless] system={} elapsed_ms={} fps={:.0}",
        system,
        elapsed.as_millis(),
        fps
    );
}

pub(super) fn flush_battery(path: &Path, sram_bytes: Option<Vec<u8>>) {
    match crate::save_paths::flush_battery_sram(path, sram_bytes) {
        Ok(Some(save_path)) => log::info!("Saved battery RAM to {}", save_path),
        Ok(None) => {}
        Err(err) => log::error!("Failed to save battery RAM: {}", err),
    }
}
