use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

pub(super) struct ExpectedSettings<'a> {
    pub sample_rate: u32,
    pub selectors: &'a Value,
    pub requested_steps: u64,
}

pub(super) fn write_json(path: &Path, value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    crate::platform::write_new_file_atomically_validated(path, &bytes, |file| {
        use std::io::Seek;
        file.rewind()?;
        let _: Value = serde_json::from_reader(file)?;
        Ok(())
    })
}

pub(super) fn logs(stdout: &[u8], stderr: &[u8], stdout_bytes: u64, stderr_bytes: u64) -> Value {
    json!({
        "stdout_bytes": stdout_bytes,
        "stderr_bytes": stderr_bytes,
        "stdout_tail": String::from_utf8_lossy(stdout),
        "stderr_tail": String::from_utf8_lossy(stderr),
    })
}

pub(super) fn artifact_context(
    path: &Path,
    source: &Value,
    requested_sha256: &str,
    system: &str,
    input_events: &Value,
    settings: ExpectedSettings<'_>,
) -> Result<Value> {
    let artifact = crate::audio_discovery::capture_artifact::CaptureArtifact::load(path)?;
    let context = &artifact.manifest["context"];
    validate_context_value(
        context,
        source,
        requested_sha256,
        system,
        input_events,
        settings,
    )?;
    Ok(json!({
        "archive_sha256": artifact.archive_sha256,
        "trace_sha256": artifact.trace_sha256,
        "context": context,
    }))
}

pub(super) fn validate_context_value(
    context: &Value,
    source: &Value,
    requested_sha256: &str,
    system: &str,
    input_events: &Value,
    settings: ExpectedSettings<'_>,
) -> Result<()> {
    let ExpectedSettings {
        sample_rate: expected_sample_rate,
        selectors,
        requested_steps,
    } = settings;
    ensure!(
        context["system"] == system
            && context["source"]["requested_file"]["sha256"] == requested_sha256
            && context["source"]["loaded_media"]["sha256"] == source["sha256"]
            && context["source"]["loaded_media"]["byte_len"] == source["len"]
            && context["input"]["player_1"] == *input_events
            && context["input"]["player_2"] == json!([])
            && context["persistent_save_files"] == "not_loaded_or_written"
            && context["sample_generation"] == true
            && context["requested_frames"] == requested_steps,
        "capture context does not match the requested source and input contract"
    );
    if source["container"].is_null() {
        ensure!(
            context["source"]["selected_member"].is_null()
                && source["sha256"] == requested_sha256
                && context["source"]["requested_file"]["byte_len"] == source["len"],
            "direct capture context does not retain the loaded source identity"
        );
    } else {
        ensure!(
            context["source"]["requested_file"]["sha256"] == source["container"]["sha256"]
                && context["source"]["requested_file"]["byte_len"] == source["container"]["len"]
                && context["source"]["selected_member"]["name"]
                    == source["selected_member"]["name"]
                && context["source"]["selected_member"]["sha256"]
                    == source["selected_member"]["sha256"]
                && context["source"]["selected_member"]["byte_len"]
                    == source["selected_member"]["len"],
            "ZIP capture context does not retain the requested archive and selected member identities"
        );
    }
    let frames = context["frames_run"]
        .as_u64()
        .context("capture context has no frame count")?;
    ensure!(
        frames == requested_steps,
        "capture context did not run exactly {requested_steps} steps"
    );
    for key in ["sample_rate_hz", "sample_rate"] {
        if let Some(rate) = context["settings"].get(key) {
            ensure!(
                metadata_integer(rate) == Some(u64::from(expected_sample_rate)),
                "capture context sample rate does not match the replay contract"
            );
        }
    }
    for player in ["player_3", "player_4", "player_5"] {
        ensure!(
            context["input"]
                .get(player)
                .is_none_or(|events| *events == json!([])),
            "capture context unexpectedly applied another player's input"
        );
    }
    ensure!(
        context["settings"].is_object(),
        "capture context has no reset settings"
    );
    if system == "coleco" {
        ensure!(
            context["firmware"]["firmware_id"] == "coleco.vision.bios"
                && context["firmware"]["sha256"]
                    .as_str()
                    .is_some_and(|hash| hash.len() == 64
                        && hash
                            .bytes()
                            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()))
                && context["firmware"]["byte_len"] == 8192,
            "Coleco capture context has no valid retail BIOS identity"
        );
    } else {
        ensure!(
            context["firmware"].is_null(),
            "ordinary cartridge capture unexpectedly used firmware"
        );
    }
    let gb_mode = match selectors["gb_mode"].as_str().unwrap_or("auto") {
        "auto" => "Auto",
        "dmg" => "ForceDmg",
        "cgb" => "ForceCgb",
        _ => return Err(anyhow::anyhow!("invalid GB capture sweep selector")),
    };
    if system == "gb" {
        ensure!(
            context["settings"]["mode_preference"] == gb_mode,
            "capture context did not apply the requested GB mode"
        );
    }
    for (selector, setting) in [
        ("sega_video_standard", "video_standard"),
        ("sega_console_region", "console_region"),
    ] {
        let expected = selectors[selector].as_str().unwrap_or("auto");
        if expected != "auto" {
            ensure!(
                context["settings"][setting] == expected,
                "capture context did not apply the requested Sega 8-bit selector"
            );
        }
    }
    Ok(())
}

fn metadata_integer(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        let rate = value.as_f64()?;
        (rate.is_finite() && rate.fract() == 0.0 && rate >= 0.0 && rate <= u64::MAX as f64)
            .then_some(rate as u64)
    })
}

pub(super) fn read_validation(path: &Path) -> Result<Value> {
    use std::io::Read;
    const LIMIT: u64 = 4 * 1024 * 1024;
    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.len() <= LIMIT,
        "validation report exceeds its file bounds"
    );
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 == metadata.len(),
        "validation report changed while reading"
    );
    Ok(serde_json::from_slice(&bytes)?)
}

pub(super) fn validation_summary(path: &Path, capture: &Value, sample_rate: u32) -> Result<Value> {
    let report = read_validation(path)?;
    ensure!(
        report["schema"] == "zeff-audio-playback-validation/1"
            && report["mode"] == "native_capture"
            && report["outcome"]["status"] == "integrity_verified"
            && report["outcome"]["archive_sha256"] == capture["archive_sha256"]
            && report["outcome"]["trace_sha256"] == capture["trace_sha256"]
            && report["render_options"]["sample_rate"] == sample_rate
            && report["outcome"]["playback"]["status"] == "rendered"
            && report["outcome"]["playback"]["evidence"]["fresh_render_matches"] == true
            && report["outcome"]["playback"]["evidence"]["reset_render_matches"] == true
            && report["outcome"]["playback"]["evidence"]["validation_duration_capped"] == false
            && report["outcome"]["native_reference"]["status"] == "matched"
            && report["outcome"]["native_reference"]["evidence"]["projected_pcm_matches"] == true,
        "native capture validation or reference projection failed"
    );
    Ok(report["outcome"].clone())
}

pub(super) fn duplicates(rows: &[Value]) -> Vec<Value> {
    let mut groups = std::collections::BTreeMap::<(u64, u64, String), Vec<String>>::new();
    for row in rows {
        if row["status"] != "success" {
            continue;
        }
        let Some(evidence) = row.pointer("/validation/playback/evidence/pcm") else {
            continue;
        };
        let (Some(rate), Some(frames), Some(hash), Some(name)) = (
            evidence["sample_rate"].as_u64(),
            evidence["frames"].as_u64(),
            evidence["pcm_sha256"].as_str(),
            row["plan"].as_str(),
        ) else {
            continue;
        };
        groups
            .entry((rate, frames, hash.to_owned()))
            .or_default()
            .push(name.to_owned());
    }
    groups
        .into_iter()
        .filter(|(_, plans)| plans.len() > 1)
        .map(|((rate, frames, hash), plans)| {
            json!({
                "sample_rate": rate, "frames": frames, "pcm_sha256": hash, "plans": plans,
            })
        })
        .collect()
}

pub(super) fn summary(rows: &[Value]) -> Value {
    let mut statuses = std::collections::BTreeMap::<String, usize>::new();
    let mut capture_validated = 0usize;
    let mut playback_validated = 0usize;
    let mut active = 0usize;
    let mut silent = 0usize;
    for row in rows {
        if let Some(status) = row["status"].as_str() {
            *statuses.entry(status.to_owned()).or_default() += 1;
        }
        if row["status"] == "source_changed" || row["status"] == "not_attempted" {
            continue;
        }
        capture_validated += usize::from(row["capture"].is_object());
        let playback = &row["validation"]["playback"];
        playback_validated += usize::from(playback["status"] == "rendered");
        if playback["evidence"]["silent"] == true {
            silent += 1;
        } else if playback["status"] == "rendered" {
            active += 1;
        }
    }
    json!({
        "status_histogram": statuses,
        "capture_validated": capture_validated,
        "playback_validated": playback_validated,
        "active": active,
        "silent": silent,
    })
}
