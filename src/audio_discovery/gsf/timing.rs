use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use super::super::SongCandidate;
use super::super::mp2k::{self, Event};
use super::super::render::{
    MAX_DURATION_SECONDS, MAX_FADE_SECONDS, MAX_LOOP_PASSES, RenderOptions,
};
use super::super::timeline::{GBA_CYCLES_PER_FRAME, ScheduledEvent, advance_time, build_timeline};

#[derive(Serialize)]
pub(super) struct Playback {
    length_ms: u64,
    fade_ms: u64,
    loop_passes: u8,
    maximum_seconds: u16,
}

impl Playback {
    pub(super) fn length_tag(&self) -> String {
        duration_tag(self.length_ms)
    }

    pub(super) fn fade_tag(&self) -> String {
        duration_tag(self.fade_ms)
    }
}

pub(super) fn validate_options(options: RenderOptions) -> Result<()> {
    ensure!(
        (1..=MAX_LOOP_PASSES).contains(&options.loops),
        "GSF loop passes must be between 1 and {MAX_LOOP_PASSES}"
    );
    ensure!(
        (1..=MAX_DURATION_SECONDS).contains(&options.max_seconds),
        "GSF duration must be between 1 and {MAX_DURATION_SECONDS} seconds"
    );
    ensure!(
        options.fade_seconds <= MAX_FADE_SECONDS,
        "GSF fade must be between 0 and {MAX_FADE_SECONDS} seconds"
    );
    Ok(())
}

pub(super) fn playback(
    song: &SongCandidate,
    bytes: &[u8],
    options: RenderOptions,
    cancel: &AtomicBool,
) -> Result<Playback> {
    validate_options(options)?;
    let programs = song
        .tracks
        .iter()
        .map(|track| mp2k::program_for_song(bytes, song, track.entry_address, cancel))
        .collect::<Result<Vec<_>>>()?;
    let (timeline, end_tick) = build_timeline(&programs, options.loops)?;
    let numerator = 75 * 1000 * GBA_CYCLES_PER_FRAME;
    let mut elapsed = 0;
    let mut previous_tick = 0;
    let mut tempo = 75;
    for event in timeline {
        ensure!(!cancel.load(Ordering::Relaxed), "GSF export cancelled");
        elapsed = advance_time(elapsed, event.tick - previous_tick, tempo, numerator)?;
        previous_tick = event.tick;
        if let ScheduledEvent::Sequence(Event::Control {
            opcode: 0xBB,
            value,
        }) = event.event
        {
            ensure!(value != 0, "GSF timing contains a zero tempo");
            tempo = value;
        }
    }
    elapsed = advance_time(elapsed, end_tick - previous_tick, tempo, numerator)?;
    let duration = u64::try_from((elapsed + (1 << 31)) >> 32).context("GSF duration overflows")?;
    Ok(cap_duration(duration, options))
}

fn cap_duration(duration_ms: u64, options: RenderOptions) -> Playback {
    let maximum = u64::from(options.max_seconds) * 1000;
    let requested_fade = u64::from(options.fade_seconds) * 1000;
    let length = if duration_ms <= maximum {
        duration_ms
    } else {
        maximum.saturating_sub(requested_fade)
    }
    .clamp(1, maximum);
    Playback {
        length_ms: length,
        fade_ms: requested_fade.min(maximum - length),
        loop_passes: options.loops,
        maximum_seconds: options.max_seconds,
    }
}

fn duration_tag(milliseconds: u64) -> String {
    format!(
        "{}:{:02}.{:03}",
        milliseconds / 60_000,
        milliseconds / 1000 % 60,
        milliseconds % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_tags_preserve_duration_and_cap_total_time_including_fade() {
        let options = RenderOptions {
            max_seconds: 5,
            fade_seconds: 2,
            ..RenderOptions::default()
        };
        for (duration, length, fade) in [(1234, 1234, 2000), (4234, 4234, 766), (6000, 3000, 2000)]
        {
            let playback = cap_duration(duration, options);
            assert_eq!((playback.length_ms, playback.fade_ms), (length, fade));
            assert!(playback.length_ms + playback.fade_ms <= 5000);
        }
        let shortest = cap_duration(
            6000,
            RenderOptions {
                max_seconds: 1,
                fade_seconds: 15,
                ..options
            },
        );
        assert_eq!((shortest.length_ms, shortest.fade_ms), (1, 999));
        assert_eq!(duration_tag(0), "0:00.000");
        assert_eq!(duration_tag(61_234), "1:01.234");
    }
}
