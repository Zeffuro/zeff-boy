use std::path::{Path, PathBuf};

use crate::audio_discovery::vgm::capture::VgmCaptureSource;
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use zeff_emu_common::audio_trace::{
    AudioTraceChip, AudioTraceSource, ChipAudioTrace, GameBoyAudioTrace, NesAudioTrace,
};

use super::HeadlessOptions;

const MAX_CAPTURE_FRAMES: u64 = 216_000;

pub(crate) fn validate_options(options: &HeadlessOptions) -> Result<()> {
    let Some(path) = &options.audio_trace_path else {
        return Ok(());
    };
    ensure!(
        path.extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip")),
        "--audio-trace requires an output ZIP path"
    );
    ensure!(
        (1..=MAX_CAPTURE_FRAMES).contains(&options.max_frames),
        "--audio-trace requires 1..={MAX_CAPTURE_FRAMES} frames"
    );
    ensure!(
        options.load_state_path.is_none()
            && options.replay_path.is_none()
            && options.replay_peer_path.is_none()
            && options.tas_project_path.is_none()
            && options.tas_script.is_none()
            && !options.apply_mods,
        "--audio-trace captures a fresh cartridge run; state loading, replay, TAS and --apply-mods are unsupported"
    );
    super::ensure_no_reset_events("audio capture", options)?;
    ensure!(
        !path.exists(),
        "audio trace output already exists: {}",
        path.display()
    );
    Ok(())
}

pub(super) fn validate_system(system: &str, options: &HeadlessOptions) -> Result<()> {
    ensure!(
        options.audio_trace_path.is_none()
            || matches!(
                system,
                "gb" | "nes" | "sms" | "gg" | "sg" | "coleco" | "pce" | "ws"
            ),
        "--audio-trace currently supports GB/GBC, base NES, Master System, Game Gear, SG-1000, ColecoVision, PC Engine HuCards and WonderSwan cartridges"
    );
    if system == "gb" && options.audio_trace_path.is_some() {
        ensure!(
            !options.no_apu,
            "GB/GBC --audio-trace cannot be used with --no-apu because APU master-disable changes the captured hardware events"
        );
        ensure!(
            !options.expect_test_pass && options.break_at.is_none(),
            "GB/GBC --audio-trace requires the requested reset-to-end interval; --expect-test-pass and --break-at can end it early"
        );
    }
    if system == "nes" && options.audio_trace_path.is_some() {
        ensure!(
            !options.no_apu && !options.expect_test_pass && options.break_at.is_none(),
            "NES --audio-trace requires active sample generation and a full fresh interval; --no-apu, --expect-test-pass and --break-at are unsupported"
        );
    }
    Ok(())
}

pub(super) struct Capture {
    path: PathBuf,
    metadata: Value,
    rom_header_bias: u64,
}

impl Capture {
    pub(super) fn prepare(
        requested_path: &Path,
        loaded_path: &Path,
        bytes: &[u8],
        options: &HeadlessOptions,
    ) -> Result<Option<Self>> {
        let Some(path) = &options.audio_trace_path else {
            return Ok(None);
        };
        validate_options(options)?;
        let source = source_metadata(requested_path, loaded_path, bytes)?;
        Ok(Some(Self {
            path: path.clone(),
            rom_header_bias: 0,
            metadata: json!({
                "source": source,
                "requested_frames": options.max_frames,
                "input": {
                    "player_1": options.input_events,
                    "player_2": options.input_events_p2,
                    "frame_intervals": "inclusive_start_and_end; frame_1_is_the_first_executed_frame",
                },
                "persistent_save_files": "not_loaded_or_written",
                "sample_generation": !options.no_apu,
            }),
        }))
    }

    pub(super) fn configure_hucard(
        &mut self,
        bytes: &[u8],
        options: &HeadlessOptions,
    ) -> Result<()> {
        let normalized = zeff_pce_core::hardware::normalize_hucard_image(bytes.to_vec())?;
        self.rom_header_bias = (bytes.len() - normalized.len()) as u64;
        self.metadata["source"]["normalization"] = json!({
            "kind": if self.rom_header_bias == 0 { "none" } else { "validated_pceas_header_removal" },
            "header_bytes": self.rom_header_bias,
            "normalized_sha256": zeff_firmware::sha256_hex(&normalized),
            "normalized_byte_len": normalized.len(),
            "trace_rom_offsets": "loaded_media_before_header_removal",
        });
        self.metadata["input"]["player_3"] = json!(options.input_events_p3);
        self.metadata["input"]["player_4"] = json!(options.input_events_p4);
        self.metadata["input"]["player_5"] = json!(options.input_events_p5);
        self.metadata["input"]["frame_intervals"] = json!(
            "inclusive_headless_step_start_and_end; step_1_is_first; an_instruction_can_publish_multiple_video_frames_per_step"
        );
        self.metadata["frame_count_unit"] = json!("headless_steps");
        Ok(())
    }

    pub(super) fn configure_wonderswan(&mut self) {
        self.metadata["input"]["frame_intervals"] =
            json!("inclusive_headless_step_start_and_end; step_1_is_first");
        self.metadata["frame_count_unit"] = json!("headless_steps");
    }

    pub(super) fn configure_game_boy(&mut self) {
        self.metadata["input"]["frame_intervals"] =
            json!("inclusive_headless_step_start_and_end; step_1_is_the_first_executed_frame");
        self.metadata["frame_count_unit"] = json!("headless_steps");
        self.metadata["sample_generation"] = json!(true);
    }

    pub(super) fn finish<C: AudioTraceChip, W>(
        mut self,
        trace: Option<ChipAudioTrace<C, W>>,
        frames_run: u64,
        system: &str,
        settings: Value,
        firmware: Option<Value>,
    ) -> Result<()>
    where
        ChipAudioTrace<C, W>: VgmCaptureSource + serde::Serialize,
    {
        let mut trace = trace.ok_or_else(|| anyhow::anyhow!("audio trace capture is missing"))?;
        for event in &mut trace.events {
            if let AudioTraceSource::CartridgeRom { offset, .. } = &mut event.instruction_source {
                *offset = offset
                    .checked_add(self.rom_header_bias)
                    .ok_or_else(|| anyhow::anyhow!("audio trace ROM offset overflow"))?;
            }
        }
        self.metadata["system"] = json!(system);
        self.metadata["frames_run"] = json!(frames_run);
        self.metadata["settings"] = settings;
        self.metadata["firmware"] = json!(firmware);
        crate::audio_discovery::trace_capture::write_new(
            &self.path,
            &trace,
            self.metadata,
            &std::sync::atomic::AtomicBool::new(false),
        )?;
        println!(
            "[headless] audio-trace={} events={} cycles={}",
            self.path.display(),
            trace.events.len(),
            trace.end_cycle
        );
        Ok(())
    }

    pub(super) fn finish_game_boy(
        mut self,
        trace: Option<GameBoyAudioTrace>,
        frames_run: u64,
        settings: Value,
    ) -> Result<()> {
        let trace =
            trace.ok_or_else(|| anyhow::anyhow!("Game Boy audio trace capture is missing"))?;
        self.metadata["system"] = json!("gb");
        self.metadata["frames_run"] = json!(frames_run);
        self.metadata["settings"] = settings;
        self.metadata["firmware"] = Value::Null;
        crate::audio_discovery::trace_capture::write_game_boy_new(
            &self.path,
            &trace,
            self.metadata,
            &std::sync::atomic::AtomicBool::new(false),
        )?;
        println!(
            "[headless] audio-trace={} events={} cycles={}",
            self.path.display(),
            trace.events.len(),
            trace.end_cycle
        );
        Ok(())
    }

    pub(super) fn finish_nes(
        mut self,
        trace: Option<NesAudioTrace>,
        frames_run: u64,
        settings: Value,
    ) -> Result<()> {
        let trace = trace.ok_or_else(|| anyhow::anyhow!("NES audio trace capture is missing"))?;
        self.metadata["system"] = json!("nes");
        self.metadata["frames_run"] = json!(frames_run);
        self.metadata["settings"] = settings;
        self.metadata["firmware"] = Value::Null;
        crate::audio_discovery::trace_capture::write_nes_new(
            &self.path,
            &trace,
            self.metadata,
            &std::sync::atomic::AtomicBool::new(false),
        )?;
        println!(
            "[headless] audio-trace={} events={} cycles={}",
            self.path.display(),
            trace.events.len(),
            trace.end_cycle
        );
        Ok(())
    }
}

fn source_metadata(requested_path: &Path, loaded_path: &Path, bytes: &[u8]) -> Result<Value> {
    let media_hash = zeff_firmware::sha256_hex(bytes);
    let (file, member) = if requested_path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        let extension = loaded_path
            .extension()
            .and_then(|extension| extension.to_str())
            .ok_or_else(|| anyhow::anyhow!("captured ZIP member has no cartridge extension"))?;
        let extracted = crate::rom_archive::extract_authenticated_bounded_zip_member(
            requested_path,
            Some(loaded_path),
            extension,
            128 * 1024 * 1024,
            crate::audio_discovery::MAX_ROM_BYTES as u64,
        )?;
        ensure!(
            extracted.bytes == bytes,
            "loaded media changed while preparing audio capture"
        );
        let (hash, byte_len, name) = extracted.witness.archive_identity();
        (
            json!({"sha256": const_hex::encode(hash), "byte_len": byte_len}),
            json!({"name": name, "sha256": media_hash, "byte_len": bytes.len()}),
        )
    } else {
        let identity = file_identity(requested_path)?;
        ensure!(
            identity["sha256"] == media_hash,
            "source file changed while preparing audio capture"
        );
        (identity, Value::Null)
    };
    Ok(json!({
        "requested_path": requested_path,
        "requested_file": file,
        "resolved_media_name": loaded_path,
        "selected_member": member,
        "loaded_media": {"sha256": media_hash, "byte_len": bytes.len(), "address_space": "core_input"},
    }))
}

fn file_identity(path: &Path) -> Result<Value> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut byte_len = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        byte_len += count as u64;
    }
    Ok(json!({"sha256": const_hex::encode(digest.finalize()), "byte_len": byte_len}))
}

#[cfg(test)]
mod pce_tests;

#[cfg(test)]
mod ws_tests;

#[cfg(test)]
mod gb_tests;

#[cfg(test)]
mod nes_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_discovery::trace_capture::tests::{member, sega_fixture};
    use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

    #[test]
    fn capture_does_not_silently_accept_another_system_or_timeline() {
        let mut options = HeadlessOptions {
            audio_trace_path: Some("capture.zip".into()),
            ..Default::default()
        };
        for system in ["gb", "nes", "sms", "gg", "sg", "coleco", "pce", "ws"] {
            assert!(validate_system(system, &options).is_ok());
        }
        for system in ["gba", "unsupported"] {
            assert!(validate_system(system, &options).is_err());
        }
        options
            .input_events
            .push(crate::cli::types::HeadlessInputEvent {
                start_frame: 1,
                end_frame: 2,
                buttons: 0,
                dpad: 0,
                coleco_keypad: None,
                reset: true,
            });
        assert!(validate_options(&options).is_err());
    }

    #[test]
    fn headless_direct_and_zip_captures_bind_inputs_and_preserve_sources() -> Result<()> {
        let directory = tempfile::tempdir()?;
        for extension in ["sms", "gg", "sg"] {
            let bytes = sega_fixture(extension);
            let rom = directory.path().join(format!("fixture.{extension}"));
            std::fs::write(&rom, &bytes)?;
            let zipped = directory.path().join(format!("fixture-{extension}.zip"));
            let selected = format!("game/fixture.{extension}");
            crate::test_support::write_zip(&zipped, &[(selected.as_str(), &bytes)])?;
            let zip_before = std::fs::read(&zipped)?;
            let mut previous_vgm = None;
            for (index, input) in [&rom, &zipped].into_iter().enumerate() {
                let output = directory
                    .path()
                    .join(format!("capture-{extension}-{index}.zip"));
                let options = HeadlessOptions {
                    audio_trace_path: Some(output.clone()),
                    max_frames: 2,
                    no_apu: index == 1,
                    input_events: vec![crate::cli::types::HeadlessInputEvent {
                        start_frame: 1,
                        end_frame: 2,
                        buttons: 1,
                        dpad: 0,
                        coleco_keypad: None,
                        reset: false,
                    }],
                    ..Default::default()
                };
                super::super::run_headless(
                    input,
                    HardwareModePreference::Auto,
                    Vec::new(),
                    &options,
                )?;
                let archive = std::fs::read(&output)?;
                let metadata: Value = serde_json::from_slice(&member(&archive, "manifest.json"))?;
                let context = &metadata["context"];
                assert_eq!(context["system"], extension);
                assert_eq!(context["frames_run"], 2);
                assert_eq!(
                    context["input"]["player_1"],
                    serde_json::to_value(&options.input_events)?
                );
                assert_eq!(context["sample_generation"], !options.no_apu);
                assert_eq!(context["persistent_save_files"], "not_loaded_or_written");
                assert_eq!(
                    context["source"]["loaded_media"]["sha256"],
                    zeff_firmware::sha256_hex(&bytes)
                );
                let expected_source = if index == 0 { &bytes } else { &zip_before };
                assert_eq!(
                    context["source"]["requested_file"]["sha256"],
                    zeff_firmware::sha256_hex(expected_source)
                );
                if index == 1 {
                    assert_eq!(context["source"]["selected_member"]["name"], selected);
                }
                let vgm = member(&archive, "capture.vgm");
                if let Some(previous) = previous_vgm.replace(vgm.clone()) {
                    assert_eq!(previous, vgm);
                }
                assert!(
                    super::super::run_headless(
                        input,
                        HardwareModePreference::Auto,
                        Vec::new(),
                        &options
                    )
                    .is_err()
                );
                assert_eq!(std::fs::read(output)?, archive);
            }
            assert_eq!(std::fs::read(rom)?, bytes);
            assert_eq!(std::fs::read(zipped)?, zip_before);
        }
        Ok(())
    }

    #[test]
    fn source_identity_mismatch_and_capture_overflow_publish_nothing() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let rom = directory.path().join("overflow.sms");
        let mut bytes = sega_fixture("sms");
        bytes[..9].copy_from_slice(&[0xf3, 0x3e, 0x90, 0xd3, 0x7f, 0xc3, 0x03, 0x00, 0x00]);
        std::fs::write(&rom, &bytes)?;
        let output = directory.path().join("capture.zip");
        let options = HeadlessOptions {
            audio_trace_path: Some(output.clone()),
            max_frames: 100,
            no_apu: true,
            ..Default::default()
        };
        let mut changed = bytes.clone();
        changed[0] ^= 1;
        assert!(Capture::prepare(&rom, &rom, &changed, &options).is_err());
        let error =
            super::super::run_headless(&rom, HardwareModePreference::Auto, Vec::new(), &options)
                .unwrap_err();
        assert!(error.to_string().contains("lost"), "{error:#}");
        assert!(!output.exists());
        assert_eq!(std::fs::read(rom)?, bytes);
        Ok(())
    }
}
