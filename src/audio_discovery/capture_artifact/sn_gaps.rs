use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use serde_json::{Value, json};
use zeff_emu_common::audio_trace::{AudioTrace, AudioTraceWrite};

use crate::audio_discovery::validation::Interval;

pub(super) fn select(
    trace: &AudioTrace,
    pcm_frames: usize,
    sample_rate: u32,
    activity: &[Interval],
    minimum_muted_frames: usize,
    cancel: &AtomicBool,
) -> Result<(Vec<Interval>, Value)> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "SN muted-gap selection cancelled"
    );
    super::super::render::validate_sample_rate(sample_rate)?;
    ensure!(
        pcm_frames > 0 && pcm_frames <= 120 * sample_rate as usize,
        "SN muted-gap PCM duration is outside the excerpt bound"
    );
    ensure!(
        activity.len() <= 256,
        "SN muted-gap activity interval count is outside the excerpt bound"
    );
    ensure!(minimum_muted_frames > 0, "SN muted-gap threshold is empty");
    ensure!(trace.cycle_hz > 0, "SN muted-gap trace clock is invalid");
    validate_activity(activity, pcm_frames)?;

    let mut decisions = activity
        .windows(2)
        .enumerate()
        .map(|(index, pair)| GapDecision {
            before_index: index,
            after_index: index + 1,
            start_frame: pair[0].end_frame,
            end_frame: pair[1].start_frame,
            longest_muted_frames: 0,
        })
        .collect::<Vec<_>>();
    let mut state = State::new(trace.chip.stereo);
    let mut run_start = state.muted().then_some(0);
    let mut gap_index = 0;
    for event in &trace.events {
        ensure!(
            !cancel.load(Ordering::Relaxed),
            "SN muted-gap selection cancelled"
        );
        let was_muted = state.muted();
        state.apply(event.write);
        let is_muted = state.muted();
        if was_muted && !is_muted {
            observe_run(
                run_start.take().expect("muted state has a start cycle")..event.cycle,
                trace,
                pcm_frames,
                sample_rate,
                &mut decisions,
                &mut gap_index,
            )?;
        } else if !was_muted && is_muted {
            run_start = Some(event.cycle);
        }
    }
    if let Some(start) = run_start {
        observe_run(
            start..trace.end_cycle,
            trace,
            pcm_frames,
            sample_rate,
            &mut decisions,
            &mut gap_index,
        )?;
    }

    let mut merged_gap_count = 0;
    let mut selected = Vec::with_capacity(activity.len());
    if let Some(first) = activity.first() {
        let mut current = Interval {
            start_frame: first.start_frame,
            end_frame: first.end_frame,
        };
        for (index, decision) in decisions.iter_mut().enumerate() {
            let retained_split = decision.longest_muted_frames >= minimum_muted_frames;
            merged_gap_count += usize::from(!retained_split);
            if retained_split {
                selected.push(current);
                current = Interval {
                    start_frame: activity[index + 1].start_frame,
                    end_frame: activity[index + 1].end_frame,
                };
            } else {
                current.end_frame = activity[index + 1].end_frame;
            }
        }
        selected.push(current);
    }
    let metadata = json!({
        "method": "amplitude_gap_with_native_psg_mute",
        "timing_contract": "frame k sampled at ceil((k + 1) * cycle_hz / sample_rate) before writes at that cycle",
        "cycle_hz": trace.cycle_hz,
        "sample_rate": sample_rate,
        "source_activity_count": activity.len(),
        "merged_gap_count": merged_gap_count,
        "minimum_muted_frames": minimum_muted_frames,
        "gap_decisions": decisions.into_iter().map(|decision| json!({
            "before_index": decision.before_index,
            "after_index": decision.after_index,
            "start_frame": decision.start_frame,
            "end_frame": decision.end_frame,
            "longest_muted_frames": decision.longest_muted_frames,
            "retained_split": decision.longest_muted_frames >= minimum_muted_frames,
        })).collect::<Vec<_>>(),
    });
    Ok((selected, metadata))
}

struct State {
    latched_register: u8,
    volumes: [u8; 4],
    stereo_control: u8,
    stereo_enabled: bool,
}

impl State {
    fn new(stereo_enabled: bool) -> Self {
        Self {
            latched_register: 0,
            volumes: [15; 4],
            stereo_control: 0xff,
            stereo_enabled,
        }
    }

    fn apply(&mut self, write: AudioTraceWrite) {
        match write {
            AudioTraceWrite::Sn76489 { value, .. } => {
                if value & 0x80 != 0 {
                    self.latched_register = (value >> 4) & 7;
                    self.apply_latched(value & 15);
                } else if self.latched_register & 1 != 0 {
                    self.volumes[usize::from(self.latched_register >> 1)] = value & 15;
                }
            }
            AudioTraceWrite::GameGearStereo { value, .. } => self.stereo_control = value,
        }
    }

    fn apply_latched(&mut self, value: u8) {
        if self.latched_register & 1 != 0 {
            self.volumes[usize::from(self.latched_register >> 1)] = value;
        }
    }

    fn muted(&self) -> bool {
        self.volumes.iter().enumerate().all(|(channel, &volume)| {
            volume == 15
                || (self.stereo_enabled
                    && self.stereo_control & ((1 << channel) | (1 << (channel + 4))) == 0)
        })
    }
}

struct GapDecision {
    before_index: usize,
    after_index: usize,
    start_frame: usize,
    end_frame: usize,
    longest_muted_frames: usize,
}

fn validate_activity(activity: &[Interval], pcm_frames: usize) -> Result<()> {
    let mut previous_end = 0;
    for interval in activity {
        ensure!(
            interval.start_frame < interval.end_frame && interval.end_frame <= pcm_frames,
            "SN muted-gap activity interval is invalid"
        );
        ensure!(
            interval.start_frame >= previous_end,
            "SN muted-gap activity intervals are not ordered"
        );
        previous_end = interval.end_frame;
    }
    Ok(())
}

fn observe_run(
    cycles: std::ops::Range<u64>,
    trace: &AudioTrace,
    pcm_frames: usize,
    sample_rate: u32,
    decisions: &mut [GapDecision],
    gap_index: &mut usize,
) -> Result<()> {
    ensure!(
        cycles.end >= cycles.start,
        "SN muted-gap trace cycles are not ordered"
    );
    let start_frame = cycle_to_frame(cycles.start, trace.cycle_hz, sample_rate, pcm_frames)?;
    let end_frame = cycle_to_frame(cycles.end, trace.cycle_hz, sample_rate, pcm_frames)?;
    while decisions
        .get(*gap_index)
        .is_some_and(|decision| decision.end_frame <= start_frame)
    {
        *gap_index += 1;
    }
    while let Some(decision) = decisions.get_mut(*gap_index) {
        if decision.start_frame >= end_frame {
            break;
        }
        let intersection_start = decision.start_frame.max(start_frame);
        let intersection_end = decision.end_frame.min(end_frame);
        decision.longest_muted_frames = decision
            .longest_muted_frames
            .max(intersection_end.saturating_sub(intersection_start));
        if decision.end_frame <= end_frame {
            *gap_index += 1;
        } else {
            break;
        }
    }
    Ok(())
}

fn cycle_to_frame(cycle: u64, cycle_hz: u32, sample_rate: u32, pcm_frames: usize) -> Result<usize> {
    let frame = u128::from(cycle) * u128::from(sample_rate) / u128::from(cycle_hz);
    Ok(usize::try_from(frame)?.min(pcm_frames))
}

#[cfg(test)]
mod tests;
