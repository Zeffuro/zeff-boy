use super::*;
use crate::hardware::blip_buf::BlipBuf;
use anyhow::bail;
use std::fmt;
use zeff_emu_common::save_state::{StateReader, StateWriter};

pub(super) const ADPCM_RESET_PREDICTOR: u16 = 0x0800;

const ADPCM_STEP_TABLE: [u16; 49] = [
    16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130,
    143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796,
    876, 963, 1060, 1166, 1282, 1411, 1552,
];
const ADPCM_INDEX_SHIFT: [i8; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];
const ADPCM_BLIP_LEVEL_SCALE: i32 = 16;
const ADPCM_BLIP_FRAME_CLOCKS: u32 = 65_536;
const ADPCM_BLIP_BUFFER_MIN_SAMPLES: u32 = 2_048;
const ADPCM_BLIP_BUFFER_MARGIN: u32 = 64;

pub(super) struct MonoBlipResampler {
    buffer: BlipBuf,
    clocks: u32,
    level: i32,
}

impl fmt::Debug for MonoBlipResampler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MonoBlipResampler")
            .field("clocks", &self.clocks)
            .field("level", &self.level)
            .finish_non_exhaustive()
    }
}

impl MonoBlipResampler {
    pub(super) fn new(sample_rate: u32) -> Self {
        Self::at_level(sample_rate, 0)
    }

    pub(super) fn at_level(sample_rate: u32, level: i32) -> Self {
        let sample_rate = sample_rate.clamp(1, super::super::psg::MAX_PSG_SAMPLE_RATE);
        let mut buffer = BlipBuf::new(adpcm_blip_buffer_samples(sample_rate));
        buffer
            .set_rates(
                CDROM2_MASTER_CLOCK_NUMERATOR as f64 / CDROM2_MASTER_CLOCK_DENOMINATOR as f64,
                f64::from(sample_rate),
            )
            .unwrap();
        Self {
            buffer,
            clocks: 0,
            level,
        }
    }

    fn advance_clocks(&mut self, mut clocks: u64, output: &mut Vec<i16>) {
        while clocks != 0 {
            let step = clocks.min(u64::from(ADPCM_BLIP_FRAME_CLOCKS - self.clocks)) as u32;
            self.clocks += step;
            clocks -= u64::from(step);
            if self.clocks == ADPCM_BLIP_FRAME_CLOCKS {
                self.flush(output);
            }
        }
    }

    fn refresh_level(&mut self, level: i32) {
        if level == self.level {
            return;
        }
        self.buffer
            .add_delta(self.clocks, level - self.level)
            .unwrap();
        self.level = level;
    }

    fn flush(&mut self, output: &mut Vec<i16>) {
        if self.clocks == 0 {
            return;
        }
        self.buffer.end_frame(self.clocks).unwrap();
        self.clocks = 0;
        let available = self.buffer.samples_avail() as usize;
        let start = output.len();
        output.resize(start + available, 0);
        let read = self.buffer.read_samples(&mut output[start..], false);
        debug_assert_eq!(read, available);
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u32(self.clocks);
        writer.write_u32(self.level as u32);
        self.buffer.write_state(writer);
    }

    pub(super) fn read_state(
        sample_rate: u32,
        reader: &mut StateReader<'_>,
    ) -> anyhow::Result<Self> {
        let clocks = reader.read_u32()?;
        if clocks >= ADPCM_BLIP_FRAME_CLOCKS {
            bail!("invalid ADPCM resampler clock in save-state: {clocks}");
        }
        let level = reader.read_u32()? as i32;
        if !(-0x8000..=0x7FC0).contains(&level) {
            bail!("invalid ADPCM resampler level in save-state: {level}");
        }
        let mut restored = Self::at_level(sample_rate, level);
        restored.clocks = clocks;
        restored.buffer.read_state(reader)?;
        Ok(restored)
    }

    pub(super) const fn level(&self) -> i32 {
        self.level
    }
}

impl CdRom2 {
    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.audio_sample_rate = sample_rate.clamp(1, super::super::psg::MAX_PSG_SAMPLE_RATE);
        self.audio_source_frames.clear();
        self.audio_resample_position = 0.0;
        self.adpcm_resampler =
            MonoBlipResampler::at_level(self.audio_sample_rate, self.current_adpcm_output_level());
        self.adpcm_audio_samples.clear();
    }

    pub fn set_sample_generation_enabled(&mut self, enabled: bool) {
        if enabled && !self.audio_sample_generation_enabled {
            self.adpcm_resampler = MonoBlipResampler::at_level(
                self.audio_sample_rate,
                self.current_adpcm_output_level(),
            );
        }
        self.audio_sample_generation_enabled = enabled;
        self.audio_source_frames.clear();
        self.audio_resample_position = 0.0;
        self.adpcm_audio_samples.clear();
        if !enabled {
            self.adpcm_resampler = MonoBlipResampler::new(self.audio_sample_rate);
        }
    }

    pub fn mix_audio_samples_into(&mut self, output: &mut [f32]) {
        if self.audio_sample_generation_enabled {
            self.adpcm_resampler.flush(&mut self.adpcm_audio_samples);
        }
        let adpcm_available = (output.len() / 2).min(self.adpcm_audio_samples.len());
        let step = CDDA_SOURCE_RATE / f64::from(self.audio_sample_rate);
        for (index, frame) in output.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let cdda_available = !self.audio_source_frames.is_empty();
            while self.audio_resample_position >= 1.0 && self.audio_source_frames.len() > 1 {
                self.audio_source_frames.pop_front();
                self.audio_resample_position -= 1.0;
            }
            let current = self
                .audio_source_frames
                .front()
                .copied()
                .unwrap_or([0.0; 2]);
            let next = self.audio_source_frames.get(1).copied().unwrap_or(current);
            let fraction = self.audio_resample_position as f32;
            let adpcm = self
                .adpcm_audio_samples
                .get(index)
                .copied()
                .map(|sample| f32::from(sample) / 32_768.0 * PROVISIONAL_CDROM2_ADPCM_MIX_GAIN)
                .unwrap_or(0.0);
            for side in 0..2 {
                let cdda = current[side] + (next[side] - current[side]) * fraction;
                frame[side] = (frame[side] + cdda * CDDA_MIX_GAIN + adpcm).clamp(-1.0, 1.0);
            }
            // Silence before a CDDA command is not part of the 44.1 kHz source
            // stream. Advancing here with an empty queue leaves a large stale
            // position that consumes the beginning of the next track as soon as
            // its first frames arrive.
            if cdda_available {
                self.audio_resample_position += step;
            }
        }
        self.adpcm_audio_samples.drain(..adpcm_available);
    }

    pub(super) fn write_audio_fade_control(&mut self, value: u8) {
        let previous_function = self.adpcm_fade_control & 0x0E;
        let function = value & 0x0E;
        self.adpcm_fade_control = value;
        if function & 0x08 == 0 {
            self.audio_fade_target = None;
            self.audio_fade_level_q16 = 0x1_0000;
            self.audio_fade_step_ticks = 0;
            self.audio_fade_ticks_to_next = 0;
            self.refresh_adpcm_output();
            return;
        }
        if self.audio_fade_target.is_some() && function == previous_function {
            return;
        }

        let target = if function & 0x02 == 0 {
            CdFadeTarget::Cdda
        } else {
            CdFadeTarget::Adpcm
        };
        let step_ticks = if function & 0x04 == 0 {
            PROVISIONAL_CDROM2_FADE_LONG_STEP_TICKS
        } else {
            PROVISIONAL_CDROM2_FADE_SHORT_STEP_TICKS
        };
        if self.audio_fade_target.is_none() {
            self.audio_fade_level_q16 = 0x1_0000;
            self.audio_fade_ticks_to_next = step_ticks;
        }
        self.audio_fade_target = Some(target);
        self.audio_fade_step_ticks = step_ticks;
        self.refresh_adpcm_output();
    }

    pub(super) fn advance_audio(&mut self, mut ticks: u64) {
        self.audio_sample_latch_clock = self.audio_sample_latch_clock.wrapping_add(ticks);
        while ticks != 0 {
            let step = if self.audio_fade_target.is_some() && self.audio_fade_level_q16 != 0 {
                ticks.min(self.audio_fade_ticks_to_next)
            } else {
                ticks
            };
            self.advance_adpcm_playback(step);
            self.advance_cdda(step);
            ticks -= step;
            if self.audio_fade_target.is_some() && self.audio_fade_level_q16 != 0 {
                self.audio_fade_ticks_to_next -= step;
            }
            if self.audio_fade_target.is_some()
                && self.audio_fade_level_q16 != 0
                && self.audio_fade_ticks_to_next == 0
            {
                self.audio_fade_level_q16 -= 1;
                self.audio_fade_ticks_to_next = if self.audio_fade_level_q16 == 0 {
                    0
                } else {
                    self.audio_fade_step_ticks
                };
                self.refresh_adpcm_output();
            }
        }
    }

    pub(super) fn latch_cdda_sample(&mut self) {
        if self
            .audio_sample_latch_clock
            .wrapping_sub(self.audio_sample_latch_last_clock)
            < CDDA_SAMPLE_LATCH_TICKS
        {
            return;
        }
        self.audio_sample_latch_last_clock = self.audio_sample_latch_clock;
        self.audio_sample_latch_right = !self.audio_sample_latch_right;
        self.audio_sample_latch = if self.audio_sample_latch_right {
            self.audio_right_sample
        } else {
            self.audio_left_sample
        };
    }

    pub(super) fn write_adpcm_address_control(&mut self, value: u8) {
        if value & 0x80 != 0 {
            self.adpcm_address_latch = 0;
            self.adpcm_read_address = 0;
            self.adpcm_write_address = 0;
            self.adpcm_read_buffer = 0;
            self.adpcm_length = 0;
            self.adpcm_playing = false;
            self.adpcm_stop_pending = false;
            self.reset_adpcm_codec();
            self.adpcm_end_irq = false;
            self.adpcm_half_irq = false;
            self.adpcm_address_control = 0;
            self.refresh_adpcm_output();
            return;
        }
        if value & 2 != 0 && self.adpcm_address_control & 2 == 0 {
            self.adpcm_write_address = self
                .adpcm_address_latch
                .wrapping_sub(u16::from(value & 1 == 0));
        }
        if value & 8 != 0 && self.adpcm_address_control & 8 == 0 {
            self.adpcm_read_address = self
                .adpcm_address_latch
                .wrapping_sub(u16::from(value & 4 == 0));
            self.adpcm_read_buffer = self.adpcm_ram[usize::from(self.adpcm_read_address)];
        }
        if value & 0x10 != 0 {
            self.reload_adpcm_length();
        }
        if value & 0x20 != 0 {
            if !self.adpcm_playing && (value & 0x40 == 0 || !self.adpcm_end_irq) {
                self.reset_adpcm_codec();
                self.adpcm_playing = true;
            }
            self.adpcm_stop_pending = false;
        } else if self.adpcm_playing {
            self.adpcm_stop_pending = true;
        }
        self.adpcm_address_control = value;
        self.refresh_adpcm_output();
    }

    pub(super) fn advance_adpcm_playback(&mut self, ticks: u64) {
        let mut remaining = ticks;
        while self.adpcm_playing && remaining != 0 {
            let threshold = ADPCM_CLOCK_DENOMINATOR * (16 - u64::from(self.adpcm_playback_rate));
            let ticks_to_nibble =
                (threshold - self.adpcm_clock_accumulator).div_ceil(ADPCM_CLOCK_NUMERATOR);
            if remaining < ticks_to_nibble {
                self.adpcm_clock_accumulator += remaining * ADPCM_CLOCK_NUMERATOR;
                self.advance_adpcm_resampler(remaining);
                return;
            }
            self.adpcm_clock_accumulator += ticks_to_nibble * ADPCM_CLOCK_NUMERATOR;
            self.advance_adpcm_resampler(ticks_to_nibble);
            remaining -= ticks_to_nibble;
            self.adpcm_clock_accumulator -= threshold;
            if self.adpcm_stop_pending {
                self.adpcm_stop_pending = false;
                self.adpcm_playing = false;
                self.refresh_adpcm_output();
            } else {
                self.decode_adpcm_nibble();
            }
        }
        self.advance_adpcm_resampler(remaining);
    }

    fn decode_adpcm_nibble(&mut self) {
        let byte = self.adpcm_ram[usize::from(self.adpcm_read_address)];
        let nibble = if self.adpcm_high_nibble_next {
            byte >> 4
        } else {
            byte & 0x0F
        };
        let step = ADPCM_STEP_TABLE[usize::from(self.adpcm_step_index)];
        let mut delta = step >> 3;
        if nibble & 1 != 0 {
            delta += step >> 2;
        }
        if nibble & 2 != 0 {
            delta += step >> 1;
        }
        if nibble & 4 != 0 {
            delta += step;
        }
        self.adpcm_predictor = if nibble & 8 == 0 {
            self.adpcm_predictor.wrapping_add(delta)
        } else {
            self.adpcm_predictor.wrapping_sub(delta)
        } & 0x0FFF;
        let shift = ADPCM_INDEX_SHIFT[usize::from(nibble & 7)];
        self.adpcm_step_index = (self.adpcm_step_index as i8 + shift).clamp(0, 48) as u8;

        self.adpcm_high_nibble_next = !self.adpcm_high_nibble_next;
        if self.adpcm_high_nibble_next {
            self.adpcm_read_address = self.adpcm_read_address.wrapping_add(1);
            if self.adpcm_address_control & 0x10 == 0 {
                self.adpcm_length = self.adpcm_length.saturating_sub(1);
                self.adpcm_half_irq = self.adpcm_length != 0 && self.adpcm_length <= 0x8000;
                if self.adpcm_length == 0 {
                    self.adpcm_end_irq = true;
                    self.adpcm_half_irq = false;
                    if self.adpcm_address_control & 0x40 != 0 {
                        self.adpcm_stop_pending = true;
                    }
                }
            }
        }
        self.refresh_adpcm_output();
    }

    fn reset_adpcm_codec(&mut self) {
        self.adpcm_predictor = ADPCM_RESET_PREDICTOR;
        self.adpcm_step_index = 0;
        self.adpcm_high_nibble_next = true;
        self.adpcm_clock_accumulator = 0;
    }

    pub(super) fn write_adpcm_playback_rate(&mut self, value: u8) {
        let old_threshold = ADPCM_CLOCK_DENOMINATOR * (16 - u64::from(self.adpcm_playback_rate));
        let new_rate = value & 0x0F;
        let new_threshold = ADPCM_CLOCK_DENOMINATOR * (16 - u64::from(new_rate));
        self.adpcm_clock_accumulator = ((u128::from(self.adpcm_clock_accumulator)
            * u128::from(new_threshold))
            / u128::from(old_threshold)) as u64;
        self.adpcm_playback_rate = new_rate;
    }

    pub(super) fn reload_adpcm_length_if_held(&mut self) {
        if self.adpcm_address_control & 0x10 != 0 {
            self.reload_adpcm_length();
        }
    }

    fn reload_adpcm_length(&mut self) {
        self.adpcm_length = if self.adpcm_address_latch == 0 {
            0x1_0000
        } else {
            u32::from(self.adpcm_address_latch)
        };
        self.adpcm_end_irq = false;
        self.adpcm_half_irq = false;
    }

    pub(super) fn complete_adpcm_write(&mut self, value: u8) {
        self.adpcm_half_irq = self.adpcm_length < 0x8000;
        if self.adpcm_address_control & 0x10 == 0 && self.adpcm_length < 0xFFFF {
            self.adpcm_length += 1;
        }
        self.adpcm_ram[usize::from(self.adpcm_write_address)] = value;
        self.adpcm_write_address = self.adpcm_write_address.wrapping_add(1);
    }

    pub(super) fn current_adpcm_output_level(&self) -> i32 {
        if self.adpcm_playing {
            let level =
                (i32::from(self.adpcm_predictor & 0x0FFC) - 0x0800) * ADPCM_BLIP_LEVEL_SCALE;
            self.apply_fade(CdFadeTarget::Adpcm, level)
        } else {
            0
        }
    }

    fn apply_fade(&self, target: CdFadeTarget, level: i32) -> i32 {
        if self.audio_fade_target == Some(target) {
            ((i64::from(level) * i64::from(self.audio_fade_level_q16)) / 0x1_0000) as i32
        } else {
            level
        }
    }

    fn cdda_fade_gain(&self) -> f32 {
        if self.audio_fade_target == Some(CdFadeTarget::Cdda) {
            self.audio_fade_level_q16 as f32 / 65_536.0
        } else {
            1.0
        }
    }

    fn advance_adpcm_resampler(&mut self, clocks: u64) {
        if self.audio_sample_generation_enabled {
            self.adpcm_resampler
                .advance_clocks(clocks, &mut self.adpcm_audio_samples);
        }
    }

    fn refresh_adpcm_output(&mut self) {
        if self.audio_sample_generation_enabled {
            self.adpcm_resampler
                .refresh_level(self.current_adpcm_output_level());
        }
    }

    pub(super) fn advance_cdda(&mut self, ticks: u64) {
        if matches!(
            self.audio_status,
            CdAudioStatus::Inactive | CdAudioStatus::Stopped
        ) {
            return;
        }
        self.audio_tick_accumulator = self
            .audio_tick_accumulator
            .saturating_add(ticks.saturating_mul(CDDA_TICK_NUMERATOR));
        while self.audio_tick_accumulator >= CDDA_TICK_DENOMINATOR {
            self.audio_tick_accumulator -= CDDA_TICK_DENOMINATOR;
            let sample = if self.audio_status == CdAudioStatus::Playing {
                self.read_current_audio_sample()
            } else {
                (0, 0)
            };
            self.audio_left_sample = sample.0;
            self.audio_right_sample = sample.1;
            if self.audio_sample_generation_enabled {
                let gain = self.cdda_fade_gain();
                self.audio_source_frames.push_back([
                    f32::from(sample.0) / 32_768.0 * gain,
                    f32::from(sample.1) / 32_768.0 * gain,
                ]);
            }
            if self.audio_status == CdAudioStatus::Playing {
                self.audio_current_sample += 1;
                if self.audio_current_sample == 588 {
                    self.audio_current_sample = 0;
                    self.audio_current_lba = self.audio_current_lba.saturating_add(1);
                    if self.audio_current_lba >= self.audio_end_lba {
                        match self.audio_end_behavior {
                            CdAudioEndBehavior::Stop => {
                                self.stop_audio(CdAudioStatus::Stopped);
                            }
                            CdAudioEndBehavior::Loop => {
                                self.audio_current_lba = self.audio_start_lba;
                            }
                            CdAudioEndBehavior::SignalCompletion => {
                                self.stop_audio(CdAudioStatus::Stopped);
                                self.status = 0;
                                self.enter_status_after(0);
                            }
                        }
                    }
                }
            }
        }
    }

    pub(super) fn stop_audio(&mut self, status: CdAudioStatus) {
        self.audio_status = status;
        self.audio_track_index = None;
        self.audio_left_sample = 0;
        self.audio_right_sample = 0;
        self.audio_source_frames.clear();
        self.audio_resample_position = 0.0;
    }

    fn read_current_audio_sample(&mut self) -> (i16, i16) {
        let lba = self.audio_current_lba;
        let sample = self.audio_current_sample;
        if let Some(track_index) = self.audio_track_index
            && let Some(result) =
                self.disc
                    .read_audio_sample_from_track_index(track_index, lba, sample)
        {
            return result.unwrap_or((0, 0));
        }

        self.audio_track_index = self.disc.stored_track_index_at_lba(lba);
        self.audio_track_index
            .and_then(|track_index| {
                self.disc
                    .read_audio_sample_from_track_index(track_index, lba, sample)
            })
            .and_then(Result::ok)
            .unwrap_or((0, 0))
    }

    pub(super) fn service_adpcm_dma(&mut self) {
        if self.adpcm_dma_control & 2 == 0 || self.phase != CdScsiPhase::DataIn {
            return;
        }

        let response_index_before = self.response_index;
        while self.response_index < self.response_available {
            self.complete_adpcm_write(self.response[self.response_index]);
            self.response_index += 1;
        }
        self.request = false;
        self.acknowledge = false;
        self.auto_acknowledge = false;
        self.acknowledged_request = false;
        self.request_pending = false;

        if self.response_index == self.response.len() {
            self.debug_adpcm_dma_completed |= response_index_before < self.response_index;
            self.enter_status_after(PROVISIONAL_CDROM2_PHASE_TICKS);
        }
    }
}

const fn adpcm_blip_buffer_samples(sample_rate: u32) -> u32 {
    let numerator =
        ADPCM_BLIP_FRAME_CLOCKS as u64 * sample_rate as u64 * CDROM2_MASTER_CLOCK_DENOMINATOR;
    let samples =
        numerator.div_ceil(CDROM2_MASTER_CLOCK_NUMERATOR) as u32 + ADPCM_BLIP_BUFFER_MARGIN;
    if samples < ADPCM_BLIP_BUFFER_MIN_SAMPLES {
        ADPCM_BLIP_BUFFER_MIN_SAMPLES
    } else {
        samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{CD_USER_SECTOR_BYTES, CdTrack, CdTrackMode};

    fn cdrom() -> CdRom2 {
        let track = CdTrack::from_index1_data(
            1,
            4,
            None,
            0,
            CdTrackMode::Mode1_2048,
            vec![0; CD_USER_SECTOR_BYTES],
        )
        .unwrap();
        CdRom2::new(CdDisc::new(vec![track]).unwrap())
    }

    fn audio_cdrom() -> CdRom2 {
        let mut raw = vec![0; 2_352];
        for frame in raw.as_chunks_mut::<4>().0 {
            frame[..2].copy_from_slice(&0x4000_i16.to_le_bytes());
            frame[2..].copy_from_slice(&(-0x4000_i16).to_le_bytes());
        }
        let track = CdTrack::from_index1_data(1, 0, None, 0, CdTrackMode::Audio, raw).unwrap();
        CdRom2::new(CdDisc::new(vec![track]).unwrap())
    }

    fn two_track_audio_cdrom() -> CdRom2 {
        let track = |number: u8, lba: u32, sample: i16| {
            let mut raw = vec![0; 2_352];
            for frame in raw.as_chunks_mut::<4>().0 {
                frame[..2].copy_from_slice(&sample.to_le_bytes());
                frame[2..].copy_from_slice(&(-sample).to_le_bytes());
            }
            CdTrack::from_index1_data(number, 0, None, lba, CdTrackMode::Audio, raw).unwrap()
        };
        CdRom2::new(CdDisc::new(vec![track(1, 0, 0x1111), track(2, 1, 0x2222)]).unwrap())
    }

    fn start(cd: &mut CdRom2, byte: u8, rate: u8) {
        cd.adpcm_ram[0] = byte;
        cd.adpcm_address_latch = 1;
        cd.write_adpcm_address_control(0x10);
        cd.write_adpcm_playback_rate(rate);
        cd.write_adpcm_address_control(0x60);
    }

    fn ticks_to_next_nibble(cd: &CdRom2) -> u64 {
        let threshold = ADPCM_CLOCK_DENOMINATOR * (16 - u64::from(cd.adpcm_playback_rate));
        (threshold - cd.adpcm_clock_accumulator).div_ceil(ADPCM_CLOCK_NUMERATOR)
    }

    fn decoded_nibbles(cd: &CdRom2) -> u64 {
        u64::from(cd.adpcm_read_address) * 2 + u64::from(!cd.adpcm_high_nibble_next)
    }

    #[test]
    fn msm5205_decodes_high_nibble_first_and_wraps_unsigned_predictor() {
        let mut cd = cdrom();
        start(&mut cd, 0x7F, 15);
        let first = ticks_to_next_nibble(&cd);
        cd.advance_adpcm_playback(first - 1);
        assert_eq!(cd.adpcm_predictor, ADPCM_RESET_PREDICTOR);
        cd.advance_adpcm_playback(1);
        assert_eq!(cd.adpcm_predictor, 2_078);
        assert_eq!(cd.adpcm_step_index, 8);
        assert!(!cd.adpcm_high_nibble_next);
        assert_eq!(cd.adpcm_read_address, 0);

        cd.advance_adpcm_playback(ticks_to_next_nibble(&cd));
        assert_eq!(cd.adpcm_predictor, 2_015);
        assert_eq!(cd.adpcm_step_index, 16);
        assert_eq!(cd.adpcm_read_address, 1);
        assert!(cd.adpcm_playing);
        assert!(cd.adpcm_stop_pending);
        cd.advance_adpcm_playback(ticks_to_next_nibble(&cd));
        assert!(!cd.adpcm_playing);

        cd.adpcm_predictor = 0x0FFF;
        cd.adpcm_step_index = 0;
        cd.adpcm_read_address = 0;
        cd.adpcm_high_nibble_next = true;
        cd.adpcm_playing = true;
        cd.adpcm_address_control = 0x20;
        cd.decode_adpcm_nibble();
        assert_eq!(cd.adpcm_predictor, 29);
        assert_eq!(cd.current_adpcm_output_level(), -32_320);
    }

    #[test]
    fn dominant_reset_never_starts_and_next_play_write_resets_codec() {
        for reset in [0x80, 0xA0, 0xE0] {
            let mut cd = cdrom();
            start(&mut cd, 0x70, 15);
            cd.advance_adpcm_playback(ticks_to_next_nibble(&cd));
            assert_ne!(cd.adpcm_predictor, ADPCM_RESET_PREDICTOR);
            cd.adpcm_address_latch = 0x1234;
            cd.adpcm_read_buffer = 0x56;
            cd.write_adpcm_address_control(reset);
            assert!(!cd.adpcm_playing);
            assert_eq!(cd.adpcm_address_control, 0);
            assert_eq!(cd.adpcm_address_latch, 0);
            assert_eq!(cd.adpcm_read_buffer, 0);
            assert_eq!(cd.adpcm_predictor, ADPCM_RESET_PREDICTOR);
            assert_eq!(cd.adpcm_resampler.level, 0);

            cd.write_adpcm_address_control(0x20);
            assert!(cd.adpcm_playing);
            assert_eq!(cd.adpcm_predictor, ADPCM_RESET_PREDICTOR);
            assert!(cd.adpcm_high_nibble_next);
        }
    }

    #[test]
    fn held_length_reload_tracks_both_latch_bytes_and_suppresses_decrement() {
        let mut cd = cdrom();
        cd.write_adpcm_address_control(0x10);
        cd.adpcm_address_latch = 0x1200;
        cd.reload_adpcm_length_if_held();
        assert_eq!(cd.adpcm_length, 0x1200);
        cd.adpcm_address_latch = 0x1234;
        cd.reload_adpcm_length_if_held();
        assert_eq!(cd.adpcm_length, 0x1234);

        cd.adpcm_ram[0] = 0;
        cd.write_adpcm_address_control(0x30);
        let before = cd.adpcm_length;
        cd.advance_adpcm_playback(ticks_to_next_nibble(&cd));
        cd.advance_adpcm_playback(ticks_to_next_nibble(&cd));
        assert_eq!(cd.adpcm_read_address, 1);
        assert_eq!(cd.adpcm_length, before);
    }

    #[test]
    fn live_rate_write_scales_fractional_phase_without_resetting_codec() {
        let mut cd = cdrom();
        start(&mut cd, 0x77, 0);
        cd.advance_adpcm_playback(4_000);
        let old_accumulator = cd.adpcm_clock_accumulator;
        let old_threshold = ADPCM_CLOCK_DENOMINATOR * 16;
        let predictor = cd.adpcm_predictor;
        cd.write_adpcm_playback_rate(15);
        let expected = (u128::from(old_accumulator) * u128::from(ADPCM_CLOCK_DENOMINATOR)
            / u128::from(old_threshold)) as u64;
        assert_eq!(cd.adpcm_clock_accumulator, expected);
        assert_eq!(cd.adpcm_predictor, predictor);
        assert!(cd.adpcm_clock_accumulator < ADPCM_CLOCK_DENOMINATOR);
        cd.advance_adpcm_playback(ticks_to_next_nibble(&cd));
        assert_ne!(cd.adpcm_predictor, predictor);
    }

    #[test]
    fn rational_cadence_is_exact_and_chunk_equivalent_at_every_rate() {
        for rate in 0..=15 {
            let mut whole = cdrom();
            let mut split = cdrom();
            start(&mut whole, 0x71, rate);
            start(&mut split, 0x71, rate);
            whole.adpcm_address_control &= !0x40;
            split.adpcm_address_control &= !0x40;
            whole.adpcm_length = 0x100;
            split.adpcm_length = 0x100;
            let ticks = 80_000;
            whole.advance_adpcm_playback(ticks);
            for chunk in [1, 17, 509, 4_003, 12_771, 62_699] {
                split.advance_adpcm_playback(chunk);
            }
            assert_eq!(whole.adpcm_predictor, split.adpcm_predictor, "rate {rate}");
            assert_eq!(
                whole.adpcm_step_index, split.adpcm_step_index,
                "rate {rate}"
            );
            assert_eq!(
                whole.adpcm_read_address, split.adpcm_read_address,
                "rate {rate}"
            );
            assert_eq!(
                whole.adpcm_clock_accumulator, split.adpcm_clock_accumulator,
                "rate {rate}"
            );
        }
    }

    #[test]
    fn every_rate_hits_each_absolute_nibble_boundary_exactly() {
        for rate in 0..=15 {
            let threshold = ADPCM_CLOCK_DENOMINATOR * (16 - u64::from(rate));
            let target_nibbles = 73;
            let boundary_ticks = (target_nibbles * threshold).div_ceil(ADPCM_CLOCK_NUMERATOR);
            let mut cd = cdrom();
            start(&mut cd, 0x11, rate);
            cd.adpcm_address_control &= !0x40;
            cd.adpcm_length = 0x1_000;
            cd.advance_adpcm_playback(boundary_ticks - 1);
            assert_eq!(decoded_nibbles(&cd), target_nibbles - 1, "rate {rate}");
            cd.advance_adpcm_playback(1);
            assert_eq!(decoded_nibbles(&cd), target_nibbles, "rate {rate}");

            let total_ticks = 1_000_000;
            let mut count = cdrom();
            start(&mut count, 0x11, rate);
            count.adpcm_address_control &= !0x40;
            count.adpcm_length = 0x1_000;
            count.advance_adpcm_playback(total_ticks);
            assert_eq!(
                decoded_nibbles(&count),
                total_ticks * ADPCM_CLOCK_NUMERATOR / threshold,
                "rate {rate}"
            );
        }
    }

    #[test]
    fn host_frame_counts_and_pcm_are_exact_across_chunking() {
        let master_ticks = 1_000_000_u64;
        for sample_rate in [44_100, 48_000, 96_000, 192_000] {
            let mut whole = cdrom();
            let mut split = cdrom();
            whole.set_sample_rate(sample_rate);
            split.set_sample_rate(sample_rate);
            start(&mut whole, 0x71, 13);
            start(&mut split, 0x71, 13);
            whole.adpcm_address_control &= !0x40;
            split.adpcm_address_control &= !0x40;
            whole.adpcm_length = 0x1_000;
            split.adpcm_length = 0x1_000;
            whole.advance_adpcm_playback(master_ticks);
            for chunk in [3, 65_534, 1, 200_003, 17, 734_442] {
                split.advance_adpcm_playback(chunk);
            }
            whole.adpcm_resampler.flush(&mut whole.adpcm_audio_samples);
            split.adpcm_resampler.flush(&mut split.adpcm_audio_samples);
            let expected = (u128::from(master_ticks)
                * u128::from(sample_rate)
                * u128::from(CDROM2_MASTER_CLOCK_DENOMINATOR)
                / u128::from(CDROM2_MASTER_CLOCK_NUMERATOR)) as usize;
            assert_eq!(whole.adpcm_audio_samples.len(), expected);
            assert_eq!(split.adpcm_audio_samples, whole.adpcm_audio_samples);
        }
    }

    #[test]
    fn rate_change_and_generation_resume_emit_no_synthetic_edge() {
        let mut cd = cdrom();
        cd.set_sample_rate(48_000);
        start(&mut cd, 0x77, 0);
        cd.adpcm_address_control &= !0x40;
        cd.adpcm_length = 0x100;
        cd.advance_adpcm_playback(ticks_to_next_nibble(&cd));
        assert_ne!(cd.current_adpcm_output_level(), 0);

        cd.set_sample_rate(96_000);
        let host_sample_clocks =
            CDROM2_MASTER_CLOCK_NUMERATOR.div_ceil(CDROM2_MASTER_CLOCK_DENOMINATOR * 96_000) * 16;
        cd.advance_adpcm_playback(ticks_to_next_nibble(&cd) - 1);
        cd.adpcm_resampler.flush(&mut cd.adpcm_audio_samples);
        assert!(cd.adpcm_audio_samples.iter().all(|sample| *sample == 0));
        cd.adpcm_audio_samples.clear();
        cd.advance_adpcm_playback(1);
        cd.advance_adpcm_playback(host_sample_clocks);
        cd.adpcm_resampler.flush(&mut cd.adpcm_audio_samples);
        assert!(cd.adpcm_audio_samples.iter().any(|sample| *sample != 0));

        cd.set_sample_generation_enabled(false);
        cd.advance_adpcm_playback(ticks_to_next_nibble(&cd));
        cd.set_sample_generation_enabled(true);
        cd.advance_adpcm_playback(ticks_to_next_nibble(&cd) - 1);
        cd.adpcm_resampler.flush(&mut cd.adpcm_audio_samples);
        assert!(cd.adpcm_audio_samples.iter().all(|sample| *sample == 0));
        cd.adpcm_audio_samples.clear();
        cd.advance_adpcm_playback(1);
        cd.advance_adpcm_playback(host_sample_clocks);
        cd.adpcm_resampler.flush(&mut cd.adpcm_audio_samples);
        assert!(cd.adpcm_audio_samples.iter().any(|sample| *sample != 0));
    }

    #[test]
    fn stop_waits_one_boundary_without_consuming_another_nibble() {
        let mut terminal = cdrom();
        start(&mut terminal, 0x77, 15);
        terminal.advance_adpcm_playback(ticks_to_next_nibble(&terminal));
        terminal.advance_adpcm_playback(ticks_to_next_nibble(&terminal));
        let final_level = terminal.adpcm_resampler.level;
        assert_ne!(final_level, 0);
        assert_eq!(terminal.adpcm_read_address, 1);
        assert!(terminal.adpcm_stop_pending);
        terminal.advance_adpcm_playback(ticks_to_next_nibble(&terminal) - 1);
        assert_eq!(terminal.adpcm_resampler.level, final_level);
        terminal.advance_adpcm_playback(1);
        assert!(!terminal.adpcm_playing);
        assert_eq!(terminal.adpcm_read_address, 1);
        assert_eq!(terminal.adpcm_resampler.level, 0);
        terminal.write_adpcm_address_control(0x60);
        assert!(!terminal.adpcm_playing);
        terminal.write_adpcm_address_control(0x20);
        assert!(terminal.adpcm_playing);
        assert_eq!(terminal.adpcm_predictor, ADPCM_RESET_PREDICTOR);

        let mut explicit = cdrom();
        start(&mut explicit, 0x77, 15);
        explicit.adpcm_address_control &= !0x40;
        explicit.adpcm_length = 0x100;
        explicit.advance_adpcm_playback(ticks_to_next_nibble(&explicit));
        let address = explicit.adpcm_read_address;
        let high_next = explicit.adpcm_high_nibble_next;
        let level = explicit.adpcm_resampler.level;
        explicit.write_adpcm_address_control(0);
        assert_eq!(explicit.adpcm_resampler.level, level);
        explicit.advance_adpcm_playback(ticks_to_next_nibble(&explicit));
        assert!(!explicit.adpcm_playing);
        assert_eq!(explicit.adpcm_read_address, address);
        assert_eq!(explicit.adpcm_high_nibble_next, high_next);
        assert_eq!(explicit.adpcm_resampler.level, 0);
    }

    #[test]
    fn psg_cdda_and_adpcm_sum_before_a_single_final_clamp() {
        let mut cancelling = cdrom();
        cancelling.audio_source_frames.push_back([0.8, 0.8]);
        cancelling.adpcm_audio_samples.push(-19_660);
        let mut output = [0.9, 0.9];
        cancelling.mix_audio_samples_into(&mut output);
        assert_eq!(output, [1.0, 1.0]);

        let mut over = cdrom();
        over.audio_source_frames.push_back([0.8, 0.8]);
        over.adpcm_audio_samples.push(13_107);
        let mut output = [0.8, 0.8];
        over.mix_audio_samples_into(&mut output);
        assert_eq!(output, [1.0, 1.0]);
    }

    #[test]
    fn inactive_output_does_not_skip_the_start_of_later_cdda() {
        let mut cd = cdrom();
        let mut silence = [0.0; 256];
        cd.mix_audio_samples_into(&mut silence);
        assert_eq!(cd.audio_resample_position, 0.0);

        cd.audio_source_frames.push_back([0.25, -0.25]);
        cd.audio_source_frames.push_back([0.75, -0.75]);
        let mut output = [0.0; 2];
        cd.mix_audio_samples_into(&mut output);
        assert_eq!(output, [0.125, -0.125]);
    }

    #[test]
    fn adpcm_writes_extend_active_stream_length_unless_reload_is_held() {
        let mut cd = cdrom();
        cd.adpcm_length = 0x7FFF;
        cd.complete_adpcm_write(0x12);
        assert_eq!(cd.adpcm_length, 0x8000);
        assert!(cd.adpcm_half_irq);
        assert_eq!(cd.adpcm_ram[0], 0x12);

        cd.complete_adpcm_write(0x34);
        assert_eq!(cd.adpcm_length, 0x8001);
        assert!(!cd.adpcm_half_irq);
        assert_eq!(cd.adpcm_ram[1], 0x34);

        cd.adpcm_address_control = 0x10;
        cd.complete_adpcm_write(0x56);
        assert_eq!(cd.adpcm_length, 0x8001);
        assert_eq!(cd.adpcm_ram[2], 0x56);

        cd.adpcm_address_control = 0;
        cd.adpcm_length = 0xFFFF;
        cd.complete_adpcm_write(0x78);
        assert_eq!(cd.adpcm_length, 0xFFFF);
    }

    #[test]
    fn mono_blip_output_and_reconfiguration_are_click_safe_at_supported_rates() {
        for sample_rate in [44_100, 48_000, 96_000, 192_000] {
            let mut cd = cdrom();
            cd.set_sample_rate(sample_rate);
            start(&mut cd, 0x77, 15);
            cd.adpcm_address_control &= !0x40;
            cd.adpcm_length = 0x100;
            cd.advance_adpcm_playback(200_000);
            let current_level = cd.current_adpcm_output_level();
            cd.set_sample_rate(sample_rate);
            assert_eq!(cd.adpcm_resampler.level, current_level);
            assert!(cd.adpcm_audio_samples.is_empty());
            cd.set_sample_generation_enabled(false);
            cd.advance_adpcm_playback(100_000);
            let disabled_level = cd.current_adpcm_output_level();
            cd.set_sample_generation_enabled(true);
            assert_eq!(cd.adpcm_resampler.level, disabled_level);
            cd.advance_adpcm_playback(200_000);
            let frames = (400_000_u64 * u64::from(sample_rate)
                / (CDROM2_MASTER_CLOCK_NUMERATOR / CDROM2_MASTER_CLOCK_DENOMINATOR))
                as usize
                + 16;
            let mut output = vec![0.0; frames * 2];
            cd.mix_audio_samples_into(&mut output);
            assert!(output.iter().any(|sample| *sample != 0.0));
            assert!(
                output
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .all(|frame| frame[0] == frame[1])
            );
            assert!(output.iter().all(|sample| sample.is_finite()));
        }
    }

    #[test]
    fn every_fade_command_latches_and_decodes_only_its_function_bits() {
        for value in 0..=u8::MAX {
            let mut cd = cdrom();
            cd.write_audio_fade_control(value);
            assert_eq!(cd.adpcm_fade_control, value);
            if value & 8 == 0 {
                assert_eq!(cd.audio_fade_target, None);
                assert_eq!(cd.audio_fade_level_q16, 0x1_0000);
                assert_eq!(cd.audio_fade_step_ticks, 0);
                assert_eq!(cd.audio_fade_ticks_to_next, 0);
                continue;
            }
            assert_eq!(
                cd.audio_fade_target,
                Some(if value & 2 == 0 {
                    CdFadeTarget::Cdda
                } else {
                    CdFadeTarget::Adpcm
                })
            );
            let expected_period = if value & 4 == 0 {
                PROVISIONAL_CDROM2_FADE_LONG_STEP_TICKS
            } else {
                PROVISIONAL_CDROM2_FADE_SHORT_STEP_TICKS
            };
            assert_eq!(cd.audio_fade_step_ticks, expected_period);
            assert_eq!(cd.audio_fade_ticks_to_next, expected_period);
            assert_eq!(cd.audio_fade_level_q16, 0x1_0000);
        }
    }

    #[test]
    fn fade_register_round_trips_and_reset_cancels_the_envelope() {
        let mut cd = cdrom();
        assert!(cd.write_physical(CDROM2_REGISTER_START + 15, 0xCF));
        assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 15), Some(0xCF));
        cd.advance_master_ticks(PROVISIONAL_CDROM2_FADE_SHORT_STEP_TICKS);
        assert_eq!(cd.audio_fade_level_q16, 0xFFFF);

        cd.reset();
        assert_eq!(cd.read_physical(CDROM2_REGISTER_START + 15), Some(0));
        assert_eq!(cd.audio_fade_target, None);
        assert_eq!(cd.audio_fade_level_q16, 0x1_0000);
        assert_eq!(cd.audio_fade_ticks_to_next, 0);
    }

    #[test]
    fn fade_repeat_retarget_speed_and_cancel_preserve_the_defined_state() {
        let mut cd = cdrom();
        cd.write_audio_fade_control(0x08);
        cd.advance_audio(PROVISIONAL_CDROM2_FADE_LONG_STEP_TICKS + 317);
        assert_eq!(cd.audio_fade_level_q16, 0xFFFF);
        let remaining = cd.audio_fade_ticks_to_next;

        cd.write_audio_fade_control(0x89);
        assert_eq!(cd.audio_fade_target, Some(CdFadeTarget::Cdda));
        assert_eq!(cd.audio_fade_ticks_to_next, remaining);
        assert_eq!(cd.audio_fade_level_q16, 0xFFFF);

        cd.write_audio_fade_control(0x0E);
        assert_eq!(cd.audio_fade_target, Some(CdFadeTarget::Adpcm));
        assert_eq!(
            cd.audio_fade_step_ticks,
            PROVISIONAL_CDROM2_FADE_SHORT_STEP_TICKS
        );
        assert_eq!(cd.audio_fade_ticks_to_next, remaining);
        assert_eq!(cd.cdda_fade_gain(), 1.0);
        cd.advance_audio(remaining);
        assert_eq!(cd.audio_fade_level_q16, 0xFFFE);
        assert_eq!(
            cd.audio_fade_ticks_to_next,
            PROVISIONAL_CDROM2_FADE_SHORT_STEP_TICKS
        );

        cd.write_audio_fade_control(0x47);
        assert_eq!(cd.adpcm_fade_control, 0x47);
        assert_eq!(cd.audio_fade_target, None);
        assert_eq!(cd.audio_fade_level_q16, 0x1_0000);
        assert_eq!(cd.audio_fade_ticks_to_next, 0);
    }

    #[test]
    fn fade_timing_is_exact_chunk_equivalent_and_reaches_zero() {
        let mut whole = cdrom();
        let mut split = cdrom();
        whole.write_audio_fade_control(0x0C);
        split.write_audio_fade_control(0x0C);
        let total = PROVISIONAL_CDROM2_FADE_SHORT_STEP_TICKS * 73 + 411;
        whole.advance_audio(total);
        for ticks in [1, 818, 17, 9_001, 37_111, 12_839, 411] {
            split.advance_audio(ticks);
        }
        assert_eq!(split.audio_fade_level_q16, whole.audio_fade_level_q16);
        assert_eq!(
            split.audio_fade_ticks_to_next,
            whole.audio_fade_ticks_to_next
        );
        assert_eq!(split.audio_tick_accumulator, whole.audio_tick_accumulator);
        assert_eq!(split.adpcm_clock_accumulator, whole.adpcm_clock_accumulator);

        whole.advance_audio(
            PROVISIONAL_CDROM2_FADE_SHORT_STEP_TICKS * u64::from(whole.audio_fade_level_q16),
        );
        assert_eq!(whole.audio_fade_level_q16, 0);
        assert_eq!(whole.audio_fade_ticks_to_next, 0);

        let mut long = cdrom();
        long.write_audio_fade_control(0x08);
        long.advance_audio(PROVISIONAL_CDROM2_FADE_LONG_STEP_TICKS * 0x1_0000_u64 - 1);
        assert_eq!(long.audio_fade_level_q16, 1);
        assert_eq!(long.audio_fade_ticks_to_next, 1);
        long.advance_audio(1);
        assert_eq!(long.audio_fade_level_q16, 0);
        assert_eq!(long.audio_fade_ticks_to_next, 0);
    }

    #[test]
    fn cdda_fade_is_baked_into_each_source_frame_without_rewriting_history() {
        let mut cd = audio_cdrom();
        cd.audio_status = CdAudioStatus::Playing;
        cd.audio_end_lba = 1;
        cd.audio_fade_target = Some(CdFadeTarget::Cdda);
        cd.audio_fade_level_q16 = 0x8000;
        cd.audio_fade_step_ticks = PROVISIONAL_CDROM2_FADE_LONG_STEP_TICKS;
        cd.audio_fade_ticks_to_next = PROVISIONAL_CDROM2_FADE_LONG_STEP_TICKS;
        cd.advance_cdda(488);
        assert_eq!(cd.audio_source_frames.front(), Some(&[0.25, -0.25]));

        cd.write_audio_fade_control(0);
        cd.advance_cdda(488);
        assert_eq!(cd.audio_source_frames.front(), Some(&[0.25, -0.25]));
        assert_eq!(cd.audio_source_frames.get(1), Some(&[0.5, -0.5]));
    }

    #[test]
    fn cdda_track_cache_revalidates_at_track_boundaries_and_is_transient() {
        let mut cd = two_track_audio_cdrom();
        cd.audio_status = CdAudioStatus::Playing;
        cd.audio_end_lba = 2;
        cd.audio_current_sample = 587;
        cd.audio_track_index = Some(0);

        cd.advance_cdda(488);
        assert_eq!(cd.audio_left_sample, 0x1111);
        assert_eq!((cd.audio_current_lba, cd.audio_current_sample), (1, 0));
        assert_eq!(cd.audio_track_index, Some(0));

        cd.advance_cdda(488);
        assert_eq!(cd.audio_left_sample, 0x2222);
        assert_eq!(cd.audio_track_index, Some(1));

        cd.stop_audio(CdAudioStatus::Stopped);
        assert_eq!(cd.audio_track_index, None);
        cd.audio_track_index = Some(1);
        cd.reset();
        assert_eq!(cd.audio_track_index, None);
    }

    #[test]
    fn cdda_source_frames_straddle_a_real_fade_boundary() {
        let mut cd = audio_cdrom();
        cd.audio_status = CdAudioStatus::Playing;
        cd.audio_end_lba = 1;
        cd.write_audio_fade_control(0x0C);

        cd.advance_audio(PROVISIONAL_CDROM2_FADE_SHORT_STEP_TICKS - 1);
        assert_eq!(cd.audio_source_frames.front(), Some(&[0.5, -0.5]));
        cd.advance_audio(1);
        assert_eq!(cd.audio_fade_level_q16, 0xFFFF);
        cd.advance_audio(156);
        assert_eq!(cd.audio_source_frames.len(), 2);
        let gain = 65_535.0 / 65_536.0;
        assert_eq!(
            cd.audio_source_frames.get(1),
            Some(&[0.5 * gain, -0.5 * gain])
        );
    }

    #[test]
    fn adpcm_fade_edges_refresh_at_the_current_resampler_clock() {
        let mut cd = cdrom();
        cd.adpcm_playback_rate = 0;
        cd.adpcm_playing = true;
        cd.adpcm_predictor = 0x0FFC;
        cd.refresh_adpcm_output();
        let full = cd.adpcm_resampler.level;
        cd.write_audio_fade_control(0x0A);
        cd.advance_audio(PROVISIONAL_CDROM2_FADE_LONG_STEP_TICKS - 1);
        assert_eq!(cd.adpcm_resampler.level, full);
        let clocks = cd.adpcm_resampler.clocks;
        cd.advance_audio(1);
        assert_eq!(cd.adpcm_resampler.clocks, clocks + 1);
        assert!(cd.adpcm_resampler.level < full);

        let faded = cd.adpcm_resampler.level;
        let clocks = cd.adpcm_resampler.clocks;
        cd.write_audio_fade_control(0);
        assert_eq!(cd.adpcm_resampler.clocks, clocks);
        assert_eq!(cd.adpcm_resampler.level, full);
        assert_ne!(faded, full);
    }

    #[test]
    fn disabled_generation_advances_fade_and_reconfiguration_seeds_faded_level() {
        let mut cd = cdrom();
        cd.adpcm_playback_rate = 0;
        cd.adpcm_playing = true;
        cd.adpcm_predictor = 0x0FFC;
        cd.write_audio_fade_control(0x0E);
        cd.set_sample_generation_enabled(false);
        cd.advance_audio(PROVISIONAL_CDROM2_FADE_SHORT_STEP_TICKS * 19);
        assert_eq!(cd.audio_fade_level_q16, 0x1_0000 - 19);
        let faded = cd.current_adpcm_output_level();

        cd.set_sample_generation_enabled(true);
        assert_eq!(cd.adpcm_resampler.level, faded);
        cd.set_sample_rate(96_000);
        assert_eq!(cd.adpcm_resampler.level, faded);
        assert!(cd.adpcm_audio_samples.is_empty());
    }
}
