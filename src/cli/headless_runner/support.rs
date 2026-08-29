use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::emu_backend::ActiveSystem;
use crate::emu_core_trait::EmulatorCore;

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
    if opts.ws_link_peer_path.is_some() && system != "ws" {
        anyhow::bail!("--ws-link-peer is only supported for WonderSwan headless runs");
    }
    if opts.expect_ws_link_bytes != 0 && system != "ws" {
        anyhow::bail!("--expect-ws-link-bytes is only supported for WonderSwan headless runs");
    }
    if opts.expect_sega8_sdsc.is_some() && !matches!(system, "sms" | "gg" | "sg") {
        anyhow::bail!("--expect-sega8-sdsc is only supported for Sega 8-bit headless runs");
    }
    if opts.expect_sega8_audio && !matches!(system, "sms" | "gg" | "sg") {
        anyhow::bail!("--expect-sega8-audio is only supported for Sega 8-bit headless runs");
    }
    if opts.expect_coleco_audio && system != "coleco" {
        anyhow::bail!("--expect-coleco-audio is only supported for ColecoVision headless runs");
    }
    if opts.sega8_video_standard.is_some() && !matches!(system, "sms" | "gg" | "sg") {
        anyhow::bail!(
            "--sega8-video-standard/--sega8-region is only supported for Sega 8-bit headless runs"
        );
    }
    if opts.sega8_console_region.is_some() && !matches!(system, "sms" | "gg" | "sg") {
        anyhow::bail!("--sega8-console-region is only supported for Sega 8-bit headless runs");
    }
    if opts.pce_controller_mode.is_some() && system != "pce" {
        anyhow::bail!("--pce-controller is only supported for PC Engine headless runs");
    }
    if opts.pce_memory_base_mode.is_some() && system != "pce" {
        anyhow::bail!("--pce-memory-base is only supported for PC Engine headless runs");
    }
    if opts.pce_arcade_card_mode.is_some() && system != "pce" {
        anyhow::bail!("--pce-arcade-card is only supported for PC Engine headless runs");
    }
    if opts.pce_save_state_path.is_some() && system != "pce" {
        anyhow::bail!("--pce-save-state-out is only supported for PC Engine headless runs");
    }
    if opts.coleco_save_state_path.is_some() && system != "coleco" {
        anyhow::bail!("--coleco-save-state-out is only supported for ColecoVision headless runs");
    }
    if has_coleco_keypad_input(opts) && system != "coleco" {
        anyhow::bail!("Coleco keypad input is only supported for ColecoVision headless runs");
    }
    if !opts.input_events_p2.is_empty()
        && !(matches!(system, "nes" | "coleco" | "pce" | "sms" | "gg" | "sg")
            || (system == "ws" && opts.ws_link_peer_path.is_some()))
    {
        anyhow::bail!(
            "--press-p2/--input-p2 is only supported for NES, ColecoVision, PC Engine, Sega 8-bit, and WonderSwan --ws-link-peer headless runs"
        );
    }
    if (!opts.input_events_p3.is_empty()
        || !opts.input_events_p4.is_empty()
        || !opts.input_events_p5.is_empty())
        && system != "pce"
    {
        anyhow::bail!(
            "--press-p3/--press-p4/--press-p5 are only supported for PC Engine headless runs"
        );
    }
    if !opts.memory_dumps.is_empty() && !matches!(system, "pce" | "ws") {
        anyhow::bail!(
            "--dump-mem is only supported for GB/GBC, PC Engine, and WonderSwan headless runs"
        );
    }
    if !opts.region_dumps.is_empty() && system != "pce" {
        anyhow::bail!("--dump-region is only supported for PC Engine headless runs");
    }
    if opts.break_at.is_some() && system != "pce" {
        anyhow::bail!("--break-at is only supported for GB/GBC and PC Engine headless runs");
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
    if !matches!(
        system,
        "gba" | "coleco" | "pce" | "ws" | "sms" | "gg" | "sg"
    ) && opts.audio_dump_path.is_some()
    {
        anyhow::bail!(
            "--audio-dump is currently only supported for GBA, ColecoVision, PC Engine, WonderSwan, and Sega 8-bit headless runs"
        );
    }
    if opts.gb_dmg_palette_preset.is_some() {
        anyhow::bail!("--gb-dmg-palette/--dmg-palette is only supported for GB/GBC headless runs");
    }

    log::info!("Running {system} headless smoke test");
    Ok(())
}

fn has_coleco_keypad_input(opts: &HeadlessOptions) -> bool {
    [
        &opts.input_events,
        &opts.input_events_p2,
        &opts.input_events_p3,
        &opts.input_events_p4,
        &opts.input_events_p5,
    ]
    .into_iter()
    .flatten()
    .any(|event| event.coleco_keypad.is_some())
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
    if opts.input_events.iter().any(|event| event.reset)
        || opts.input_events_p2.iter().any(|event| event.reset)
        || opts.input_events_p3.iter().any(|event| event.reset)
        || opts.input_events_p4.iter().any(|event| event.reset)
        || opts.input_events_p5.iter().any(|event| event.reset)
    {
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

pub(super) fn print_memory_region_dumps(
    emulator: &mut impl EmulatorCore,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    let mut bytes = Vec::new();
    for dump in &opts.region_dumps {
        let region = emulator.copy_memory_region(&dump.region, &mut bytes)?;
        let lines = memory_region_dump_lines(region.id, &bytes, dump.offset, dump.len)?;
        for line in lines {
            println!("{line}");
        }
    }
    Ok(())
}

fn memory_region_dump_lines(
    region: &str,
    bytes: &[u8],
    offset: usize,
    len: usize,
) -> anyhow::Result<Vec<String>> {
    let end = offset
        .checked_add(len)
        .ok_or_else(|| anyhow::anyhow!("memory region dump range overflows usize"))?;
    anyhow::ensure!(
        end <= bytes.len(),
        "memory region '{region}' range 0x{offset:X}..0x{end:X} exceeds its 0x{:X}-byte size",
        bytes.len()
    );

    let mut lines = vec![format!(
        "[region] id={region} offset={offset:08X} len={len}"
    )];
    for (line_index, chunk) in bytes[offset..end].chunks(16).enumerate() {
        let address = offset + line_index * 16;
        let values = chunk
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(" ");
        lines.push(format!("[region] {address:08X}: {values}"));
    }
    Ok(lines)
}

pub(super) fn flush_battery(
    recovery_session: &mut crate::save_paths::SramRecoverySession,
    path: &Path,
    system: ActiveSystem,
    media_identity: [u8; 32],
    sram_bytes: Option<Vec<u8>>,
) {
    match crate::save_paths::flush_battery_sram(
        recovery_session,
        path,
        system.storage_subdir(),
        media_identity,
        sram_bytes,
    ) {
        Ok(Some(save_path)) => log::info!("Saved battery RAM to {}", save_path),
        Ok(None) => {}
        Err(err) => log::error!("Failed to save battery RAM: {}", err),
    }
}

#[cfg(test)]
mod tests {
    use super::{ensure_system_headless_options, memory_region_dump_lines};
    use crate::cli::types::{HeadlessOptions, HeadlessRegionDump};

    #[test]
    fn region_dump_formats_only_the_requested_range() {
        let bytes = (0..32).collect::<Vec<u8>>();
        let lines = memory_region_dump_lines("video_ram", &bytes, 14, 4).unwrap();

        assert_eq!(lines[0], "[region] id=video_ram offset=0000000E len=4");
        assert_eq!(lines[1], "[region] 0000000E: 0E 0F 10 11");
    }

    #[test]
    fn region_dump_rejects_out_of_bounds_and_overflowing_ranges() {
        assert!(memory_region_dump_lines("video_ram", &[0; 8], 7, 2).is_err());
        assert!(memory_region_dump_lines("video_ram", &[0; 8], usize::MAX, 2).is_err());
    }

    #[test]
    fn region_dump_is_rejected_until_a_headless_system_wires_it() {
        let mut opts = HeadlessOptions::default();
        opts.region_dumps.push(HeadlessRegionDump {
            region: "video_ram".to_owned(),
            offset: 0,
            len: 1,
        });

        ensure_system_headless_options("pce", &opts).unwrap();
        assert!(ensure_system_headless_options("gba", &opts).is_err());
    }

    #[test]
    fn pce_state_output_is_rejected_until_a_headless_system_wires_it() {
        let opts = HeadlessOptions {
            pce_save_state_path: Some(std::path::PathBuf::from("target/endpoint.pcestate")),
            ..HeadlessOptions::default()
        };

        ensure_system_headless_options("pce", &opts).unwrap();
        let error = ensure_system_headless_options("gba", &opts).unwrap_err();
        assert!(error.to_string().contains("--pce-save-state-out"));
    }

    #[test]
    fn coleco_state_output_is_rejected_until_the_coleco_headless_route_wires_it() {
        let opts = HeadlessOptions {
            coleco_save_state_path: Some(std::path::PathBuf::from("target/endpoint.colstate")),
            ..HeadlessOptions::default()
        };

        ensure_system_headless_options("coleco", &opts).unwrap();
        let error = ensure_system_headless_options("gba", &opts).unwrap_err();
        assert!(error.to_string().contains("--coleco-save-state-out"));
    }
}
