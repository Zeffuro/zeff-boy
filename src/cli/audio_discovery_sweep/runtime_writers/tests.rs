use std::sync::atomic::AtomicBool;

use serde_json::{Value, json};
use zeff_emu_common::audio_trace::{
    AudioTraceEvent, AudioTraceSource, AudioTraceStart, AudioTraceTiming, NesAudioTrace,
    NesTraceChip, NesTraceOrigin, NesTraceRegion, NesTraceReset, NesTraceWrite,
};

use super::*;

fn rom_source() -> Vec<u8> {
    let mut source = vec![0; 16 + 0x4000];
    source[..6].copy_from_slice(&[b'N', b'E', b'S', 0x1a, 1, 0]);
    source
}

fn offset(pc: u16) -> usize {
    16 + (usize::from(pc) - 0x8000) % 0x4000
}

fn write_instruction(source: &mut [u8], pc: u16, bytes: [u8; 3]) {
    for (delta, byte) in bytes.into_iter().enumerate() {
        source[offset(pc.checked_add(delta as u16).unwrap())] = byte;
    }
}

fn fixture(source: &[u8]) -> (Value, Value) {
    let report = serde_json::to_value(zeff_audio_discovery::drivers::scan(
        zeff_emu_common::system::System::Nes,
        source,
        zeff_audio_discovery::ScanLimits::default(),
        &AtomicBool::new(false),
    ))
    .unwrap();
    let identity = json!({
        "kind": "direct_cartridge_file",
        "sha256": zeff_firmware::sha256_hex(source),
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
    let context = json!({
        "system": "nes",
        "source": {
            "requested_file": {"sha256": evidence["source_identity"]["sha256"], "byte_len": source.len()},
            "loaded_media": {"sha256": evidence["source_identity"]["sha256"], "byte_len": source.len()},
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
    let mut row = json!({
        "plan": "baseline", "status": "success", "requested_steps": 720,
        "applied_input": {"player_1": [], "press": null},
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
    });
    row["candidate_evidence"] = super::super::evidence::reference(&evidence, true);
    assert!(super::super::evidence::binding_matches(&evidence, &row));
    (evidence, row)
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
        end_cycle: 1_000,
        events,
        dropped_events: 0,
        invalidated: None,
    }
}

fn event(pc: u32, source_offset: u64, address: u16, cycle: u64) -> AudioTraceEvent<NesTraceWrite> {
    AudioTraceEvent {
        cycle,
        pc,
        instruction_source: AudioTraceSource::CartridgeRom {
            offset: source_offset,
            bit_reversed: false,
        },
        write: NesTraceWrite::Register {
            address,
            value: 0,
            odd_cycle: cycle.is_multiple_of(2),
        },
    }
}

fn excluded_total(result: &Value) -> u64 {
    result["excluded_events"]
        .as_object()
        .unwrap()
        .values()
        .map(Value::as_u64)
        .sum::<Option<u64>>()
        .unwrap()
}

#[test]
fn records_absolute_and_indexed_sites_without_a_static_inventory() {
    let mut source = rom_source();
    write_instruction(&mut source, 0x8100, [0x8d, 0x00, 0x40]);
    write_instruction(&mut source, 0x8103, [0x9d, 0x00, 0x40]);
    write_instruction(&mut source, 0x8106, [0x99, 0x17, 0x40]);
    let (evidence, row) = fixture(&source);
    assert!(
        evidence["report"]["driver_candidates"]
            .as_array()
            .is_none_or(Vec::is_empty)
    );
    let result = summarize(
        &evidence,
        &row,
        &source,
        &trace(vec![
            event(0x8100, offset(0x8100) as u64, 0x4000, 4),
            event(0x8100, offset(0x8100) as u64, 0x4000, 9),
            event(0x8103, offset(0x8103) as u64, 0x4003, 12),
            event(0x8106, offset(0x8106) as u64, 0x4017, 15),
        ]),
        &AtomicBool::new(false),
    );
    assert_eq!(result["status"], "complete");
    assert_eq!(result["qualification"], "observed_instruction_sites_only");
    assert_eq!(result["sites"].as_array().unwrap().len(), 3);
    assert_eq!(result["sites"][0]["instruction"]["bytes"], "8d0040");
    assert_eq!(result["sites"][0]["addressing"]["mode"], "absolute");
    assert_eq!(result["sites"][1]["addressing"]["mode"], "absolute_x");
    assert_eq!(result["sites"][2]["addressing"]["mode"], "absolute_y");
    assert_eq!(result["sites"][0]["observed"]["count"], 2);
    assert_eq!(
        result["sites"][0]["registers"][0]["observed"]["last_cycle"],
        9
    );
    assert_eq!(result["observed_event_count"], 4);
    assert_eq!(
        result["observed_event_count"].as_u64().unwrap() + excluded_total(&result),
        4
    );
}

#[test]
fn nrom_mirroring_includes_crossing_instruction_provenance_and_rejects_pc_wrap() {
    let mut source = rom_source();
    source[16 + 0x3fff] = 0x8d;
    source[16] = 0;
    source[17] = 0x40;
    let (evidence, row) = fixture(&source);
    let result = summarize(
        &evidence,
        &row,
        &source,
        &trace(vec![
            event(0xbfff, (16 + 0x3fff) as u64, 0x4000, 4),
            event(0xffff, (16 + 0x3fff) as u64, 0x4000, 7),
        ]),
        &AtomicBool::new(false),
    );
    assert_eq!(result["sites"].as_array().unwrap().len(), 1);
    assert_eq!(
        result["sites"][0]["instruction"]["source_offsets"],
        json!([16 + 0x3fff, 16, 17])
    );
    assert_eq!(result["excluded_events"]["invalid_instruction_source"], 1);
}

#[test]
fn rejects_unknown_non_instruction_invalid_and_unsupported_sources_with_accounting() {
    let mut source = rom_source();
    write_instruction(&mut source, 0x8100, [0x8d, 0x00, 0x40]);
    write_instruction(&mut source, 0x8103, [0x8f, 0x00, 0x40]);
    write_instruction(&mut source, 0x8106, [0x8d, 0x01, 0x40]);
    write_instruction(&mut source, 0x8109, [0x9d, 0x01, 0x3f]);
    write_instruction(&mut source, 0x810c, [0x9d, 0x00, 0x3f]);
    let (evidence, row) = fixture(&source);
    let good = event(0x8100, offset(0x8100) as u64, 0x4000, 8);
    let unknown = AudioTraceEvent {
        instruction_source: AudioTraceSource::Unknown,
        ..good
    };
    let reversed = AudioTraceEvent {
        instruction_source: AudioTraceSource::CartridgeRom {
            offset: offset(0x8100) as u64,
            bit_reversed: true,
        },
        ..good
    };
    let status = AudioTraceEvent {
        write: NesTraceWrite::StatusRead {
            value: 0,
            origin: NesTraceOrigin::Cpu,
        },
        ..good
    };
    let dmc = AudioTraceEvent {
        write: NesTraceWrite::DmcFetch {
            address: 0,
            value: 0,
            source: AudioTraceSource::Unknown,
        },
        ..good
    };
    let zero_pc = AudioTraceEvent { pc: 0, ..good };
    let wide_pc = AudioTraceEvent {
        pc: 0x1_0000,
        ..good
    };
    let bad_register = AudioTraceEvent {
        write: NesTraceWrite::Register {
            address: 0x4014,
            value: 0,
            odd_cycle: false,
        },
        ..good
    };
    let bad_opcode = event(0x8103, offset(0x8103) as u64, 0x4000, 9);
    let wrong_destination = event(0x8106, offset(0x8106) as u64, 0x4000, 10);
    let index_255 = event(0x8109, offset(0x8109) as u64, 0x4000, 11);
    let index_256 = event(0x810c, offset(0x810c) as u64, 0x4000, 12);
    let wrong_offset = event(0x8100, 17, 0x4000, 13);
    let result = summarize(
        &evidence,
        &row,
        &source,
        &trace(vec![
            good,
            unknown,
            reversed,
            status,
            dmc,
            zero_pc,
            wide_pc,
            bad_register,
            bad_opcode,
            wrong_destination,
            index_255,
            index_256,
            wrong_offset,
        ]),
        &AtomicBool::new(false),
    );
    assert_eq!(result["sites"].as_array().unwrap().len(), 2);
    assert_eq!(result["excluded_events"]["non_cartridge_source"], 1);
    assert_eq!(result["excluded_events"]["bit_reversed_source"], 1);
    assert_eq!(result["excluded_events"]["non_register"], 2);
    assert_eq!(result["excluded_events"]["invalid_pc"], 2);
    assert_eq!(result["excluded_events"]["non_apu_register"], 1);
    assert_eq!(
        result["excluded_events"]["unsupported_or_unreachable_instruction"],
        3
    );
    assert_eq!(result["excluded_events"]["invalid_instruction_source"], 1);
    assert_eq!(
        result["observed_event_count"].as_u64().unwrap() + excluded_total(&result),
        13
    );
}

#[test]
fn source_trace_cancellation_and_capacity_fail_closed() {
    let mut source = rom_source();
    write_instruction(&mut source, 0x8100, [0x8d, 0x00, 0x40]);
    let (evidence, row) = fixture(&source);
    let capture = trace(vec![event(0x8100, offset(0x8100) as u64, 0x4000, 1)]);
    let mut changed = source.clone();
    changed[20] ^= 1;
    assert_eq!(
        summarize(&evidence, &row, &changed, &capture, &AtomicBool::new(false))["reason"],
        "source_identity_mismatch"
    );
    assert_eq!(
        summarize(&evidence, &row, &source, &capture, &AtomicBool::new(true))["sites"],
        json!([])
    );
    let mut partial = capture.clone();
    partial.dropped_events = 1;
    assert_eq!(
        summarize(&evidence, &row, &source, &partial, &AtomicBool::new(false))["reason"],
        "incomplete_trace"
    );
    let over_events = trace(vec![capture.events[0]; MAX_EVENTS + 1]);
    assert_eq!(
        summarize(
            &evidence,
            &row,
            &source,
            &over_events,
            &AtomicBool::new(false)
        )["sites"],
        json!([])
    );

    let mut many = rom_source();
    let mut events = Vec::new();
    for index in 0..=MAX_SITES {
        let pc = 0x8000 + (index * 3) as u16;
        write_instruction(&mut many, pc, [0x8d, 0x00, 0x40]);
        events.push(event(pc.into(), offset(pc) as u64, 0x4000, index as u64));
    }
    let (many_evidence, many_row) = fixture(&many);
    let at_limit = summarize(
        &many_evidence,
        &many_row,
        &many,
        &trace(events[..MAX_SITES].to_vec()),
        &AtomicBool::new(false),
    );
    assert_eq!(at_limit["status"], "complete");
    assert_eq!(at_limit["sites"].as_array().unwrap().len(), MAX_SITES);
    let result = summarize(
        &many_evidence,
        &many_row,
        &many,
        &trace(events),
        &AtomicBool::new(false),
    );
    assert_eq!(result["reason"], "site_limit");
    assert_eq!(result["sites"], json!([]));
}
