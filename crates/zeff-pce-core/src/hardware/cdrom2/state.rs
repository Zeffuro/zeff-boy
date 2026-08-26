use super::*;
use anyhow::bail;
use zeff_emu_common::save_state::{StateReader, StateWriter};

pub(crate) const MAX_CDROM2_STATE_SECTION_BYTES: usize = 2 * 1024 * 1024;

const MAX_COMMAND_BYTES: usize = 10;
const MAX_RESPONSE_BYTES: usize = 256 * CD_USER_SECTOR_BYTES;
const MAX_CDDA_SOURCE_FRAMES: usize = CDDA_SOURCE_SAMPLE_RATE_HZ as usize;
const MAX_ADPCM_AUDIO_SAMPLES: usize = super::super::psg::MAX_PSG_SAMPLE_RATE as usize;

impl CdRom2 {
    pub(crate) fn validate_v1_state(&self) -> anyhow::Result<()> {
        if self.command.len() > MAX_COMMAND_BYTES {
            bail!("CD command exceeds save-state bound");
        }
        if self.response.len() > MAX_RESPONSE_BYTES {
            bail!("CD response exceeds save-state bound");
        }
        if self.audio_source_frames.len() > MAX_CDDA_SOURCE_FRAMES {
            bail!("CDDA source queue exceeds save-state bound");
        }
        if self.adpcm_audio_samples.len() > MAX_ADPCM_AUDIO_SAMPLES {
            bail!("ADPCM audio queue exceeds save-state bound");
        }
        Ok(())
    }

    pub(crate) fn runtime_audio_config(&self) -> (u32, bool) {
        (self.audio_sample_rate, self.audio_sample_generation_enabled)
    }

    pub(crate) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.work_ram[..]);
        writer.write_bytes(&self.bram[..]);
        writer.write_bytes(&self.adpcm_ram[..]);
        writer.write_bool(self.bram_unlocked);
        writer.write_bool(self.reset_asserted);
        writer.write_u8(phase_to_tag(self.phase));
        writer.write_bool(self.request);
        writer.write_bool(self.acknowledge);
        writer.write_bool(self.auto_acknowledge);
        writer.write_bool(self.acknowledged_request);
        writer.write_bool(self.request_pending);
        writer.write_bool(self.data_ready_irq_enabled);
        writer.write_bool(self.status_irq_enabled);
        writer.write_bool(self.audio_end_irq_enabled);
        writer.write_bool(self.audio_half_irq_enabled);
        writer.write_u8(self.output_latch);
        writer.write_vec(&self.command);
        writer.write_vec(&self.response);
        writer.write_u32(self.response_index as u32);
        writer.write_u32(self.response_available as u32);
        writer.write_u8(self.status);
        writer.write_u8(self.sense_key);
        writer.write_u8(self.additional_sense_code);
        write_event(writer, self.event);
        write_optional_u64(writer, self.sector_arrival_ticks);
        writer.write_u16(self.sectors_pending);
        writer.write_u64(self.sector_tick_remainder);

        writer.write_u8(audio_status_to_tag(self.audio_status));
        writer.write_u32(self.audio_start_lba);
        writer.write_u32(self.audio_end_lba);
        writer.write_u32(self.audio_current_lba);
        writer.write_u16(self.audio_current_sample as u16);
        writer.write_u8(end_behavior_to_tag(self.audio_end_behavior));
        writer.write_u64(self.audio_tick_accumulator);
        writer.write_u16(self.audio_left_sample as u16);
        writer.write_u16(self.audio_right_sample as u16);
        writer.write_u16(self.audio_sample_latch as u16);
        writer.write_bool(self.audio_sample_latch_right);
        writer.write_u64(self.audio_sample_latch_clock);
        writer.write_u64(self.audio_sample_latch_last_clock);
        writer.write_u32(self.audio_source_frames.len() as u32);
        for frame in &self.audio_source_frames {
            writer.write_u32(frame[0].to_bits());
            writer.write_u32(frame[1].to_bits());
        }
        writer.write_f64(self.audio_resample_position);
        writer.write_u32(self.audio_sample_rate);
        writer.write_bool(self.audio_sample_generation_enabled);

        writer.write_u16(self.adpcm_address_latch);
        writer.write_u16(self.adpcm_read_address);
        writer.write_u16(self.adpcm_write_address);
        writer.write_u8(self.adpcm_read_buffer);
        writer.write_u8(self.adpcm_dma_control);
        writer.write_u8(self.adpcm_address_control);
        writer.write_u8(self.adpcm_playback_rate);
        writer.write_u8(self.adpcm_fade_control);
        writer.write_u8(match self.audio_fade_target {
            None => 0,
            Some(CdFadeTarget::Cdda) => 1,
            Some(CdFadeTarget::Adpcm) => 2,
        });
        writer.write_u32(self.audio_fade_level_q16);
        writer.write_u64(self.audio_fade_step_ticks);
        writer.write_u64(self.audio_fade_ticks_to_next);
        writer.write_u32(self.adpcm_length);
        writer.write_bool(self.adpcm_playing);
        writer.write_bool(self.adpcm_stop_pending);
        writer.write_bool(self.adpcm_high_nibble_next);
        writer.write_u64(self.adpcm_clock_accumulator);
        writer.write_u16(self.adpcm_predictor);
        writer.write_u8(self.adpcm_step_index);
        writer.write_bool(self.adpcm_end_irq);
        writer.write_bool(self.adpcm_half_irq);
        self.adpcm_resampler.write_state(writer);
        writer.write_u32(self.adpcm_audio_samples.len() as u32);
        for &sample in &self.adpcm_audio_samples {
            writer.write_u16(sample as u16);
        }
    }

    pub(crate) fn read_state(&mut self, reader: &mut StateReader<'_>) -> anyhow::Result<()> {
        let target_audio = self.runtime_audio_config();
        let mut restored = Self::with_super_system_card(self.disc.clone(), self.super_system_card);
        restored.set_sample_rate(target_audio.0);
        restored.set_sample_generation_enabled(target_audio.1);
        restored.read_state_fields(reader, target_audio)?;
        *self = restored;
        Ok(())
    }

    fn read_state_fields(
        &mut self,
        reader: &mut StateReader<'_>,
        target_audio: (u32, bool),
    ) -> anyhow::Result<()> {
        reader.read_exact(&mut self.work_ram[..])?;
        reader.read_exact(&mut self.bram[..])?;
        reader.read_exact(&mut self.adpcm_ram[..])?;
        self.bram_unlocked = reader.read_bool()?;
        self.reset_asserted = reader.read_bool()?;
        self.phase = tag_to_phase(reader.read_u8()?)?;
        self.request = reader.read_bool()?;
        self.acknowledge = reader.read_bool()?;
        self.auto_acknowledge = reader.read_bool()?;
        self.acknowledged_request = reader.read_bool()?;
        self.request_pending = reader.read_bool()?;
        self.data_ready_irq_enabled = reader.read_bool()?;
        self.status_irq_enabled = reader.read_bool()?;
        self.audio_end_irq_enabled = reader.read_bool()?;
        self.audio_half_irq_enabled = reader.read_bool()?;
        self.output_latch = reader.read_u8()?;
        self.command = reader.read_vec(MAX_COMMAND_BYTES)?;
        self.response = reader.read_vec(MAX_RESPONSE_BYTES)?;
        self.response_index = reader.read_u32()? as usize;
        self.response_available = reader.read_u32()? as usize;
        if self.response_index > self.response_available
            || self.response_available > self.response.len()
        {
            bail!("invalid CD response cursors in save-state");
        }
        self.status = reader.read_u8()?;
        self.sense_key = reader.read_u8()?;
        self.additional_sense_code = reader.read_u8()?;
        self.event = read_event(reader)?;
        self.sector_arrival_ticks = read_optional_u64(reader)?;
        self.sectors_pending = reader.read_u16()?;
        self.sector_tick_remainder = reader.read_u64()?;
        if self.sector_tick_remainder >= CDROM2_SECTOR_TICKS_DENOMINATOR {
            bail!("invalid CD sector-clock remainder in save-state");
        }
        if self.sectors_pending == 0 && self.sector_arrival_ticks.is_some()
            || self.sectors_pending != 0 && self.sector_arrival_ticks.is_none()
        {
            bail!("inconsistent pending CD sector schedule in save-state");
        }

        self.audio_status = tag_to_audio_status(reader.read_u8()?)?;
        self.audio_start_lba = reader.read_u32()?;
        self.audio_end_lba = reader.read_u32()?;
        self.audio_current_lba = reader.read_u32()?;
        self.audio_current_sample = usize::from(reader.read_u16()?);
        if self.audio_current_sample >= 588 {
            bail!("invalid CDDA sample cursor in save-state");
        }
        self.audio_end_behavior = tag_to_end_behavior(reader.read_u8()?)?;
        self.audio_tick_accumulator = reader.read_u64()?;
        if self.audio_tick_accumulator >= CDDA_TICK_DENOMINATOR {
            bail!("invalid CDDA tick accumulator in save-state");
        }
        self.audio_left_sample = reader.read_u16()? as i16;
        self.audio_right_sample = reader.read_u16()? as i16;
        self.audio_sample_latch = reader.read_u16()? as i16;
        self.audio_sample_latch_right = reader.read_bool()?;
        self.audio_sample_latch_clock = reader.read_u64()?;
        self.audio_sample_latch_last_clock = reader.read_u64()?;
        let source_frame_count = reader.read_u32()? as usize;
        if source_frame_count > MAX_CDDA_SOURCE_FRAMES {
            bail!("CDDA source queue exceeds save-state bound: {source_frame_count}");
        }
        self.audio_source_frames = VecDeque::with_capacity(source_frame_count);
        for _ in 0..source_frame_count {
            let frame = [
                f32::from_bits(reader.read_u32()?),
                f32::from_bits(reader.read_u32()?),
            ];
            if frame
                .iter()
                .any(|sample| !sample.is_finite() || sample.abs() > 1.0)
            {
                bail!("invalid CDDA source sample in save-state");
            }
            self.audio_source_frames.push_back(frame);
        }
        self.audio_resample_position = reader.read_f64()?;
        if !self.audio_resample_position.is_finite() || self.audio_resample_position < 0.0 {
            bail!("invalid CDDA resample position in save-state");
        }
        let saved_sample_rate = reader.read_u32()?;
        if saved_sample_rate != target_audio.0 {
            bail!(
                "PC Engine CD save-state sample rate mismatch: state is {saved_sample_rate} Hz, destination is {} Hz",
                target_audio.0
            );
        }
        let saved_generation_enabled = reader.read_bool()?;
        if !saved_generation_enabled
            && (!self.audio_source_frames.is_empty() || self.audio_resample_position != 0.0)
        {
            bail!("disabled CDDA output has queued state in save-state");
        }

        self.adpcm_address_latch = reader.read_u16()?;
        self.adpcm_read_address = reader.read_u16()?;
        self.adpcm_write_address = reader.read_u16()?;
        self.adpcm_read_buffer = reader.read_u8()?;
        self.adpcm_dma_control = reader.read_u8()?;
        if self.adpcm_dma_control & !3 != 0 {
            bail!("invalid ADPCM DMA control in save-state");
        }
        self.adpcm_address_control = reader.read_u8()?;
        self.adpcm_playback_rate = reader.read_u8()?;
        if self.adpcm_playback_rate > 15 {
            bail!("invalid ADPCM playback rate in save-state");
        }
        self.adpcm_fade_control = reader.read_u8()?;
        self.audio_fade_target = match reader.read_u8()? {
            0 => None,
            1 => Some(CdFadeTarget::Cdda),
            2 => Some(CdFadeTarget::Adpcm),
            tag => bail!("invalid CD fade-target tag in save-state: {tag}"),
        };
        self.audio_fade_level_q16 = reader.read_u32()?;
        self.audio_fade_step_ticks = reader.read_u64()?;
        self.audio_fade_ticks_to_next = reader.read_u64()?;
        validate_fade(self)?;
        self.adpcm_length = reader.read_u32()?;
        if self.adpcm_length > 0x1_0000 {
            bail!("invalid ADPCM length in save-state");
        }
        self.adpcm_playing = reader.read_bool()?;
        self.adpcm_stop_pending = reader.read_bool()?;
        if self.adpcm_stop_pending && !self.adpcm_playing {
            bail!("stopped ADPCM has a pending stop in save-state");
        }
        self.adpcm_high_nibble_next = reader.read_bool()?;
        self.adpcm_clock_accumulator = reader.read_u64()?;
        let threshold = ADPCM_CLOCK_DENOMINATOR * (16 - u64::from(self.adpcm_playback_rate));
        if self.adpcm_clock_accumulator >= threshold {
            bail!("invalid ADPCM clock accumulator in save-state");
        }
        self.adpcm_predictor = reader.read_u16()?;
        self.adpcm_step_index = reader.read_u8()?;
        if self.adpcm_predictor > 0x0FFF || self.adpcm_step_index > 48 {
            bail!("invalid ADPCM codec state in save-state");
        }
        self.adpcm_end_irq = reader.read_bool()?;
        self.adpcm_half_irq = reader.read_bool()?;
        let saved_resampler = audio::MonoBlipResampler::read_state(saved_sample_rate, reader)?;
        let adpcm_sample_count = reader.read_u32()? as usize;
        if adpcm_sample_count > MAX_ADPCM_AUDIO_SAMPLES {
            bail!("ADPCM audio queue exceeds save-state bound: {adpcm_sample_count}");
        }
        let mut saved_adpcm_samples = Vec::with_capacity(adpcm_sample_count);
        for _ in 0..adpcm_sample_count {
            saved_adpcm_samples.push(reader.read_u16()? as i16);
        }
        if !saved_generation_enabled && adpcm_sample_count != 0 {
            bail!("disabled ADPCM output has queued audio in save-state");
        }

        self.audio_sample_rate = target_audio.0;
        self.audio_sample_generation_enabled = target_audio.1;
        if target_audio.1 && saved_generation_enabled {
            if saved_resampler.level() != self.current_adpcm_output_level() {
                bail!("ADPCM resampler level does not match codec state");
            }
            self.adpcm_resampler = saved_resampler;
            self.adpcm_audio_samples = saved_adpcm_samples;
        } else if target_audio.1 {
            self.audio_source_frames.clear();
            self.audio_resample_position = 0.0;
            self.adpcm_resampler = audio::MonoBlipResampler::at_level(
                target_audio.0,
                self.current_adpcm_output_level(),
            );
            self.adpcm_audio_samples.clear();
        } else {
            self.audio_source_frames.clear();
            self.audio_resample_position = 0.0;
            self.adpcm_resampler = audio::MonoBlipResampler::new(target_audio.0);
            self.adpcm_audio_samples.clear();
        }
        self.validate_v1_state()
    }
}

fn validate_fade(cd: &CdRom2) -> anyhow::Result<()> {
    if cd.audio_fade_level_q16 > 0x1_0000 {
        bail!("invalid CD fade level in save-state");
    }
    match cd.audio_fade_target {
        None if cd.audio_fade_level_q16 == 0x1_0000
            && cd.audio_fade_step_ticks == 0
            && cd.audio_fade_ticks_to_next == 0 => {}
        Some(_)
            if matches!(
                cd.audio_fade_step_ticks,
                PROVISIONAL_CDROM2_FADE_LONG_STEP_TICKS | PROVISIONAL_CDROM2_FADE_SHORT_STEP_TICKS
            ) && (cd.audio_fade_ticks_to_next != 0 || cd.audio_fade_level_q16 == 0)
                && cd.audio_fade_ticks_to_next <= cd.audio_fade_step_ticks => {}
        _ => bail!("inconsistent CD fade state in save-state"),
    }
    Ok(())
}

fn write_optional_u64(writer: &mut StateWriter, value: Option<u64>) {
    writer.write_bool(value.is_some());
    if let Some(value) = value {
        writer.write_u64(value);
    }
}

fn read_optional_u64(reader: &mut StateReader<'_>) -> anyhow::Result<Option<u64>> {
    reader.read_bool()?.then(|| reader.read_u64()).transpose()
}

fn write_event(writer: &mut StateWriter, event: Option<(CdEvent, u64)>) {
    let Some((event, ticks)) = event else {
        writer.write_u8(0);
        return;
    };
    writer.write_u8(event_to_tag(event) + 1);
    writer.write_u64(ticks);
}

fn read_event(reader: &mut StateReader<'_>) -> anyhow::Result<Option<(CdEvent, u64)>> {
    let tag = reader.read_u8()?;
    if tag == 0 {
        return Ok(None);
    }
    Ok(Some((tag_to_event(tag - 1)?, reader.read_u64()?)))
}

const fn phase_to_tag(phase: CdScsiPhase) -> u8 {
    match phase {
        CdScsiPhase::BusFree => 0,
        CdScsiPhase::Selection => 1,
        CdScsiPhase::Command => 2,
        CdScsiPhase::DataIn => 3,
        CdScsiPhase::Busy => 4,
        CdScsiPhase::Status => 5,
        CdScsiPhase::MessageIn => 6,
    }
}

fn tag_to_phase(tag: u8) -> anyhow::Result<CdScsiPhase> {
    Ok(match tag {
        0 => CdScsiPhase::BusFree,
        1 => CdScsiPhase::Selection,
        2 => CdScsiPhase::Command,
        3 => CdScsiPhase::DataIn,
        4 => CdScsiPhase::Busy,
        5 => CdScsiPhase::Status,
        6 => CdScsiPhase::MessageIn,
        _ => bail!("invalid CD SCSI-phase tag in save-state: {tag}"),
    })
}

const fn event_to_tag(event: CdEvent) -> u8 {
    match event {
        CdEvent::EnterCommand => 0,
        CdEvent::RaiseRequest => 1,
        CdEvent::ExecuteCommand => 2,
        CdEvent::EnterStatus => 3,
        CdEvent::EnterMessage => 4,
        CdEvent::EnterBusFree => 5,
        CdEvent::CompleteAutoAck => 6,
        CdEvent::CompleteAudioStart => 7,
    }
}

fn tag_to_event(tag: u8) -> anyhow::Result<CdEvent> {
    Ok(match tag {
        0 => CdEvent::EnterCommand,
        1 => CdEvent::RaiseRequest,
        2 => CdEvent::ExecuteCommand,
        3 => CdEvent::EnterStatus,
        4 => CdEvent::EnterMessage,
        5 => CdEvent::EnterBusFree,
        6 => CdEvent::CompleteAutoAck,
        7 => CdEvent::CompleteAudioStart,
        _ => bail!("invalid CD protocol-event tag in save-state: {tag}"),
    })
}

const fn audio_status_to_tag(status: CdAudioStatus) -> u8 {
    match status {
        CdAudioStatus::Playing => 0,
        CdAudioStatus::Inactive => 1,
        CdAudioStatus::Paused => 2,
        CdAudioStatus::Stopped => 3,
    }
}

fn tag_to_audio_status(tag: u8) -> anyhow::Result<CdAudioStatus> {
    Ok(match tag {
        0 => CdAudioStatus::Playing,
        1 => CdAudioStatus::Inactive,
        2 => CdAudioStatus::Paused,
        3 => CdAudioStatus::Stopped,
        _ => bail!("invalid CDDA status tag in save-state: {tag}"),
    })
}

const fn end_behavior_to_tag(behavior: CdAudioEndBehavior) -> u8 {
    match behavior {
        CdAudioEndBehavior::Stop => 0,
        CdAudioEndBehavior::Loop => 1,
        CdAudioEndBehavior::SignalCompletion => 2,
    }
}

fn tag_to_end_behavior(tag: u8) -> anyhow::Result<CdAudioEndBehavior> {
    Ok(match tag {
        0 => CdAudioEndBehavior::Stop,
        1 => CdAudioEndBehavior::Loop,
        2 => CdAudioEndBehavior::SignalCompletion,
        _ => bail!("invalid CDDA end-behavior tag in save-state: {tag}"),
    })
}
