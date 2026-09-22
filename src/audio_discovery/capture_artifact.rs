use std::collections::BTreeSet;
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::Value;
use zeff_emu_common::audio_trace::{
    AudioTrace, GameBoyAudioTrace, Huc6280AudioTrace, NesAudioTrace, WonderSwanAudioTrace,
};

use super::{
    pcm::{
        PcmSession, gb_trace::GbTraceSession, huc6280_trace::Huc6280TraceSession,
        nes_trace::NesTraceSession, sn_trace::SnTraceSession, ws_trace::WonderSwanTraceSession,
    },
    render::RenderOptions,
};

mod sn_gaps;
mod writers;

const MAX_ARCHIVE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_TRACE_BYTES: u64 = 96 * 1024 * 1024;

#[derive(Deserialize)]
struct Artifact {
    path: String,
    byte_len: u64,
    sha256: String,
}

#[derive(Deserialize)]
struct Manifest {
    schema: String,
    artifacts: Vec<Artifact>,
    context: Value,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum NativeTrace {
    Sn(AudioTrace),
    Nes(NesAudioTrace),
    Huc6280(Box<Huc6280AudioTrace>),
    GameBoy(GameBoyAudioTrace),
    WonderSwan(WonderSwanAudioTrace),
}

pub(crate) struct CaptureArtifact {
    pub(crate) archive_sha256: String,
    pub(crate) manifest: Value,
    pub(crate) trace_sha256: String,
    trace: Vec<u8>,
}

impl CaptureArtifact {
    pub(crate) fn validated_nes_trace(&self, cancel: &AtomicBool) -> Result<NesAudioTrace> {
        ensure!(
            self.manifest["context"]["system"] == "nes",
            "NES capture required"
        );
        let trace: NesAudioTrace = serde_json::from_slice(&self.trace)?;
        let context = &self.manifest["context"];
        super::trace_capture::validate_provenance(&trace, context)?;
        super::trace_capture::validate_nes_sample_provenance(&trace, context)?;
        drop(NesTraceSession::new(
            trace.clone(),
            RenderOptions {
                max_seconds: 120,
                ..Default::default()
            },
            cancel,
        )?);
        Ok(trace)
    }

    pub(crate) fn excerpt_intervals(
        &self,
        options: RenderOptions,
        evidence: &super::validation::PlaybackEvidence,
        cancel: &AtomicBool,
    ) -> Result<Option<(Vec<super::validation::Interval>, Value)>> {
        let trace: NativeTrace = serde_json::from_slice(&self.trace)?;
        let NativeTrace::Sn(trace) = trace else {
            return Ok(None);
        };
        super::trace_capture::validate_provenance(&trace, &self.manifest["context"])?;
        let duration = SnTraceSession::new(trace.clone(), options, cancel)?.duration_frames();
        ensure!(
            options.fade_seconds == 0
                && evidence.deterministic()
                && !evidence.validation_duration_capped
                && !evidence.pcm.activity_intervals_truncated
                && evidence.pcm.sample_rate == options.sample_rate
                && evidence.pcm.frames == duration,
            "PSG excerpt selection requires complete deterministic unfaded PCM"
        );
        sn_gaps::select(
            &trace,
            evidence.pcm.frames,
            options.sample_rate,
            &evidence.pcm.activity_intervals,
            evidence.activity_gap_frames,
            cancel,
        )
        .map(Some)
    }

    pub(crate) fn writer_evidence(
        &self,
        options: RenderOptions,
        cancel: &AtomicBool,
    ) -> Result<Value> {
        let trace: NativeTrace = serde_json::from_slice(&self.trace)?;
        if let NativeTrace::Huc6280(trace) = &trace {
            super::pcm::validate_options(options)?;
            let context = &self.manifest["context"];
            validate_hucard_context(context)?;
            super::trace_capture::validate_provenance(trace.as_ref(), context)?;
            super::pcm::huc6280_trace::validate(trace, cancel)?;
        } else {
            drop(self.session(options, cancel)?);
        }
        let mut evidence = writers::summarize(&trace, cancel)?;
        evidence["archive_sha256"] = self.archive_sha256.clone().into();
        evidence["trace_sha256"] = self.trace_sha256.clone().into();
        Ok(evidence)
    }

    pub(crate) fn load(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path).context("could not open audio capture")?;
        ensure!(
            file.metadata()?.len() <= MAX_ARCHIVE_BYTES,
            "audio capture ZIP exceeds 128 MiB"
        );
        let mut bytes = Vec::new();
        file.take(MAX_ARCHIVE_BYTES + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_ARCHIVE_BYTES,
            "audio capture ZIP exceeds 128 MiB"
        );
        Self::from_bytes(&bytes)
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() as u64 <= MAX_ARCHIVE_BYTES,
            "audio capture ZIP exceeds 128 MiB"
        );
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
        ensure!(
            (2..=4).contains(&zip.len()),
            "audio capture has an unexpected artifact count"
        );
        let mut names = BTreeSet::new();
        for index in 0..zip.len() {
            let entry = zip.by_index(index)?;
            ensure!(
                matches!(entry.name(), "manifest.json" | "trace.json" | "capture.vgm")
                    && names.insert(entry.name().to_owned()),
                "unexpected or duplicate capture entry"
            );
        }
        let metadata = read_entry(&mut zip, "manifest.json", 256 * 1024)?;
        let manifest: Value = serde_json::from_slice(&metadata)?;
        let typed: Manifest = serde_json::from_slice(&metadata)?;
        ensure!(
            typed.schema == "zeff-audio-trace-capture/1",
            "unsupported audio capture schema"
        );
        let source = &typed.context["source"]["loaded_media"];
        ensure!(
            source["byte_len"].as_u64().is_some_and(|len| len > 0)
                && source["sha256"].as_str().is_some_and(valid_hash),
            "audio capture has no valid loaded-media identity"
        );
        let mut declared = BTreeSet::new();
        let mut trace = None;
        for artifact in typed.artifacts {
            ensure!(
                matches!(artifact.path.as_str(), "trace.json" | "capture.vgm")
                    && declared.insert(artifact.path.clone())
                    && valid_hash(&artifact.sha256),
                "invalid or duplicate declared capture artifact"
            );
            let data = read_entry(&mut zip, &artifact.path, MAX_TRACE_BYTES)?;
            ensure!(
                data.len() as u64 == artifact.byte_len
                    && zeff_firmware::sha256_hex(&data) == artifact.sha256,
                "capture artifact size or SHA-256 mismatch: {}",
                artifact.path
            );
            if artifact.path == "trace.json" {
                trace = Some(data);
            }
        }
        ensure!(
            declared.len() + 1 == names.len(),
            "capture contains undeclared artifacts"
        );
        let trace = trace.context("audio capture has no native trace")?;
        Ok(Self {
            archive_sha256: zeff_firmware::sha256_hex(bytes),
            trace_sha256: zeff_firmware::sha256_hex(&trace),
            manifest,
            trace,
        })
    }

    pub(crate) fn session(
        &self,
        options: RenderOptions,
        cancel: &AtomicBool,
    ) -> Result<Box<dyn PcmSession>> {
        let trace: NativeTrace = serde_json::from_slice(&self.trace)
            .context("unsupported native audio trace descriptor")?;
        let context = &self.manifest["context"];
        match trace {
            NativeTrace::Sn(trace) => {
                super::trace_capture::validate_provenance(&trace, context)?;
                Ok(Box::new(SnTraceSession::new(trace, options, cancel)?))
            }
            NativeTrace::Nes(trace) => {
                super::trace_capture::validate_provenance(&trace, context)?;
                super::trace_capture::validate_nes_sample_provenance(&trace, context)?;
                Ok(Box::new(NesTraceSession::new(trace, options, cancel)?))
            }
            NativeTrace::Huc6280(trace) => {
                validate_hucard_context(context)?;
                super::trace_capture::validate_provenance(trace.as_ref(), context)?;
                Ok(Box::new(Huc6280TraceSession::new(*trace, options, cancel)?))
            }
            NativeTrace::GameBoy(trace) => {
                ensure!(
                    context["system"] == "gb",
                    "Game Boy replay requires a GB/GBC capture context"
                );
                super::trace_capture::validate_provenance(&trace, context)?;
                Ok(Box::new(GbTraceSession::new(trace, options, cancel)?))
            }
            NativeTrace::WonderSwan(trace) => {
                validate_wonderswan_context(context, trace.chip.color)?;
                super::trace_capture::validate_provenance(&trace, context)?;
                Ok(Box::new(WonderSwanTraceSession::new(
                    trace, options, cancel,
                )?))
            }
        }
    }
}

fn validate_wonderswan_context(context: &Value, color: bool) -> Result<()> {
    let model = match context["settings"]["minimum_system"].as_str() {
        Some("WonderSwan") => Some(false),
        Some("WonderSwanColor") => Some(true),
        Some(value) => value
            .strip_prefix("Unknown(")
            .and_then(|value| value.strip_suffix(')'))
            .and_then(|value| value.parse::<u8>().ok())
            .filter(|&value| value >= 2)
            .map(|_| true),
        None => None,
    };
    ensure!(
        context["system"] == "ws"
            && context["firmware"].is_null()
            && context["settings"]["system_start"] == "cartridge_reset_without_bios"
            && model == Some(color),
        "WonderSwan replay requires a matching cartridge-reset capture context"
    );
    Ok(())
}

fn validate_hucard_context(context: &Value) -> Result<()> {
    ensure!(
        context["system"] == "pce"
            && context["firmware"].is_null()
            && context["settings"]["arcade_card"] == "Disabled"
            && context["source"]["normalization"]["trace_rom_offsets"]
                == "loaded_media_before_header_removal",
        "native HuC6280 playback requires a HuCard capture context without CD or Arcade Card"
    );
    Ok(())
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn read_entry(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str, limit: u64) -> Result<Vec<u8>> {
    let entry = zip
        .by_name(name)
        .with_context(|| format!("capture is missing {name}"))?;
    ensure!(
        entry.size() <= limit,
        "capture entry exceeds its size limit: {name}"
    );
    let mut bytes = Vec::new();
    entry.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "capture entry exceeds its size limit: {name}"
    );
    Ok(bytes)
}

#[cfg(test)]
pub(crate) mod tests;
