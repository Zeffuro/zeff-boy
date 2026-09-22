use std::sync::atomic::AtomicBool;

use serde_json::{Value, json};
use zeff_emu_common::audio_trace::{
    AudioTraceEvent, AudioTraceSource, AudioTraceStart, AudioTraceTiming, NesAudioTrace,
    NesTraceChip, NesTraceRegion, NesTraceReset, NesTraceWrite,
};

use super::*;

fn fixture() -> (Value, Value, Vec<u8>) {
    let source = zeff_audio_discovery::drivers::nes_sound_writes::synthetic_binding_rom();
    let report = serde_json::to_value(zeff_audio_discovery::drivers::scan(
        zeff_emu_common::system::System::Nes,
        &source,
        zeff_audio_discovery::ScanLimits::default(),
        &AtomicBool::new(false),
    ))
    .unwrap();
    assert!(
        report
            .pointer("/driver_candidates/0/code/calls/0")
            .is_some()
    );
    assert!(
        report
            .pointer("/driver_candidates/0/code/selector_consumers/0")
            .is_some()
    );
    let identity = json!({
        "kind": "direct_cartridge_file",
        "sha256": zeff_firmware::sha256_hex(&source),
        "len": source.len(), "container": null, "selected_member": null,
    });
    let evidence = super::super::evidence::wrap(
        report.clone(),
        identity.clone(),
        json!({
            "analysis_profile": "standalone-unmodified-v1", "source": identity,
            "transforms": [], "media": report["media"],
        }),
        report["limits"].clone(),
    )
    .unwrap();
    let row = bound_row(&evidence);
    (evidence, row, source)
}

fn bound_row(evidence: &Value) -> Value {
    let source = &evidence["source_identity"];
    let context = json!({
        "system": "nes",
        "source": {
            "requested_file": {"sha256": source["sha256"], "byte_len": source["len"]},
            "loaded_media": {"sha256": source["sha256"], "byte_len": source["len"]},
            "selected_member": null,
        },
        "input": {"player_1": [], "player_2": []},
        "frames_run": 720, "requested_frames": 720,
        "settings": {"sample_rate_hz": 48000}, "firmware": null,
        "persistent_save_files": "not_loaded_or_written", "sample_generation": true,
    });
    let pcm = json!({
        "sample_rate": 48000, "frames": 12, "pcm_sha256": "c".repeat(64),
        "active_frames": 0, "activity_intervals": [], "activity_intervals_truncated": false,
    });
    json!({
        "plan": "baseline", "status": "success", "requested_steps": 720,
        "applied_input": {"player_1": [], "press": null},
        "candidate_evidence": super::super::evidence::reference(evidence, true),
        "capture": {
            "archive_sha256": "d".repeat(64), "trace_sha256": "e".repeat(64),
            "context": context,
        },
        "validation": {
            "status": "integrity_verified", "archive_sha256": "d".repeat(64),
            "trace_sha256": "e".repeat(64), "capture_manifest": {"context": context},
            "playback": {"status": "rendered", "evidence": {
                "fresh_render_matches": true, "reset_render_matches": true,
                "validation_duration_capped": false, "session_duration_frames": 12,
                "silent": true, "silence_threshold_i16": 8, "activity_gap_frames": 36000,
                "pcm": pcm,
            }},
            "native_reference": {"status": "matched", "evidence": {
                "projected_pcm_matches": true, "pcm": pcm,
            }},
        },
    })
}

fn trace(events: Vec<AudioTraceEvent<NesTraceWrite>>) -> NesAudioTrace {
    NesAudioTrace {
        generation: 1,
        cycle_hz: 19_687_500,
        cycle_hz_denominator: 11,
        chip: NesTraceChip {
            clock_hz_numerator: 19_687_500,
            clock_hz_denominator: 11,
            region: NesTraceRegion::Ntsc,
            reset: NesTraceReset::ZeffPowerOnV1,
            initial_cpu_cycle: 7,
            initial_cpu_cycle_odd: true,
            initial_apu_frame_cycle: 9,
            initial_half_rate_timer_clock: false,
        },
        timing: AudioTraceTiming::CpuBusCycleBoundary,
        start: AudioTraceStart::Reset,
        end_cycle: 100,
        events,
        dropped_events: 0,
        invalidated: None,
    }
}

fn event(pc: u32, offset: u64, register: u16, cycle: u64) -> AudioTraceEvent<NesTraceWrite> {
    AudioTraceEvent {
        cycle,
        pc,
        instruction_source: AudioTraceSource::CartridgeRom {
            offset,
            bit_reversed: false,
        },
        write: NesTraceWrite::Register {
            address: register,
            value: 0,
            odd_cycle: cycle.is_multiple_of(2),
        },
    }
}

fn writer_event(evidence: &Value, writer: usize, cycle: u64) -> AudioTraceEvent<NesTraceWrite> {
    let writer = &evidence["report"]["driver_candidates"][0]["code"]["writes"][writer];
    event(
        writer["cpu_address"].as_u64().unwrap() as u32,
        writer["span"]["offset"].as_u64().unwrap(),
        writer["register"].as_u64().unwrap() as u16,
        cycle,
    )
}

#[test]
fn exact_instruction_origin_matches_and_retains_unobserved_writers() {
    let (evidence, row, source) = fixture();
    let result = summarize(
        &evidence,
        &row,
        &source,
        &trace(vec![
            writer_event(&evidence, 0, 4),
            writer_event(&evidence, 0, 9),
        ]),
        &AtomicBool::new(false),
    );
    assert_eq!(result["status"], "complete");
    let writers = &result["candidates"][0]["writers"];
    assert_eq!(writers.as_array().unwrap().len(), 2);
    assert_eq!(
        writers[0]["observed"],
        json!({"count": 2, "first_cycle": 4, "last_cycle": 9})
    );
    assert_eq!(writers[1]["observed"]["count"], 0);
    assert_eq!(result["identity"]["capture"]["context"]["system"], "nes");
    assert!(result.pointer("/candidates/0/candidate/calls").is_none());
    assert!(
        result
            .pointer("/candidates/0/candidate/selector_consumers")
            .is_none()
    );
}

#[test]
fn silent_context_and_unmatched_runtime_writes_remain_complete() {
    let (evidence, row, source) = fixture();
    let mut unmatched = writer_event(&evidence, 0, 4);
    unmatched.pc = 0x8123;
    let result = summarize(
        &evidence,
        &row,
        &source,
        &trace(vec![unmatched]),
        &AtomicBool::new(false),
    );
    assert_eq!(result["status"], "complete");
    assert!(
        result["candidates"][0]["writers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|writer| writer["observed"]["count"] == 0)
    );
}

#[test]
fn only_exact_instruction_register_and_mapped_rom_source_correlate() {
    let (evidence, row, source) = fixture();
    let valid = writer_event(&evidence, 0, 7);
    let offset = valid.instruction_source_offset();
    let mut pc_zero = valid;
    pc_zero.pc = 0;
    let mut wrong_offset = valid;
    wrong_offset.instruction_source = AudioTraceSource::CartridgeRom {
        offset: 17,
        bit_reversed: false,
    };
    let mut reversed = valid;
    reversed.instruction_source = AudioTraceSource::CartridgeRom {
        offset,
        bit_reversed: true,
    };
    let mut wrong_register = valid;
    wrong_register.write = NesTraceWrite::Register {
        address: 0x4001,
        value: 0,
        odd_cycle: false,
    };
    let unknown = AudioTraceEvent {
        instruction_source: AudioTraceSource::Unknown,
        ..valid
    };
    let status = AudioTraceEvent {
        write: NesTraceWrite::StatusRead {
            value: 0,
            origin: zeff_emu_common::audio_trace::NesTraceOrigin::Cpu,
        },
        ..valid
    };
    let dmc = AudioTraceEvent {
        write: NesTraceWrite::DmcFetch {
            address: 0,
            value: 0,
            source: AudioTraceSource::Unknown,
        },
        ..valid
    };
    let result = summarize(
        &evidence,
        &row,
        &source,
        &trace(vec![
            pc_zero,
            wrong_offset,
            reversed,
            wrong_register,
            unknown,
            status,
            dmc,
            valid,
        ]),
        &AtomicBool::new(false),
    );
    assert_eq!(
        result["candidates"][0]["writers"][0]["observed"]["count"],
        1
    );
}

#[test]
fn changed_source_context_static_evidence_and_incomplete_reports_cannot_correlate() {
    let (evidence, row, source) = fixture();
    let capture = trace(vec![writer_event(&evidence, 0, 1)]);
    let mut changed_source = source.clone();
    changed_source[20] ^= 1;
    assert_eq!(
        summarize(
            &evidence,
            &row,
            &changed_source,
            &capture,
            &AtomicBool::new(false)
        )["reason"],
        "source_identity_mismatch"
    );

    let mut changed_context = row.clone();
    changed_context["capture"]["context"]["requested_frames"] = json!(721);
    assert_eq!(
        summarize(
            &evidence,
            &changed_context,
            &source,
            &capture,
            &AtomicBool::new(false)
        )["reason"],
        "unbound_capture_or_static_evidence"
    );

    let mut changed_span = evidence.clone();
    changed_span["report"]["driver_candidates"][0]["evidence"][3]["sha256"] = json!("0".repeat(64));
    assert_eq!(
        summarize(
            &changed_span,
            &row,
            &source,
            &capture,
            &AtomicBool::new(false)
        )["reason"],
        "unbound_capture_or_static_evidence"
    );

    let mut incomplete_report = evidence["report"].clone();
    incomplete_report["status"] = json!({"kind": "incomplete", "reason": "work_limit"});
    let incomplete = super::super::evidence::wrap(
        incomplete_report.clone(), evidence["source_identity"].clone(),
        json!({
            "analysis_profile": "standalone-unmodified-v1",
            "source": evidence["source_identity"], "transforms": [], "media": incomplete_report["media"],
        }), incomplete_report["limits"].clone(),
    ).unwrap();
    let mut incomplete_row = bound_row(&incomplete);
    incomplete_row["candidate_evidence"] = super::super::evidence::reference(&incomplete, true);
    assert_eq!(
        summarize(
            &incomplete,
            &incomplete_row,
            &source,
            &capture,
            &AtomicBool::new(false)
        )["reason"],
        "unbound_capture_or_static_evidence"
    );
}

#[test]
fn cancellation_and_explicit_limits_publish_no_partial_writers() {
    let (evidence, row, source) = fixture();
    let cancelled = AtomicBool::new(true);
    let result = summarize(
        &evidence,
        &row,
        &source,
        &trace(vec![writer_event(&evidence, 0, 1)]),
        &cancelled,
    );
    assert_eq!(result["reason"], "cancelled");
    assert_eq!(result["candidates"], json!([]));

    let mut report = evidence["report"].clone();
    let candidate = report["driver_candidates"][0].clone();
    report["driver_candidates"] = Value::Array(vec![candidate; MAX_CANDIDATES + 1]);
    let limited = super::super::evidence::wrap(
        report.clone(),
        evidence["source_identity"].clone(),
        json!({
            "analysis_profile": "standalone-unmodified-v1",
            "source": evidence["source_identity"], "transforms": [], "media": report["media"],
        }),
        report["limits"].clone(),
    )
    .unwrap();
    let limited_row = bound_row(&limited);
    let result = summarize(
        &limited,
        &limited_row,
        &source,
        &trace(vec![]),
        &AtomicBool::new(false),
    );
    assert_eq!(result["reason"], "candidate_limit");
    assert_eq!(result["candidates"], json!([]));
}

#[test]
fn absent_candidate_inventory_is_an_unavailable_empty_result() {
    let (evidence, _, source) = fixture();
    let mut report = evidence["report"].clone();
    report.as_object_mut().unwrap().remove("driver_candidates");
    let empty = super::super::evidence::wrap(
        report.clone(),
        evidence["source_identity"].clone(),
        json!({
            "analysis_profile": "standalone-unmodified-v1",
            "source": evidence["source_identity"], "transforms": [], "media": report["media"],
        }),
        report["limits"].clone(),
    )
    .unwrap();
    let result = summarize(
        &empty,
        &bound_row(&empty),
        &source,
        &trace(vec![]),
        &AtomicBool::new(false),
    );
    assert_eq!(result["reason"], "no_nrom_static_writers");
    assert_eq!(result["candidates"], json!([]));
}

#[test]
fn static_writer_requires_authenticated_legal_contiguous_store_bytes() {
    let (evidence, _, source) = fixture();
    let candidate = &evidence["report"]["driver_candidates"][0];
    let writer = &candidate["code"]["writes"][0];
    let nrom = Nrom::parse(&source).unwrap();
    assert!(StaticWriter::parse(0, 0, candidate, writer, &source, nrom).is_some());

    let mut bad_hash = candidate.clone();
    let match_index = bad_hash["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .position(|item| item["kind"] == "sound_register_write" && item["span"] == writer["span"])
        .unwrap();
    bad_hash["evidence"][match_index]["sha256"] = json!("0".repeat(64));
    assert!(StaticWriter::parse(0, 0, &bad_hash, writer, &source, nrom).is_none());

    let mut bad_operand = source.clone();
    let offset = writer["span"]["offset"].as_u64().unwrap() as usize;
    bad_operand[offset + 1] ^= 1;
    assert!(StaticWriter::parse(0, 0, candidate, writer, &bad_operand, nrom).is_none());

    let mut crossing = writer.clone();
    crossing["span"]["offset"] = json!(16 + 0x3fff);
    assert!(StaticWriter::parse(0, 0, candidate, &crossing, &source, nrom).is_none());
}

trait InstructionSourceOffset {
    fn instruction_source_offset(&self) -> u64;
}

impl InstructionSourceOffset for AudioTraceEvent<NesTraceWrite> {
    fn instruction_source_offset(&self) -> u64 {
        match self.instruction_source {
            AudioTraceSource::CartridgeRom { offset, .. } => offset,
            _ => unreachable!(),
        }
    }
}
