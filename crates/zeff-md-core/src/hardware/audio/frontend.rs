use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

pub(crate) const MAX_FRONTEND_AUDIO_BYTES: usize =
    4 + 3 + 24 + output::HISTORY * 16 + 16 + 8 + 1 + 4 + SAMPLE_CAPACITY * 4;

fn mask<const N: usize>(mutes: [bool; N]) -> u8 {
    mutes
        .into_iter()
        .enumerate()
        .fold(0, |bits, (index, mute)| bits | (u8::from(mute) << index))
}

fn read_mask<const N: usize>(reader: &mut StateReader<'_>) -> Result<[bool; N]> {
    let bits = reader.read_u8()?;
    ensure!(bits >> N == 0, "invalid MD frontend mute bits");
    Ok(std::array::from_fn(|index| bits & (1 << index) != 0))
}

impl Audio {
    pub(crate) fn write_frontend_state(&self, writer: &mut StateWriter) -> Result<()> {
        assert!(self.deferred.is_none(), "canonical frontend audio state");
        writer.write_u32(self.sample_rate);
        writer.write_bool(self.generation_enabled);
        writer.write_u8(mask(self.mutes));
        writer.write_u8(mask(self.fm_mutes));
        writer.write_u64(self.master_hz);
        writer.write_u64(self.now);
        writer.write_u64(self.sample_phase);
        self.output.write_frontend_state(writer);
        writer.write_u32(self.len as u32);
        for index in 0..self.len {
            for sample in self.samples[(self.head + index) % SAMPLE_CAPACITY] {
                writer.write_u16(sample as u16);
            }
        }
        Ok(())
    }

    pub(crate) fn read_frontend_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        ensure!(self.deferred.is_none(), "noncanonical frontend audio state");
        let rate = reader.read_u32()?;
        ensure!(
            (1..=MAX_SAMPLE_RATE).contains(&rate),
            "invalid MD frontend sample rate"
        );
        let generation = reader.read_bool()?;
        let mutes = read_mask::<PSG_CHANNELS>(reader)?;
        let fm_mutes = read_mask::<FM_CHANNEL_COUNT>(reader)?;
        ensure!(
            reader.read_u64()? == self.master_hz,
            "MD frontend master clock mismatch"
        );
        ensure!(
            reader.read_u64()? == self.now,
            "MD frontend audio clock mismatch"
        );
        let phase = reader.read_u64()?;
        ensure!(phase < self.master_hz, "invalid MD frontend sample phase");
        let output = Output::read_frontend_state(reader)?;
        let psg = i32::from(self.psg.sample_with_mutes(mutes)) / PSG_MIX_DIVISOR;
        let level = self
            .opn2
            .sample_with_mutes(fm_mutes)
            .map(|fm| (fm + psg).clamp(i32::from(i16::MIN), i32::from(i16::MAX)));
        ensure!(output.level() == level, "MD frontend output level mismatch");
        let count = reader.read_u32()? as usize;
        ensure!(
            count <= SAMPLE_CAPACITY && (generation || count == 0),
            "invalid MD frontend queue length"
        );
        let mut samples = vec![[0; OUTPUT_CHANNELS]; SAMPLE_CAPACITY].into_boxed_slice();
        for frame in &mut samples[..count] {
            for sample in frame {
                *sample = reader.read_u16()? as i16;
            }
        }
        ensure!(reader.is_exhausted(), "trailing MD frontend audio data");
        if rate == self.sample_rate
            && generation == self.generation_enabled
            && mutes == self.mutes
            && fm_mutes == self.fm_mutes
        {
            self.sample_phase = phase;
            self.output = output;
            self.samples = samples;
            self.head = 0;
            self.len = count;
        }
        Ok(())
    }
}
