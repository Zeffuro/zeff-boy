use std::io::Cursor;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use anyhow::{Context, Result, ensure};

use super::SongCandidate;
use super::mp2k::{self, Event};
use super::timeline::{
    GBA_CYCLES_PER_FRAME, Scheduled, ScheduledEvent, advance_time, build_timeline,
};

mod options;
#[cfg(test)]
mod tests;
mod voices;

use options::validate_options;
pub(crate) use options::{
    DEFAULT_SAMPLE_RATE, MAX_DURATION_SECONDS, MAX_FADE_SECONDS, MAX_LOOP_PASSES, PlaybackGain,
    RenderOptions, SAMPLE_RATES, validate_sample_rate,
};
use voices::{
    TrackControls, dispatch, send_cc, set_bend, set_bend_range, set_key_shift, set_tune,
    validate_programs, validate_soundfont_presets,
};

const SYNTH_BLOCK_FRAMES: usize = 8;
const MAX_PCM_BYTES: usize = 2 * 1024 * 1024 * 1024;
const PROGRESS_COMPLETE: u32 = 100;

#[cfg(test)]
pub(crate) struct RenderedAudio {
    pub(crate) pcm: Vec<i16>,
    pub(crate) sample_rate: u32,
    pub(crate) warnings: Vec<String>,
}

pub(crate) struct RenderSummary {
    pub(crate) frames: usize,
    pub(crate) sample_rate: u32,
    pub(crate) warnings: Vec<String>,
}

#[derive(Clone, Debug)]
struct FrameEvent {
    frame: usize,
    track: u8,
    event: ScheduledEvent,
}

pub(crate) struct RenderSession {
    song: SongCandidate,
    sound_font: Arc<rustysynth::SoundFont>,
    options: RenderOptions,
    events: Vec<FrameEvent>,
    output_frames: usize,
    song_end_frame: usize,
    fade_start: usize,
    base_warnings: Vec<String>,
    warnings: Vec<String>,
    synth: rustysynth::Synthesizer,
    controls: Vec<TrackControls>,
    event_index: usize,
    rendered_frames: usize,
    position_frames: usize,
    post_song: bool,
    track_mask: u16,
}

impl RenderSession {
    pub(crate) fn new(
        song: &SongCandidate,
        bytes: &[u8],
        sf2: &[u8],
        options: RenderOptions,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        validate_options(options)?;
        ensure!(!song.tracks.is_empty(), "song has no tracks");
        ensure!(
            song.tracks.len() <= 16,
            "songs with more than 16 tracks cannot be rendered"
        );
        check_cancelled(cancel)?;
        let programs = song
            .tracks
            .iter()
            .map(|track| mp2k::program_for_song(bytes, song, track.entry_address, cancel))
            .collect::<Result<Vec<_>>>()?;
        validate_programs(song, &programs)?;
        let (scheduled, song_end_tick) = build_timeline(&programs, options.loops)?;
        let (events, song_end_frame) = map_frames(scheduled, song_end_tick, options.sample_rate)?;
        let max_frames = usize::from(options.max_seconds)
            .checked_mul(options.sample_rate as usize)
            .context("render duration overflows")?;
        let fade_frames = usize::from(options.fade_seconds)
            .checked_mul(options.sample_rate as usize)
            .context("render duration overflows")?;
        let output_frames = song_end_frame
            .checked_add(fade_frames)
            .context("render duration overflows")?
            .min(max_frames);
        ensure!(output_frames > 0, "song has zero render duration");
        let pcm_bytes = output_frames
            .checked_mul(2)
            .and_then(|samples| samples.checked_mul(std::mem::size_of::<i16>()))
            .context("render output size overflows")?;
        ensure!(
            pcm_bytes <= MAX_PCM_BYTES,
            "render output exceeds the 2 GiB PCM limit"
        );
        let mut cursor = Cursor::new(sf2);
        let sound_font = Arc::new(
            rustysynth::SoundFont::new(&mut cursor).context("generated SoundFont is invalid")?,
        );
        validate_soundfont_presets(song, &programs, &sound_font)?;
        let mut warnings = vec![
            "SoundFont synthesis approximates MP2k mixer interpolation, envelopes, channel limits, and PSG output.".to_owned(),
            format!(
                "Sequence event times are quantized to {SYNTH_BLOCK_FRAMES}-sample synthesis blocks (at most {:.3} ms at {} Hz).",
                1000.0 * SYNTH_BLOCK_FRAMES as f64 / f64::from(options.sample_rate),
                options.sample_rate
            ),
        ];
        if song.reverb != 0 {
            warnings.push("MP2k reverb is not reproduced; the offline render disables SoundFont reverb and chorus.".to_owned());
        }
        if programs.iter().flat_map(|program| &program.events).any(
            |timed| matches!(timed.event, Event::Control { opcode: 0xC2..=0xC5, value } if value != 0),
        ) {
            warnings.push("MP2k LFO modulation controls are present and are not reproduced.".to_owned());
        }
        if song_end_frame > max_frames {
            warnings.push(format!(
                "The expanded song exceeded the {}-second limit and was truncated.",
                options.max_seconds
            ));
        }
        let fade_start = if song_end_frame <= max_frames {
            song_end_frame.min(output_frames)
        } else {
            output_frames.saturating_sub(fade_frames.min(output_frames))
        };
        let track_mask = track_mask_all(song.tracks.len());
        let synth = new_synth(&sound_font, options.sample_rate)?;
        let mut session = Self {
            song: song.clone(),
            sound_font,
            options,
            events,
            output_frames,
            song_end_frame,
            fade_start,
            base_warnings: warnings.clone(),
            warnings,
            synth,
            controls: Vec::new(),
            event_index: 0,
            rendered_frames: 0,
            position_frames: 0,
            post_song: false,
            track_mask,
        };
        session.reset()?;
        Ok(session)
    }

    pub(crate) fn duration_frames(&self) -> usize {
        self.output_frames
    }
    pub(crate) fn position_frames(&self) -> usize {
        self.position_frames
    }
    pub(crate) fn sample_rate(&self) -> u32 {
        self.options.sample_rate
    }
    pub(crate) fn track_count(&self) -> usize {
        self.song.tracks.len()
    }
    pub(crate) fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub(crate) fn set_track_mask(&mut self, track_mask: u16) -> Result<()> {
        let tracks = self.track_count();
        ensure!(
            track_mask & !track_mask_all(tracks) == 0,
            "track mask selects an unavailable track"
        );
        self.track_mask = track_mask;
        apply_track_mask(&mut self.synth, tracks, track_mask);
        Ok(())
    }

    pub(crate) fn reset(&mut self) -> Result<()> {
        let tracks = self.track_count();
        self.synth = new_synth(&self.sound_font, self.options.sample_rate)?;
        self.controls = vec![TrackControls::default(); tracks];
        initialize_channels(&mut self.synth, tracks)?;
        if self.track_mask != track_mask_all(tracks) {
            apply_track_mask(&mut self.synth, tracks, self.track_mask);
        }
        self.event_index = 0;
        self.rendered_frames = 0;
        self.position_frames = 0;
        self.post_song = false;
        self.warnings.clone_from(&self.base_warnings);
        Ok(())
    }

    pub(crate) fn read(&mut self, output: &mut [i16], cancel: &AtomicBool) -> Result<usize> {
        ensure!(
            output.len() >= SYNTH_BLOCK_FRAMES * 2 && output.len().is_multiple_of(2),
            "render read buffer must hold at least eight stereo frames"
        );
        check_cancelled(cancel)?;
        let remaining = self.output_frames - self.position_frames;
        if remaining == 0 {
            return Ok(0);
        }
        let capacity = output.len() / 2;
        let requested = if remaining <= capacity {
            remaining
        } else {
            capacity / SYNTH_BLOCK_FRAMES * SYNTH_BLOCK_FRAMES
        };
        ensure!(
            requested > 0,
            "render read buffer cannot hold a synthesis block"
        );
        let end_position = self.position_frames + requested;
        let mut written = 0;
        while self.position_frames < end_position {
            check_cancelled(cancel)?;
            while self.event_index < self.events.len()
                && self.events[self.event_index].frame < self.output_frames
                && self.events[self.event_index].frame <= self.rendered_frames
            {
                let event = &self.events[self.event_index];
                dispatch(
                    &mut self.synth,
                    &self.song,
                    event,
                    &mut self.controls,
                    self.options.playback_gain,
                    &mut self.warnings,
                )?;
                let tracks = self.track_count();
                if self.track_mask != track_mask_all(tracks) {
                    apply_track_mask(&mut self.synth, tracks, self.track_mask);
                }
                self.event_index += 1;
            }
            let next_event = self
                .events
                .get(self.event_index)
                .filter(|event| event.frame < self.output_frames)
                .map_or(self.song_end_frame.min(self.output_frames), |event| {
                    event.frame
                });
            if self.rendered_frames >= next_event {
                if self.event_index < self.events.len()
                    && self.events[self.event_index].frame < self.output_frames
                {
                    continue;
                }
                if !self.post_song {
                    if self.song_end_frame <= self.output_frames {
                        for channel in 0..self.track_count() {
                            send_cc(&mut self.synth, channel, 123, 0);
                        }
                    }
                    self.post_song = true;
                    continue;
                }
            }
            let target = if self.post_song {
                self.output_frames
            } else {
                next_event
            };
            let count = (target - self.rendered_frames)
                .min(SYNTH_BLOCK_FRAMES)
                .min(end_position - self.position_frames);
            let terminal = self.rendered_frames + count == self.output_frames;
            ensure!(
                count == SYNTH_BLOCK_FRAMES || terminal,
                "render block is not synthesis aligned"
            );
            let mut left = [0.0f32; SYNTH_BLOCK_FRAMES];
            let mut right = [0.0f32; SYNTH_BLOCK_FRAMES];
            self.synth.render(&mut left[..count], &mut right[..count]);
            for index in 0..count {
                let absolute = self.rendered_frames + index;
                let gain = if absolute < self.fade_start || self.output_frames == self.fade_start {
                    1.0
                } else {
                    (self.output_frames - absolute) as f32
                        / (self.output_frames - self.fade_start) as f32
                };
                output[(written + index) * 2] = float_to_pcm(left[index] * gain);
                output[(written + index) * 2 + 1] = float_to_pcm(right[index] * gain);
            }
            self.rendered_frames += count;
            self.position_frames += count;
            written += count;
        }
        Ok(written * 2)
    }
}

fn new_synth(
    sound_font: &Arc<rustysynth::SoundFont>,
    sample_rate: u32,
) -> Result<rustysynth::Synthesizer> {
    let mut settings = rustysynth::SynthesizerSettings::new(sample_rate as i32);
    settings.block_size = SYNTH_BLOCK_FRAMES;
    settings.maximum_polyphony = 64;
    settings.enable_reverb_and_chorus = false;
    rustysynth::Synthesizer::new(sound_font, &settings)
        .context("could not initialize the SoundFont synthesizer")
}

fn initialize_channels(synth: &mut rustysynth::Synthesizer, tracks: usize) -> Result<()> {
    for channel in 0..tracks {
        send_cc(synth, channel, 0, 0);
        send_cc(synth, channel, 7, 0);
        send_cc(synth, channel, 10, 64);
        set_key_shift(synth, channel, 0)?;
        set_tune(synth, channel, 0);
        set_bend_range(synth, channel, 2);
        set_bend(synth, channel, 0);
    }
    Ok(())
}

fn track_mask_all(tracks: usize) -> u16 {
    if tracks == 16 {
        u16::MAX
    } else {
        (1u16 << tracks) - 1
    }
}

fn apply_track_mask(synth: &mut rustysynth::Synthesizer, tracks: usize, mask: u16) {
    for channel in 0..tracks {
        send_cc(
            synth,
            channel,
            11,
            if mask & (1 << channel) != 0 { 127 } else { 0 },
        );
    }
}

#[cfg(test)]
pub(crate) fn render(
    song: &SongCandidate,
    bytes: &[u8],
    sf2: &[u8],
    options: RenderOptions,
    cancel: &AtomicBool,
    progress: &AtomicU32,
) -> Result<RenderedAudio> {
    let mut pcm = Vec::new();
    let summary = render_into(song, bytes, sf2, options, cancel, progress, &mut |block| {
        ensure!(
            pcm.len() + block.len() <= 64 * 1024 * 1024,
            "in-memory test render exceeds its limit"
        );
        pcm.extend_from_slice(block);
        Ok(())
    })?;
    Ok(RenderedAudio {
        pcm,
        sample_rate: summary.sample_rate,
        warnings: summary.warnings,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_into(
    song: &SongCandidate,
    bytes: &[u8],
    sf2: &[u8],
    options: RenderOptions,
    cancel: &AtomicBool,
    progress: &AtomicU32,
    output: &mut impl FnMut(&[i16]) -> Result<()>,
) -> Result<RenderSummary> {
    progress.store(0, Ordering::Relaxed);
    let mut session = RenderSession::new(song, bytes, sf2, options, cancel)?;
    let mut block = [0i16; 8192];
    while session.position_frames() < session.duration_frames() {
        let written = session.read(&mut block, cancel)?;
        ensure!(written != 0, "render session stopped before its duration");
        output(&block[..written])?;
        let complete = ((session.position_frames() as u128 * u128::from(PROGRESS_COMPLETE))
            / session.duration_frames() as u128) as u32;
        progress.store(complete.min(PROGRESS_COMPLETE - 1), Ordering::Relaxed);
    }
    progress.store(PROGRESS_COMPLETE, Ordering::Release);
    Ok(RenderSummary {
        frames: session.duration_frames(),
        sample_rate: session.sample_rate(),
        warnings: session.warnings().to_vec(),
    })
}
fn map_frames(
    timeline: Vec<Scheduled>,
    song_end_tick: u32,
    sample_rate: u32,
) -> Result<(Vec<FrameEvent>, usize)> {
    let numerator = 75u128 * u128::from(sample_rate) * GBA_CYCLES_PER_FRAME;
    let mut sample_time_q32 = 0u128;
    let mut previous_tick = 0u32;
    let mut tempo = 75u8;
    let mut output = Vec::with_capacity(timeline.len());
    for scheduled in timeline {
        ensure!(
            scheduled.tick >= previous_tick,
            "sequence timeline is not ordered"
        );
        sample_time_q32 = advance_time(
            sample_time_q32,
            scheduled.tick - previous_tick,
            tempo,
            numerator,
        )?;
        previous_tick = scheduled.tick;
        let frame = quantized_frame(sample_time_q32)?;
        if let ScheduledEvent::Sequence(Event::Control {
            opcode: 0xBB,
            value,
        }) = scheduled.event
        {
            ensure!(
                value != 0,
                "sequence contains a zero tempo that stops sequence time"
            );
            tempo = value;
        }
        output.push(FrameEvent {
            frame,
            track: scheduled.track,
            event: scheduled.event,
        });
    }
    if previous_tick < song_end_tick {
        sample_time_q32 = advance_time(
            sample_time_q32,
            song_end_tick - previous_tick,
            tempo,
            numerator,
        )?;
    }
    Ok((output, quantized_frame(sample_time_q32)?))
}

fn quantized_frame(sample_time_q32: u128) -> Result<usize> {
    let frame = usize::try_from(sample_time_q32 >> 32).context("sequence duration is too large")?;
    Ok((frame + SYNTH_BLOCK_FRAMES / 2) / SYNTH_BLOCK_FRAMES * SYNTH_BLOCK_FRAMES)
}

fn float_to_pcm(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
}

fn push_warning(warnings: &mut Vec<String>, warning: &str) {
    if !warnings.iter().any(|existing| existing == warning) {
        warnings.push(warning.to_owned());
    }
}

fn check_cancelled(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "audio render cancelled");
    Ok(())
}
