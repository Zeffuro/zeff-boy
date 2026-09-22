use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use serde_json::{Value, json};

use super::{control_probe, correlation, evidence, input, runtime_writers};
use crate::audio_discovery::capture_artifact::CaptureArtifact;

#[cfg(test)]
#[path = "correlation_artifact/runtime_tests.rs"]
mod runtime_tests;
#[cfg(test)]
#[path = "correlation_artifact/tests.rs"]
mod tests;

pub(super) struct ObservedWriters {
    pub correlation: Value,
    pub runtime: Value,
    pub control: Value,
}

impl ObservedWriters {
    fn unavailable(reason: &str) -> Self {
        Self {
            correlation: unavailable(reason),
            runtime: runtime_writers::unavailable(reason),
            control: control_probe::unavailable(reason),
        }
    }
}

pub(super) fn unavailable(reason: &str) -> Value {
    json!({
        "schema": "zeff-audio-writer-correlation/1",
        "qualification": "observed_writes_only",
        "status": "unavailable", "reason": reason,
    })
}

pub(super) fn observe(
    loaded: &input::Loaded,
    row: &Value,
    path: &Path,
    cancel: &AtomicBool,
) -> ObservedWriters {
    if cancel.load(Ordering::Relaxed) {
        return ObservedWriters::unavailable("cancelled");
    }
    if loaded.system != zeff_emu_common::system::System::Nes {
        return ObservedWriters::unavailable("unsupported_system");
    }
    if !evidence::binding_matches(&loaded.candidate_evidence, row) {
        return ObservedWriters::unavailable("capture_not_bound");
    }
    let result = (|| -> Result<ObservedWriters> {
        let artifact = CaptureArtifact::load(path)?;
        ensure!(
            row["capture"]["archive_sha256"] == artifact.archive_sha256
                && row["capture"]["trace_sha256"] == artifact.trace_sha256
                && row["capture"]["context"] == artifact.manifest["context"],
            "capture changed after validation"
        );
        let trace = artifact.validated_nes_trace(cancel)?;
        let correlation = correlation::summarize(
            &loaded.candidate_evidence,
            row,
            &loaded.bytes,
            &trace,
            cancel,
        );
        let runtime = runtime_writers::summarize(
            &loaded.candidate_evidence,
            row,
            &loaded.bytes,
            &trace,
            cancel,
        );
        let control = control_probe::observe(&loaded.bytes, &trace, row, &runtime, cancel);
        if cancel.load(Ordering::Relaxed) {
            return Ok(ObservedWriters::unavailable("cancelled"));
        }
        Ok(ObservedWriters {
            correlation,
            runtime,
            control,
        })
    })();
    result.unwrap_or_else(|error| {
        if cancel.load(Ordering::Relaxed) {
            return ObservedWriters::unavailable("cancelled");
        }
        let mut value = ObservedWriters::unavailable("invalid_capture");
        value.correlation["message"] = json!(format!("{error:#}"));
        value.runtime["message"] = value.correlation["message"].clone();
        value.control["message"] = value.correlation["message"].clone();
        value
    })
}
