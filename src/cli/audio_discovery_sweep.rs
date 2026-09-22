use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

#[path = "audio_discovery_sweep/census.rs"]
mod census;
#[path = "audio_discovery_sweep/comparison.rs"]
mod comparison;
#[path = "audio_discovery_sweep/control_flow.rs"]
mod control_flow;
#[path = "audio_discovery_sweep/control_probe.rs"]
mod control_probe;
#[path = "audio_discovery_sweep/correlation.rs"]
mod correlation;
#[path = "audio_discovery_sweep/correlation_artifact.rs"]
mod correlation_artifact;
#[path = "audio_discovery_sweep/evidence.rs"]
mod evidence;
#[path = "audio_discovery_sweep/input.rs"]
mod input;
#[path = "audio_discovery_sweep/nes_source.rs"]
mod nes_source;
#[path = "audio_discovery_sweep/plans.rs"]
mod plans;
#[path = "audio_discovery_sweep/process.rs"]
mod process;
#[path = "audio_discovery_sweep/report.rs"]
mod report;
#[path = "audio_discovery_sweep/runtime_writers.rs"]
mod runtime_writers;
#[cfg(test)]
#[path = "audio_discovery_sweep/tests.rs"]
mod tests;

const CAPTURE_TIMEOUT: Duration = Duration::from_secs(180);
const VALIDATION_TIMEOUT: Duration = Duration::from_secs(60);

pub(in crate::cli) fn run_if_requested(args: &[OsString]) -> Result<bool> {
    let Some(request) = input::parse(args)? else {
        return Ok(false);
    };
    run(request)?;
    Ok(true)
}

fn run(request: input::Request) -> Result<()> {
    super::audio_discovery_input::ensure_distinct_output_path(&request.output, &request.input)?;
    super::audio_discovery_input::ensure_outside_output_directory(&request.input, &request.output)?;
    if let Some(path) = &request.plan_document_path {
        super::audio_discovery_input::ensure_distinct_output_path(&request.output, path)?;
        super::audio_discovery_input::ensure_outside_output_directory(path, &request.output)?;
    }
    reserve_output(&request.output)?;
    let output =
        crate::platform::StableDirectory::open_or_create(&request.output, "capture sweep output")?;
    let cancel = AtomicBool::new(false);
    let loaded = match input::load(&request, &cancel) {
        Ok(loaded) => loaded,
        Err(error) => {
            let status = if error.is::<input::UnsupportedConfiguration>() {
                "unsupported_configuration"
            } else {
                "input_error"
            };
            let rows = request
                .plans
                .iter()
                .enumerate()
                .map(|(index, plan)| {
                    plan_row(
                        plan,
                        if index == 0 { status } else { "not_attempted" },
                        json!({"message": format!("{error:#}"), "reason": status}),
                    )
                })
                .collect::<Vec<_>>();
            output.revalidate()?;
            let mut report = json!({
                "schema": "zeff-audio-capture-sweep/1",
                "requested_input": request.input,
                "plans": &rows,
                "summary": report::summary(&rows),
                "duplicate_intervals": [],
            });
            add_plan_document(&mut report, &request);
            report::write_json(&output.path().join("report.json"), &report)?;
            return Err(error);
        }
    };
    let executable = std::env::current_exe().context("could not locate the current executable")?;
    let cwd = std::env::current_dir().context("could not locate the current directory")?;
    let runner = Runner {
        executable: &executable,
        cwd: &cwd,
        cancel: &cancel,
    };
    let mut rows = Vec::new();
    let mut source_changed = false;
    for plan in &request.plans {
        output.revalidate()?;
        let plan_dir = output.path().join(&plan.name);
        std::fs::create_dir(&plan_dir)?;
        let plan_output = crate::platform::StableDirectory::open_or_create(
            &plan_dir,
            "capture sweep plan output",
        )?;
        let mut row = if source_changed {
            plan_row(plan, "not_attempted", json!({"reason": "source_changed"}))
        } else if !input::source_is_unchanged(&request.input, &loaded.requested_sha256) {
            source_changed = true;
            plan_row(plan, "source_changed", Value::Null)
        } else {
            run_plan(&runner, &output, &plan_output, &request, &loaded, plan).unwrap_or_else(
                |error| {
                    plan_row(
                        plan,
                        "capture_failed",
                        json!({"message": format!("{error:#}")}),
                    )
                },
            )
        };
        output.revalidate()?;
        plan_output.revalidate()?;
        source_changed |= row["status"] == "source_changed";
        row["candidate_evidence"] =
            evidence::reference_for_row(&loaded.candidate_evidence, &row, source_changed);
        let observed = correlation_artifact::observe(
            &loaded,
            &row,
            &plan_output.path().join("capture.zip"),
            &cancel,
        );
        row["writer_correlation"] = observed.correlation;
        row["runtime_writers"] = observed.runtime;
        row["runtime_control"] = observed.control;
        if !input::source_is_unchanged(&request.input, &loaded.requested_sha256) {
            source_changed = true;
            if row["status"] != "not_attempted" {
                row["status"] = json!("source_changed");
            }
        }
        row["candidate_evidence"] =
            evidence::reference_for_row(&loaded.candidate_evidence, &row, source_changed);
        if source_changed {
            row["writer_correlation"] = correlation_artifact::unavailable("source_changed");
            row["runtime_writers"] = runtime_writers::unavailable("source_changed");
            row["runtime_control"] = control_probe::unavailable("source_changed");
        }
        output.revalidate()?;
        plan_output.revalidate()?;
        report::write_json(&plan_output.path().join("report.json"), &row)?;
        rows.push(row);
    }
    let failures = rows.iter().filter(|row| row["status"] != "success").count();
    let mut report = json!({
        "schema": "zeff-audio-capture-sweep/1",
        "requested_source_sha256": &loaded.requested_sha256,
        "source": &loaded.source,
        "loaded_source_identity": &loaded.source_identity,
        "static_observation": &loaded.static_observation,
        "candidate_evidence": &loaded.candidate_evidence,
        "candidate_census": census::summarize(
            &loaded.candidate_evidence,
            &loaded.static_observation,
            &rows,
        ),
        "system": loaded.system.code(),
        "selectors": &loaded.selectors,
        "plans": &rows,
        "summary": report::summary(&rows),
        "duplicate_intervals": report::duplicates(&rows),
        "limitations": [
            "Plans are fresh bounded input experiments, not song selectors.",
            "Activity and duplicate intervals are evidence only; they do not establish songs, causal input changes, ends, or loops.",
            "Silent deterministic native intervals are valid successes.",
        ],
    });
    add_plan_document(&mut report, &request);
    report["plan_comparison"] = comparison::compare(&rows);
    output.revalidate()?;
    report::write_json(&output.path().join("report.json"), &report)?;
    ensure!(
        failures == 0,
        "capture sweep recorded failures; see {}",
        output.path().display()
    );
    Ok(())
}

fn reserve_output(path: &Path) -> Result<()> {
    ensure!(
        !path.exists(),
        "capture sweep output directory already exists"
    );
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    std::fs::create_dir(path).with_context(|| {
        format!(
            "failed to reserve new capture sweep output {}",
            path.display()
        )
    })
}

struct Runner<'a> {
    executable: &'a Path,
    cwd: &'a Path,
    cancel: &'a AtomicBool,
}

fn run_plan(
    runner: &Runner<'_>,
    output: &crate::platform::StableDirectory,
    plan_directory: &crate::platform::StableDirectory,
    request: &input::Request,
    loaded: &input::Loaded,
    plan: &plans::Plan,
) -> Result<Value> {
    let capture = plan_directory.path().join("capture.zip");
    let native = plan_directory.path().join("native.f32");
    let validation = plan_directory.path().join("validation.json");
    let mut args = vec![
        OsString::from("--headless"),
        OsString::from("--no-sram"),
        OsString::from("--max-frames"),
        OsString::from(plan.steps.to_string()),
    ];
    args.extend(loaded.source_args.iter().cloned());
    args.extend([
        OsString::from("--audio-trace"),
        capture.as_os_str().to_owned(),
        OsString::from("--audio-dump"),
        native.as_os_str().to_owned(),
    ]);
    forward_options(&mut args, request, loaded.system);
    if let Some(press) = &plan.press {
        args.extend([OsString::from("--press"), OsString::from(press)]);
    }
    output.revalidate()?;
    plan_directory.revalidate()?;
    let outcome = process::run(
        runner.executable,
        &args,
        runner.cwd,
        CAPTURE_TIMEOUT,
        runner.cancel,
    )?;
    if !succeeded(&outcome) {
        return Ok(process_row(plan, capture_status(&outcome), &outcome));
    }
    output.revalidate()?;
    plan_directory.revalidate()?;
    let context = match report::artifact_context(
        &capture,
        &loaded.source_identity,
        &loaded.requested_sha256,
        loaded.system.code(),
        &plan.events,
        report::ExpectedSettings {
            sample_rate: loaded.sample_rate,
            selectors: &loaded.selectors,
            requested_steps: plan.steps,
        },
    ) {
        Ok(context) => context,
        Err(error) => {
            return Ok(plan_row(
                plan,
                "artifact_invalid",
                json!({"message": format!("{error:#}"), "capture_process": process_json(&outcome)}),
            ));
        }
    };
    if !input::source_is_unchanged(&request.input, &loaded.requested_sha256) {
        return Ok(plan_row(
            plan,
            "source_changed",
            json!({"capture": context, "capture_process": process_json(&outcome)}),
        ));
    }
    let validation_args = vec![
        OsString::from("--audio-capture-check"),
        validation.as_os_str().to_owned(),
        capture.as_os_str().to_owned(),
        OsString::from("--audio-capture-reference-f32"),
        native.as_os_str().to_owned(),
        OsString::from("--audio-sample-rate"),
        OsString::from(loaded.sample_rate.to_string()),
        OsString::from("--audio-max-seconds"),
        OsString::from("120"),
    ];
    output.revalidate()?;
    plan_directory.revalidate()?;
    let validation_outcome = match process::run(
        runner.executable,
        &validation_args,
        runner.cwd,
        VALIDATION_TIMEOUT,
        runner.cancel,
    ) {
        Ok(outcome) => outcome,
        Err(error) => {
            return Ok(plan_row(
                plan,
                "validation_failed",
                json!({
                    "message": format!("{error:#}"), "capture": context,
                    "capture_process": process_json(&outcome),
                }),
            ));
        }
    };
    output.revalidate()?;
    plan_directory.revalidate()?;
    let validation_summary = report::validation_summary(&validation, &context, loaded.sample_rate);
    if !succeeded(&validation_outcome) {
        let status = validation_status(
            validation_outcome.termination,
            validation_has_reference_mismatch(&validation),
        );
        return Ok(plan_row(
            plan,
            status,
            json!({
                "capture": context,
                "capture_process": process_json(&outcome),
                "validation_process": process_json(&validation_outcome),
            }),
        ));
    }
    let summary = match validation_summary {
        Ok(summary) => summary,
        Err(error) => {
            return Ok(plan_row(
                plan,
                if validation_has_reference_mismatch(&validation) {
                    "reference_mismatch"
                } else {
                    "validation_failed"
                },
                json!({
                    "message": format!("{error:#}"),
                    "capture": context,
                    "capture_process": process_json(&outcome),
                    "validation_process": process_json(&validation_outcome),
                }),
            ));
        }
    };
    if !input::source_is_unchanged(&request.input, &loaded.requested_sha256) {
        return Ok(plan_row(
            plan,
            "source_changed",
            json!({
                "capture": context,
                "validation": summary,
                "capture_process": process_json(&outcome),
                "validation_process": process_json(&validation_outcome),
            }),
        ));
    }
    Ok(plan_row(
        plan,
        "success",
        json!({
            "capture": context,
            "validation": summary,
            "capture_process": process_json(&outcome),
            "validation_process": process_json(&validation_outcome),
        }),
    ))
}

fn validation_has_reference_mismatch(path: &Path) -> bool {
    report::read_validation(path)
        .is_ok_and(|report| report["outcome"]["native_reference"]["status"] == "mismatch")
}

fn validation_status(termination: process::Termination, mismatch: bool) -> &'static str {
    match termination {
        process::Termination::TimedOut => "validation_timeout",
        process::Termination::OutputLimit => "output_limit",
        process::Termination::Cancelled => "cancelled",
        process::Termination::Exited if mismatch => "reference_mismatch",
        process::Termination::Exited => "validation_failed",
    }
}

fn succeeded(outcome: &process::ProcessOutcome) -> bool {
    outcome.termination == process::Termination::Exited && outcome.exit_code == Some(0)
}

fn capture_status(outcome: &process::ProcessOutcome) -> &'static str {
    match outcome.termination {
        process::Termination::TimedOut => "capture_timeout",
        process::Termination::OutputLimit => "output_limit",
        process::Termination::Exited => "capture_failed",
        process::Termination::Cancelled => "cancelled",
    }
}

fn forward_options(
    args: &mut Vec<OsString>,
    request: &input::Request,
    system: zeff_emu_common::system::System,
) {
    if system == zeff_emu_common::system::System::Gb {
        args.extend([
            OsString::from("--mode"),
            OsString::from(request.gb_mode.as_deref().unwrap_or("auto")),
        ]);
    }
    if matches!(
        system,
        zeff_emu_common::system::System::Sms
            | zeff_emu_common::system::System::Gg
            | zeff_emu_common::system::System::Sg
    ) {
        if let Some(value) = &request.sega_video_standard {
            args.extend([
                OsString::from("--sega8-video-standard"),
                OsString::from(value),
            ]);
        }
        if let Some(value) = &request.sega_console_region {
            args.extend([
                OsString::from("--sega8-console-region"),
                OsString::from(value),
            ]);
        }
    }
}

fn plan_row(plan: &plans::Plan, status: &str, details: Value) -> Value {
    let mut row = json!({
        "plan": &plan.name,
        "requested_steps": plan.steps,
        "status": status,
        "applied_input": {"press": &plan.press, "player_1": &plan.events},
    });
    if let Some(object) = details.as_object() {
        row.as_object_mut()
            .expect("plan row is an object")
            .extend(object.clone());
    }
    row
}

fn process_row(plan: &plans::Plan, status: &str, outcome: &process::ProcessOutcome) -> Value {
    plan_row(
        plan,
        status,
        json!({"capture_process": process_json(outcome)}),
    )
}

fn add_plan_document(report: &mut Value, request: &input::Request) {
    if let Some(sha256) = &request.plan_document_sha256 {
        report["plan_document"] = json!({
            "sha256": sha256,
            "input_frame_ranges": "one_based_inclusive_emulation_steps",
            "normalized_schedule": plans::schedule_json(&request.plans),
        });
    }
}

fn process_json(outcome: &process::ProcessOutcome) -> Value {
    let mut logs = report::logs(
        &outcome.stdout,
        &outcome.stderr,
        outcome.stdout_bytes,
        outcome.stderr_bytes,
    );
    logs["termination"] = json!(format!("{:?}", outcome.termination));
    logs["exit_code"] = json!(outcome.exit_code);
    logs["elapsed_ms"] = json!(outcome.elapsed_ms);
    logs
}
