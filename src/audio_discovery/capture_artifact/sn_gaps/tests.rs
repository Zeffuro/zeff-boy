use std::sync::atomic::AtomicBool;

use anyhow::Result;
use zeff_emu_common::audio_trace::{AudioTrace, AudioTraceWrite};

use super::select;
use crate::audio_discovery::validation::Interval;

fn trace(events: &[(u64, AudioTraceWrite)], end_cycle: u64, stereo: bool) -> AudioTrace {
    let (mut trace, _) = crate::audio_discovery::trace_capture::tests::sega_trace("gg");
    trace.cycle_hz = 48_000;
    trace.cycle_hz_denominator = 1;
    trace.chip.clock_hz = 48_000;
    trace.chip.stereo = stereo;
    trace.end_cycle = end_cycle;
    let source = trace.events[0].instruction_source;
    trace.events = events
        .iter()
        .map(
            |&(cycle, write)| zeff_emu_common::audio_trace::AudioTraceEvent {
                cycle,
                pc: 0,
                instruction_source: source,
                write,
            },
        )
        .collect();
    trace
}

fn activity() -> Vec<Interval> {
    vec![
        Interval {
            start_frame: 0,
            end_frame: 100,
        },
        Interval {
            start_frame: 200,
            end_frame: 300,
        },
    ]
}

fn write(value: u8) -> AudioTraceWrite {
    AudioTraceWrite::Sn76489 { port: 0x7f, value }
}

#[test]
fn keeps_only_gaps_with_a_full_native_mute_run() -> Result<()> {
    let trace = trace(
        &[(0, write(0x90)), (100, write(0x9f)), (200, write(0x90))],
        300,
        false,
    );
    let cancel = AtomicBool::new(false);
    let (selected, metadata) = select(&trace, 300, 48_000, &activity(), 100, &cancel)?;
    assert_eq!(selected, activity());
    assert_eq!(metadata["gap_decisions"][0]["longest_muted_frames"], 100);
    assert_eq!(metadata["gap_decisions"][0]["retained_split"], true);
    let (selected, metadata) = select(&trace, 300, 48_000, &activity(), 101, &cancel)?;
    assert_eq!(
        selected,
        vec![Interval {
            start_frame: 0,
            end_frame: 300
        }]
    );
    assert_eq!(metadata["merged_gap_count"], 1);
    Ok(())
}

#[test]
fn data_and_noise_volume_updates_control_muting() -> Result<()> {
    let cancel = AtomicBool::new(false);
    let data_trace = trace(
        &[(0, write(0x90)), (100, write(0x0f)), (200, write(0x00))],
        300,
        false,
    );
    let (_, data) = select(&data_trace, 300, 48_000, &activity(), 100, &cancel)?;
    assert_eq!(data["gap_decisions"][0]["retained_split"], true);
    let noise_trace = trace(
        &[(0, write(0xf0)), (100, write(0xff)), (200, write(0xf0))],
        300,
        false,
    );
    let (_, noise) = select(&noise_trace, 300, 48_000, &activity(), 100, &cancel)?;
    assert_eq!(noise["gap_decisions"][0]["retained_split"], true);
    Ok(())
}

#[test]
fn game_gear_stereo_does_not_change_the_latched_volume_register() -> Result<()> {
    let trace = trace(
        &[
            (0, write(0x90)),
            (100, AudioTraceWrite::GameGearStereo { port: 6, value: 0 }),
            (150, write(0x0f)),
            (
                200,
                AudioTraceWrite::GameGearStereo {
                    port: 6,
                    value: 0xff,
                },
            ),
        ],
        300,
        true,
    );
    let (_, metadata) = select(
        &trace,
        300,
        48_000,
        &activity(),
        100,
        &AtomicBool::new(false),
    )?;
    assert_eq!(metadata["gap_decisions"][0]["longest_muted_frames"], 100);
    Ok(())
}

#[test]
fn a_subsample_unmute_interrupts_a_mute_run_without_summing_it() -> Result<()> {
    let trace = trace(
        &[
            (0, write(0x90)),
            (100, write(0x9f)),
            (101, write(0x90)),
            (102, write(0x9f)),
            (200, write(0x90)),
        ],
        300,
        false,
    );
    let activity = vec![
        Interval {
            start_frame: 0,
            end_frame: 91,
        },
        Interval {
            start_frame: 183,
            end_frame: 250,
        },
    ];
    let (_, metadata) = select(&trace, 300, 44_100, &activity, 91, &AtomicBool::new(false))?;
    assert_eq!(metadata["gap_decisions"][0]["longest_muted_frames"], 90);
    assert_eq!(metadata["gap_decisions"][0]["retained_split"], false);
    Ok(())
}

#[test]
fn clamps_tail_runs_and_rejects_cancelled_or_invalid_input() -> Result<()> {
    let trace = trace(&[(0, write(0x90)), (200, write(0x9f))], 1_000, false);
    let activity = vec![
        Interval {
            start_frame: 0,
            end_frame: 200,
        },
        Interval {
            start_frame: 250,
            end_frame: 300,
        },
    ];
    let (_, metadata) = select(&trace, 300, 48_000, &activity, 50, &AtomicBool::new(false))?;
    assert_eq!(metadata["gap_decisions"][0]["longest_muted_frames"], 50);
    assert!(select(&trace, 300, 48_000, &activity, 50, &AtomicBool::new(true)).is_err());
    assert!(
        select(
            &trace,
            300,
            48_000,
            &[Interval {
                start_frame: 2,
                end_frame: 2
            }],
            50,
            &AtomicBool::new(false)
        )
        .is_err()
    );
    let (silent, metadata) = select(&trace, 300, 48_000, &[], 50, &AtomicBool::new(false))?;
    assert!(silent.is_empty());
    assert_eq!(metadata["source_activity_count"], 0);
    Ok(())
}
