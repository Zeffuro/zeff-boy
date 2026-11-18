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
    if opts.replay_path.is_some() {
        return super::replay::run_md_replay_headless(source, path, bytes, opts);
    }
    super::audio_trace::validate_options(opts)?;
    super::audio_trace::validate_system("md", opts)?;
    let bytes = if opts.audio_trace_path.is_some() {
        Some(match bytes {
            Some(bytes) => bytes,
            None => {
                let mut bytes = Vec::new();
                std::fs::File::open(source)?
                    .take(zeff_md_core::constants::MAX_CARTRIDGE_SIZE as u64 + 1)
                    .read_to_end(&mut bytes)?;
                anyhow::ensure!(
                    bytes.len() <= zeff_md_core::constants::MAX_CARTRIDGE_SIZE,
                    "Mega Drive cartridge exceeds the supported 4 MiB limit"
                );
                bytes
            }
        })
    } else {
        bytes
    };
    let capture = bytes
        .as_ref()
        .map(|bytes| super::audio_trace::Capture::prepare(source, path, bytes, opts))
        .transpose()?
        .flatten();
    let loaded = load_backend_from_rom_source(
        ActiveSystem::Md,
        source,
        path,
        bytes,
        BackendLoadConfig {
            apply_mods: opts.apply_mods,
            md_region: opts.md_region,
            md_timing: opts.md_timing.unwrap_or_default(),
            md_load_save_ram: capture.is_none() && !opts.no_sram,
            md_host_persistence: capture.is_none() && !opts.no_sram,
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
    if capture.is_some() {
        backend
            .reset_and_begin_audio_trace(zeff_emu_common::audio_trace::MAX_AUDIO_TRACE_EVENTS)?;
        backend.set_controllers(
            opts.md_controller
                .unwrap_or(zeff_md_core::ControllerKind::SixButton),
        );
    }
    let mut audio = Vec::new();
    let mut dump = Vec::new();
    let mut stats = AudioStats::default();
    let mut screenshot = false;
    let mut unavailable_captures = 0_u64;
    let mut stuck = StuckTracker::from_options(opts);
    let mut stuck_active = false;
    let start = Instant::now();
    let initial = backend.core().framebuffer_dimensions();
    if backend.core().framebuffer_available() {
        write_screenshot_if_requested(opts, 0, backend.framebuffer(), initial, &mut screenshot)?;
        write_screenshot_sequence_if_requested(opts, 0, backend.framebuffer(), initial)?;
    } else if capture_requested(opts, 0) {
        unavailable_captures += 1;
    }
    for frame in 0..opts.max_frames {
        let first = input_for_frame(opts, frame + 1);
        let second = input_p2_for_frame(opts, frame + 1);
        if first.reset || second.reset {
            backend
                .try_warm_reset()
                .map_err(|error| anyhow::anyhow!("Mega Drive warm reset: {error:?}"))?;
        }
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
        let dimensions = backend.core().framebuffer_dimensions();
        if backend.core().framebuffer_available() {
            write_screenshot_if_requested(
                opts,
                frame + 1,
                backend.framebuffer(),
                dimensions,
                &mut screenshot,
            )?;
            write_screenshot_sequence_if_requested(
                opts,
                frame + 1,
                backend.framebuffer(),
                dimensions,
            )?;
            observe_stuck(
                &mut stuck,
                "md",
                6,
                frame + 1,
                u64::from(backend.core().registers().pc),
                backend.framebuffer(),
                None,
                None,
                false,
                &mut stuck_active,
            );
        } else {
            stuck = StuckTracker::from_options(opts);
            stuck_active = false;
            if capture_requested(opts, frame + 1) {
                unavailable_captures += 1;
            }
        }
    }
    fail_on_stuck_if_needed("md", stuck.as_ref(), opts)?;
    let dimensions = backend.core().framebuffer_dimensions();
    if backend.core().framebuffer_available() {
        write_final_screenshot_if_needed(
            opts,
            opts.max_frames,
            backend.framebuffer(),
            dimensions,
            &mut screenshot,
        )?;
    } else if !screenshot
        && opts.screenshot_path.is_some()
        && opts
            .screenshot_frame
            .is_none_or(|frame| frame > opts.max_frames)
    {
        unavailable_captures += 1;
    }
    println!(
        "[headless] system=md rom={} frames={} ticks={} pc={:06X} region={:?} status=ok video_available={}",
        path.display(),
        opts.max_frames,
        backend.timing_snapshot().now().get(),
        backend.core().registers().pc,
        backend.core().region(),
        backend.core().framebuffer_available()
    );
    print_perf("md", opts.max_frames, start);
    emit_debug_state(
        opts,
        serde_json::json!({
            "system":"md","frames":opts.max_frames,"ticks":backend.timing_snapshot().now().get(),
            "pc":format!("{:06X}",backend.core().registers().pc),"region":format!("{:?}",backend.core().region()),
            "timing":format!("{:?}",backend.core().timing_model()),
            "registers":{"d":backend.core().registers().d,"a":backend.core().registers().a},
            "video":{"width":dimensions.0,"height":dimensions.1,"available":backend.core().framebuffer_available(),"unavailable_captures":unavailable_captures,"presented_frame":backend.core().frame_count(),"error":format!("{:?}",backend.core().video_error())},
            "audio":{"sample_rate":backend.core().sample_rate(),"samples":stats.sample_count,"nonzero_samples":stats.nonzero_samples,"peak_abs":stats.peak_abs},
            "capabilities":{
                "save_states":backend.supports_save_states(),
                "rewind":backend.supports_rewind(),
                "replay":backend.supports_replay(),
                "debugger":backend.supports_debugger()
            }
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
        write_audio_dump_f32le(path, &dump, backend.core().sample_rate())?;
    }
    if let Some(capture) = capture {
        let settings = serde_json::json!({
            "region": format!("{:?}", backend.core().region()),
            "timing": format!("{:?}", backend.core().timing_model()),
            "controller": format!("{:?}", opts.md_controller.unwrap_or(zeff_md_core::ControllerKind::SixButton)),
            "frame_count_unit": "pacing_fields",
            "sample_rate": backend.core().sample_rate(),
        });
        capture.finish_md(backend.finish_audio_trace()?, opts.max_frames, settings)?;
    }
    if let Some(path) = backend.flush_battery_sram()? {
        println!("[headless] md-save-ram={path}");
    }
    anyhow::ensure!(
        unavailable_captures == 0,
        "{unavailable_captures} requested Mega Drive captures had unknown CRAM colors"
    );
    Ok(())
}

fn capture_requested(opts: &HeadlessOptions, frame: u64) -> bool {
    (opts.screenshot_path.is_some() && opts.screenshot_frame == Some(frame))
        || (opts.screenshot_dir.is_some()
            && opts.screenshot_every != 0
            && frame.is_multiple_of(opts.screenshot_every))
}

fn validate_options(opts: &HeadlessOptions) -> anyhow::Result<()> {
    ensure_system_headless_options("md", opts)?;
    anyhow::ensure!(
        opts.tas_script.is_none(),
        "Mega Drive TAS execution is not supported yet"
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
            && opts.replay_peer_path.is_none()
            && !opts.replay_peer_live_link
            && opts.replay_tail_frames == 0
            && !opts.allow_gb_link_replay_divergence,
        "Mega Drive link replay is not supported"
    );
    anyhow::ensure!(
        opts.replay_path.is_some() || opts.expect_replay_final_hash.is_none(),
        "--expect-replay-final-hash requires --replay for Mega Drive"
    );
    if opts.replay_path.is_some() {
        super::replay::validate_md_replay_options(opts)?;
    }
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
    fn md_headless_debug_report_matches_backend_capabilities() -> anyhow::Result<()> {
        let directory = crate::test_support::test_directory("md-headless-capabilities")?;
        let path = directory.path().join("loop.md");
        let report = directory.path().join("debug.json");
        let mut bytes = vec![0; 0x400];
        bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
        bytes[0x200..0x204].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
        let backend = load_backend_from_rom_source(
            ActiveSystem::Md,
            &path,
            &path,
            Some(bytes.clone()),
            BackendLoadConfig {
                md_load_save_ram: false,
                md_host_persistence: false,
                md_controller: zeff_md_core::ControllerKind::SixButton,
                ..Default::default()
            },
        )?
        .backend;
        let capabilities = backend.capabilities();
        run_md_headless(
            &path,
            &path,
            Some(bytes.clone()),
            &HeadlessOptions {
                max_frames: 1,
                no_apu: true,
                no_sram: true,
                debug_state_path: Some(report.clone()),
                ..Default::default()
            },
        )?;
        let debug: serde_json::Value = serde_json::from_slice(&std::fs::read(report)?)?;
        assert_eq!(debug["system"], "md");
        assert_eq!(debug["frames"], 1);
        assert!(debug["ticks"].as_u64().is_some_and(|ticks| ticks > 0));
        assert_eq!(
            debug["capabilities"],
            serde_json::json!({
                "save_states": capabilities.supports_save_states,
                "rewind": capabilities.supports_rewind,
                "replay": capabilities.supports_replay,
                "debugger": capabilities.supports_debugger
            })
        );
        assert_eq!(
            debug["capabilities"],
            serde_json::json!({
                "save_states": true,
                "rewind": true,
                "replay": true,
                "debugger": true
            })
        );
        precise_headless_restore(directory.path(), &path, bytes.clone())?;
        warm_reset_headless_ordering(directory.path(), &path, bytes)?;
        unknown_cram_capture(directory.path(), &path)?;
        Ok(())
    }

    fn precise_headless_restore(
        directory: &Path,
        path: &Path,
        bytes: Vec<u8>,
    ) -> anyhow::Result<()> {
        let report = directory.join("precise.json");
        let state = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!(
                "md-precise-{}-{}.mdstate",
                std::process::id(),
                directory.file_name().unwrap().to_string_lossy()
            ));
        for restore in [false, true] {
            run_md_headless(
                path,
                path,
                Some(bytes.clone()),
                &HeadlessOptions {
                    max_frames: 1,
                    no_apu: true,
                    no_sram: true,
                    md_timing: (!restore).then_some(zeff_md_core::TimingModel::Precise),
                    load_state_path: restore.then(|| state.clone()),
                    md_save_state_path: (!restore).then(|| state.clone()),
                    debug_state_path: Some(report.clone()),
                    ..Default::default()
                },
            )?;
            let debug: serde_json::Value = serde_json::from_slice(&std::fs::read(&report)?)?;
            assert_eq!(debug["timing"], "Precise");
            assert!(debug["ticks"].as_u64().is_some_and(|ticks| ticks > 0));
        }
        std::fs::remove_file(state)?;
        Ok(())
    }

    fn unknown_cram_capture(directory: &Path, path: &Path) -> anyhow::Result<()> {
        let mut bytes = vec![0; 0x400];
        bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
        let mut words = Vec::<u16>::new();
        for value in [0x8114, 0x8f02, 0x9301, 0x9780, 0xc07e, 0x0080] {
            words.extend([0x33fc, value, 0x00c0, 4]);
        }
        words.extend([0x33fc, 0xabcd, 0x00c0, 0, 0x60fe, 0x4e71]);
        for (index, word) in words.into_iter().enumerate() {
            bytes[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
        }
        let screenshot = directory.join("unknown.png");
        let report = directory.join("unknown.json");
        let error = run_md_headless(
            path,
            path,
            Some(bytes),
            &HeadlessOptions {
                max_frames: 2,
                no_sram: true,
                screenshot_path: Some(screenshot.clone()),
                screenshot_frame: Some(1),
                debug_state_path: Some(report.clone()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("unknown CRAM"));
        assert!(!screenshot.exists());
        let report: serde_json::Value = serde_json::from_slice(&std::fs::read(report)?)?;
        assert_eq!(report["frames"], 2);
        assert_eq!(report["video"]["available"], false);
        assert_eq!(report["video"]["unavailable_captures"], 1);
        Ok(())
    }

    fn warm_reset_headless_ordering(
        directory: &Path,
        path: &Path,
        mut bytes: Vec<u8>,
    ) -> anyhow::Result<()> {
        bytes[0x200..0x20c].copy_from_slice(&[
            0x52, 0x79, 0, 0xff, 3, 0, 0x52, 0x81, 0x60, 0xfc, 0x4e, 0x71,
        ]);
        let EmuBackend::Md(mut control) = load_backend_from_rom_source(
            ActiveSystem::Md,
            path,
            path,
            Some(bytes.clone()),
            BackendLoadConfig {
                md_load_save_ram: false,
                md_host_persistence: false,
                ..Default::default()
            },
        )?
        .backend
        else {
            unreachable!()
        };
        control.step_frame();
        assert!(control.take_runtime_fault().is_none());
        let state = control.encode_state_bytes()?;
        let seed = directory.join("reset-seed.mdstate");
        let output = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!(
                "md-warm-reset-{}-{}.mdstate",
                std::process::id(),
                directory.file_name().unwrap().to_string_lossy()
            ));
        std::fs::write(&seed, &state)?;
        control.load_state_from_bytes(state)?;
        control.set_apu_sample_generation_enabled(false);
        for frame in 1..=2 {
            if frame == 1 {
                control
                    .try_warm_reset()
                    .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            }
            control.set_input(0, 0);
            control.set_input_p2(0, 0);
            control.step_frame();
            assert!(control.take_runtime_fault().is_none());
            control.drain_audio_samples_into(&mut Vec::new());
        }
        assert_eq!(&control.core().work_ram()[0x300..0x302], &[0, 2]);
        run_md_headless(
            path,
            path,
            Some(bytes),
            &HeadlessOptions {
                max_frames: 2,
                no_apu: true,
                no_sram: true,
                load_state_path: Some(seed),
                md_save_state_path: Some(output.clone()),
                input_events: crate::cli::parse::parse_input_event_arg(
                    "reset@1-2,reset@1",
                    "--press",
                )?,
                input_events_p2: crate::cli::parse::parse_input_event_arg("reset@1", "--press-p2")?,
                ..Default::default()
            },
        )?;
        assert_eq!(std::fs::read(output)?, control.encode_state_bytes()?);
        Ok(())
    }
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
