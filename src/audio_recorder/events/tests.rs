use std::path::Path;

use serde_json::Value;
use zeff_emu_common::system::System;
use zeff_emu_common::time::ClockRate;

use super::*;
use crate::audio_tooling::{AudioTopology, AudioVoiceState};
use crate::settings::AudioRecordingFormat;
use crate::test_support::test_directory;

const CHANNELS: &[AudioChannelDescriptor] = &[
    AudioChannelDescriptor {
        id: AudioChannelId(0),
        name: "Pulse",
        group: "Test APU",
        class: AudioVoiceClass::Pulse,
        caps: AudioSemanticCaps::GATE_PITCH_LEVEL,
        muteable: true,
    },
    AudioChannelDescriptor {
        id: AudioChannelId(3),
        name: "Noise",
        group: "Test APU",
        class: AudioVoiceClass::Noise,
        caps: AudioSemanticCaps::GATE_LEVEL,
        muteable: false,
    },
];

fn context() -> AudioRecordingContext {
    AudioRecordingContext {
        system: System::Gb,
        topology: AudioTopology {
            generation: 4,
            channels: CHANNELS,
        },
        clock_rate: ClockRate::from_hz(4_194_304),
    }
}

fn frame(frame: u64, pitch_hz: f64) -> AudioSemanticFrame {
    AudioSemanticFrame {
        frame,
        tempo_us_per_beat: 1_000_000,
        voices: vec![
            AudioVoiceState {
                channel: AudioChannelId(0),
                name: "Pulse",
                class: AudioVoiceClass::Pulse,
                active: true,
                pitch_hz: Some(pitch_hz),
                level: Some(0.75),
            },
            AudioVoiceState {
                channel: AudioChannelId(3),
                name: "Noise",
                class: AudioVoiceClass::Noise,
                active: false,
                pitch_hz: None,
                level: Some(0.0),
            },
        ],
    }
}

fn records(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn writer_is_topology_first_diffed_epoch_aware_and_complete() {
    let root = test_directory("audio-events-complete").unwrap();
    let target = root.path().join("capture.zaudio");
    std::fs::write(&target, b"existing target").unwrap();

    let mut writer = ZeffAudioEventWriter::start(&target, context()).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"existing target");
    writer.write_frame(&frame(10, 440.0)).unwrap();
    writer.write_frame(&frame(11, 440.0)).unwrap();
    writer
        .begin_epoch(AudioTimelineDiscontinuity::StateLoad)
        .unwrap();
    writer.write_frame(&frame(2, 440.0)).unwrap();
    writer.finish().unwrap();

    let records = records(&target);
    let record_kinds = records
        .iter()
        .map(|record| record["record"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        record_kinds,
        ["header", "epoch", "frame", "epoch", "frame", "end"]
    );
    assert_eq!(records[0]["schema"], "zeff-audio-events/1");
    assert_eq!(records[0]["system"], "gb");
    assert_eq!(records[0]["timebase"]["unit"], "emulated_frame");
    assert_eq!(records[0]["timebase"]["origin"], "per_epoch");
    assert_eq!(records[0]["timebase"]["sample"], "end_of_frame");
    assert_eq!(records[0]["topology"]["generation"], 4);
    assert!(records[0].get("clock").is_none());
    assert_eq!(
        records[0]["topology"]["channels"][0]["capabilities"][0],
        "gate"
    );
    assert_eq!(records[2]["epoch"], 0);
    assert_eq!(records[2]["frame"], 0);
    assert_eq!(records[2]["voices"].as_array().unwrap().len(), 2);
    assert!(records[2]["voices"][0].get("name").is_none());
    assert!(records[2]["voices"][0].get("class").is_none());
    assert_eq!(records[3]["reason"], "state_load");
    assert_eq!(records[4]["frame"], 0);
    assert_eq!(records[5]["epochs"], 2);
    assert_eq!(records[5]["frames"], 3);
    assert_eq!(records[5]["state_events"], 4);
    assert_eq!(records[5]["last_epoch"], 1);
    assert_eq!(records[5]["last_frame"], 0);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn empty_recording_is_valid_and_has_no_last_coordinate() {
    let root = test_directory("audio-events-empty").unwrap();
    let target = root.path().join("empty.zaudio");
    ZeffAudioEventWriter::start(&target, context())
        .unwrap()
        .finish()
        .unwrap();

    let records = records(&target);
    assert_eq!(records.len(), 3);
    assert_eq!(records[0]["record"], "header");
    assert_eq!(records[1]["record"], "epoch");
    assert_eq!(records[2]["record"], "end");
    assert_eq!(records[2]["frames"], 0);
    assert!(records[2]["last_epoch"].is_null());
    assert!(records[2]["last_frame"].is_null());
}

#[test]
fn invalid_frame_leaves_existing_target_untouched_and_cleans_temp() {
    let root = test_directory("audio-events-invalid").unwrap();
    let target = root.path().join("capture.zaudio");
    std::fs::write(&target, b"keep me").unwrap();

    let mut writer = ZeffAudioEventWriter::start(&target, context()).unwrap();
    let error = writer.write_frame(&frame(0, f64::NAN)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    drop(writer);

    assert_eq!(std::fs::read(&target).unwrap(), b"keep me");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn recorder_integration_keeps_writer_errors_sticky_until_finish() {
    let root = test_directory("audio-events-recorder-error").unwrap();
    let target = root.path().join("capture.zaudio");
    std::fs::write(&target, b"keep me").unwrap();

    let mut recorder = crate::audio_recorder::AudioRecorder::start(
        &target,
        48_000,
        AudioRecordingFormat::ZeffEvents,
        Some(context()),
    )
    .unwrap();
    recorder.write_audio_semantic_frame(frame(0, f64::NAN));
    assert!(recorder.finish().is_err());

    assert_eq!(std::fs::read(&target).unwrap(), b"keep me");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn write_failure_poisons_writer_and_never_replaces_target() {
    let root = test_directory("audio-events-poisoned").unwrap();
    let target = root.path().join("capture.zaudio");
    let read_only_path = root.path().join("read-only-sink");
    std::fs::write(&target, b"keep me").unwrap();
    std::fs::write(&read_only_path, b"sink").unwrap();

    let mut writer = ZeffAudioEventWriter::start(&target, context()).unwrap();
    writer.writer = Some(BufWriter::with_capacity(
        0,
        File::open(&read_only_path).unwrap(),
    ));
    assert!(writer.write_frame(&frame(0, 440.0)).is_err());
    assert!(writer.finish().is_err());

    assert_eq!(std::fs::read(&target).unwrap(), b"keep me");
    assert!(std::fs::read_dir(root.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".tmp.")
    }));
}

#[test]
fn failed_epoch_write_does_not_advance_or_clear_epoch_state() {
    let root = test_directory("audio-events-epoch-write-failure").unwrap();
    let target = root.path().join("capture.zaudio");
    let read_only_path = root.path().join("read-only-sink");
    std::fs::write(&read_only_path, b"sink").unwrap();

    let mut writer = ZeffAudioEventWriter::start(&target, context()).unwrap();
    writer.write_frame(&frame(10, 440.0)).unwrap();
    writer.writer = Some(BufWriter::with_capacity(
        0,
        File::open(&read_only_path).unwrap(),
    ));
    assert!(
        writer
            .begin_epoch(AudioTimelineDiscontinuity::Reset)
            .is_err()
    );
    assert_eq!(writer.epoch, 0);
    assert_eq!(writer.epoch_origin, Some(10));
    assert_eq!(writer.last_source_frame, Some(10));
    assert_eq!(writer.previous.len(), CHANNELS.len());
    drop(writer);

    assert!(!target.exists());
    assert!(std::fs::read_dir(root.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".tmp.")
    }));
}

#[test]
fn frame_order_must_increase_within_each_epoch() {
    let root = test_directory("audio-events-order").unwrap();
    let target = root.path().join("capture.zaudio");
    let mut writer = ZeffAudioEventWriter::start(&target, context()).unwrap();
    writer.write_frame(&frame(20, 440.0)).unwrap();
    assert_eq!(
        writer.write_frame(&frame(20, 442.0)).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    writer
        .begin_epoch(AudioTimelineDiscontinuity::Rewind)
        .unwrap();
    writer.write_frame(&frame(1, 442.0)).unwrap();
    writer.finish().unwrap();
}

#[test]
fn negative_zero_is_normalized_and_does_not_create_a_diff() {
    let root = test_directory("audio-events-negative-zero").unwrap();
    let target = root.path().join("capture.zaudio");
    let mut writer = ZeffAudioEventWriter::start(&target, context()).unwrap();
    let mut first = frame(7, -0.0);
    first.voices[0].level = Some(-0.0);
    writer.write_frame(&first).unwrap();
    let mut second = frame(8, 0.0);
    second.voices[0].level = Some(0.0);
    writer.write_frame(&second).unwrap();
    writer.finish().unwrap();

    let records = records(&target);
    assert_eq!(records[2]["frame"], 0);
    assert_eq!(records[2]["voices"][0]["pitch_hz"], 0.0);
    assert_eq!(records[2]["voices"][0]["level"], 0.0);
    assert_eq!(records.len(), 4);
    assert_eq!(records[3]["record"], "end");
    assert_eq!(records[3]["last_frame"], 1);
}

#[test]
fn topology_identity_wins_and_diffs_have_deterministic_channel_order() {
    let root = test_directory("audio-events-topology-identity").unwrap();
    let target = root.path().join("capture.zaudio");
    let mut writer = ZeffAudioEventWriter::start(&target, context()).unwrap();
    let mut first = frame(40, 440.0);
    first.voices.reverse();
    let pulse = first
        .voices
        .iter_mut()
        .find(|voice| voice.channel == AudioChannelId(0))
        .unwrap();
    pulse.name = "Dynamic hybrid mode";
    pulse.class = AudioVoiceClass::Noise;
    writer.write_frame(&first).unwrap();

    let mut second = first;
    second.frame = 41;
    second
        .voices
        .iter_mut()
        .find(|voice| voice.channel == AudioChannelId(0))
        .unwrap()
        .pitch_hz = None;
    writer.write_frame(&second).unwrap();
    writer.finish().unwrap();

    let records = records(&target);
    assert_eq!(records[2]["voices"][0]["channel"], 0);
    assert_eq!(records[2]["voices"][1]["channel"], 3);
    assert_eq!(records[3]["voices"].as_array().unwrap().len(), 1);
    assert_eq!(records[3]["voices"][0]["channel"], 0);
    assert!(records[3]["voices"][0]["pitch_hz"].is_null());
}
