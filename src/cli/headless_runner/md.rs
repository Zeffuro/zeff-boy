use std::io::Read;
use std::path::Path;
use std::time::Instant;

use zeff_emu_common::time::{FrameLifecycle, MachineTiming};

use super::*;
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use crate::emu_core_trait::EmulatorCore;

pub(super) fn run_md_headless(
    source: &Path,
    path: &Path,
    bytes: Option<Vec<u8>>,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    validate_options(opts)?;
    let loaded = load_backend_from_rom_source(
        ActiveSystem::Md,
        source,
        path,
        bytes,
        BackendLoadConfig {
            apply_mods: opts.apply_mods,
            md_region: opts.md_region,
            md_controller: opts
                .md_controller
                .unwrap_or(zeff_md_core::ControllerKind::SixButton),
            ..Default::default()
        },
    )?;
    let EmuBackend::Md(mut backend) = loaded.backend else {
        unreachable!()
    };
    if let Some(path) = &opts.load_state_path {
        let mut state = Vec::new();
        std::fs::File::open(path)?
            .take(zeff_md_core::constants::MAX_STATE_BYTES as u64 + 1)
            .read_to_end(&mut state)?;
        backend.load_state_from_bytes(state)?;
    }
    backend.set_apu_sample_generation_enabled(!opts.no_apu);
    let mut audio = Vec::new();
    let mut dump = Vec::new();
    let mut stats = AudioStats::default();
    let mut screenshot = false;
    let mut stuck = StuckTracker::from_options(opts);
    let mut stuck_active = false;
    let start = Instant::now();
    let initial = backend.emu.framebuffer_dimensions();
    write_screenshot_if_requested(opts, 0, backend.framebuffer(), initial, &mut screenshot)?;
    write_screenshot_sequence_if_requested(opts, 0, backend.framebuffer(), initial)?;
    for frame in 0..opts.max_frames {
        let first = input_for_frame(opts, frame + 1);
        let second = input_p2_for_frame(opts, frame + 1);
        backend.set_input(first.buttons, first.dpad);
        backend.set_input_p2(second.buttons, second.dpad);
        backend.step_frame();
        if let Some(fault) = backend.take_runtime_fault() {
            anyhow::bail!("{fault}");
        }
        backend.drain_audio_samples_into(&mut audio);
        stats.observe(&audio);
        if opts.audio_dump_path.is_some() {
            dump.extend_from_slice(&audio);
        }
        audio.clear();
        let dimensions = backend.emu.framebuffer_dimensions();
        write_screenshot_if_requested(
            opts,
            frame + 1,
            backend.framebuffer(),
            dimensions,
            &mut screenshot,
        )?;
        write_screenshot_sequence_if_requested(opts, frame + 1, backend.framebuffer(), dimensions)?;
        observe_stuck(
            &mut stuck,
            "md",
            6,
            frame + 1,
            u64::from(backend.emu.registers().pc),
            backend.framebuffer(),
            None,
            None,
            false,
            &mut stuck_active,
        );
    }
    fail_on_stuck_if_needed("md", stuck.as_ref(), opts)?;
    let dimensions = backend.emu.framebuffer_dimensions();
    write_final_screenshot_if_needed(
        opts,
        opts.max_frames,
        backend.framebuffer(),
        dimensions,
        &mut screenshot,
    )?;
    println!(
        "[headless] system=md rom={} frames={} ticks={} pc={:06X} region={:?} status=ok",
        path.display(),
        opts.max_frames,
        backend.timing_snapshot().now().get(),
        backend.emu.registers().pc,
        backend.emu.region()
    );
    print_perf("md", opts.max_frames, start);
    emit_debug_state(
        opts,
        serde_json::json!({
            "system":"md","frames":opts.max_frames,"ticks":backend.timing_snapshot().now().get(),
            "pc":format!("{:06X}",backend.emu.registers().pc),"region":format!("{:?}",backend.emu.region()),
            "registers":{"d":backend.emu.registers().d,"a":backend.emu.registers().a},
            "video":{"width":dimensions.0,"height":dimensions.1,"presented_frame":backend.emu.frame_count(),"error":format!("{:?}",backend.emu.video_error())},
            "audio":{"sample_rate":backend.emu.sample_rate(),"samples":stats.sample_count,"nonzero_samples":stats.nonzero_samples,"peak_abs":stats.peak_abs},
            "capabilities":{"save_states":true,"rewind":false,"replay":false,"debugger":false}
        }),
    )?;
    if let Some(path) = &opts.md_save_state_path {
        let state = backend.encode_state_bytes()?;
        let path = write_md_state_artifact(path, &state)?;
        println!(
            "[headless] md-save-state={} bytes={}",
            path.display(),
            state.len()
        );
    }
    if let Some(path) = &opts.audio_dump_path {
        write_audio_dump_f32le(path, &dump, backend.emu.sample_rate())?;
    }
    if let Some(path) = backend.flush_battery_sram()? {
        println!("[headless] md-save-ram={path}");
    }
    Ok(())
}

fn validate_options(opts: &HeadlessOptions) -> anyhow::Result<()> {
    ensure_system_headless_options("md", opts)?;
    ensure_no_reset_events("md", opts)?;
    anyhow::ensure!(
        opts.tas_script.is_none() && opts.replay_path.is_none(),
        "Mega Drive replay and TAS execution are not supported yet"
    );
    anyhow::ensure!(
        !opts.trace_opcodes
            && opts.trace_pc_range.is_none()
            && opts.trace_opcode_filter.is_empty()
            && !opts.trace_watch_interrupts
            && opts.trace_bus_filters.is_empty(),
        "Mega Drive instruction and bus tracing are not supported yet"
    );
    anyhow::ensure!(
        opts.expect_gb_link_events == 0
            && opts.expect_replay_final_hash.is_none()
            && opts.replay_peer_path.is_none()
            && !opts.replay_peer_live_link
            && opts.replay_tail_frames == 0
            && !opts.allow_gb_link_replay_divergence,
        "Mega Drive link and replay expectations are not supported yet"
    );
    anyhow::ensure!(
        opts.zapper_events.is_empty() && !opts.break_on_gba_bad_state,
        "Mega Drive lightgun and GBA debug options are not supported"
    );
    anyhow::ensure!(
        !opts.expect_test_pass,
        "Mega Drive test-exit detection is not supported yet"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn md_headless_publishes_guest_save_writes_at_successful_exit() -> anyhow::Result<()> {
        let directory = crate::test_support::test_directory("md-headless-save")?;
        let path = directory.path().join("save.md");
        let mut bytes = vec![0; 0x400];
        bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
        bytes[0x1b0..0x1bc]
            .copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20, 3, 0xff]);
        bytes[0x200..0x22c].copy_from_slice(&[
            0x13, 0xfc, 0, 1, 0, 0xa1, 0x30, 0xf1, 0x13, 0xfc, 0, 0x6d, 0, 0x20, 0, 1, 0x10, 0x39,
            0, 0xa1, 0, 3, 0x13, 0xc0, 0, 0x20, 0, 3, 0x10, 0x39, 0, 0xa1, 0, 5, 0x13, 0xc0, 0,
            0x20, 0, 5, 0x60, 0xfe, 0x4e, 0x71,
        ]);
        run_md_headless(
            &path,
            &path,
            Some(bytes),
            &HeadlessOptions {
                max_frames: 1,
                no_apu: true,
                input_events: crate::cli::parse::parse_input_event_arg("right@1", "--press")?,
                input_events_p2: crate::cli::parse::parse_input_event_arg("left@1", "--press-p2")?,
                ..Default::default()
            },
        )?;
        let mut expected = vec![0xff; 512];
        expected[0] = 0x6d;
        expected[1] = 0x77;
        expected[2] = 0x7b;
        assert_eq!(std::fs::read(path.with_extension("sav"))?, expected);
        Ok(())
    }
    #[test]
    fn md_headless_accepts_pcm_and_rejects_unavailable_execution_features() {
        assert!(
            validate_options(&HeadlessOptions {
                audio_dump_path: Some("audio.f32".into()),
                md_region: Some(zeff_md_core::Region::Pal),
                load_state_path: Some("state.mdstate".into()),
                md_save_state_path: Some("saved.mdstate".into()),
                ..Default::default()
            })
            .is_ok()
        );
        for options in [
            HeadlessOptions {
                expect_gb_link_events: 1,
                ..Default::default()
            },
            HeadlessOptions {
                expect_replay_final_hash: Some("00".repeat(32)),
                ..Default::default()
            },
            HeadlessOptions {
                break_on_gba_bad_state: true,
                ..Default::default()
            },
            HeadlessOptions {
                trace_opcodes: true,
                ..Default::default()
            },
            HeadlessOptions {
                zapper_events: vec![crate::cli::types::HeadlessZapperEvent {
                    start_frame: 0,
                    end_frame: 1,
                    x: 1,
                    y: 1,
                    trigger: true,
                    hit: false,
                }],
                ..Default::default()
            },
        ] {
            assert!(validate_options(&options).is_err());
            let missing = Path::new("missing.md");
            assert!(
                !run_md_headless(missing, missing, None, &options)
                    .unwrap_err()
                    .to_string()
                    .contains("os error")
            );
        }
    }
}
