use super::Apu;
use crate::save_state::{StateReader, StateWriter};
use anyhow::Result;

impl Apu {
    /// Apply IO register values from a BESS save state (FF00–FF7F array).
    /// Sets APU registers without triggering side effects (no sound pulses).
    pub(crate) fn apply_bess_io(&mut self, io_regs: &[u8]) {
        // NR52 (FF26) — master enable only, write first per BESS spec
        self.nr52 = io_regs[0x26] & 0x80;
        // NR10–NR51 (FF10–FF25) → regs[0..22]
        self.regs[..0x17].copy_from_slice(&io_regs[0x10..0x27]);
        // Wave RAM (FF30–FF3F)
        self.wave_ram.copy_from_slice(&io_regs[0x30..0x40]);
    }

    pub(crate) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.regs);
        writer.write_bytes(&self.wave_ram);
        writer.write_u8(self.nr52);
        for ch in &self.channels {
            writer.write_bool(ch.enabled);
            writer.write_bool(ch.length_enabled);
            writer.write_u16(ch.length_counter);
            writer.write_u8(ch.sweep_period);
            writer.write_bool(ch.sweep_negate);
            writer.write_bool(ch.sweep_negate_used);
            writer.write_u8(ch.sweep_shift);
            writer.write_u8(ch.sweep_timer);
            writer.write_u16(ch.sweep_shadow_freq);
            writer.write_bool(ch.sweep_enabled);
            writer.write_u8(ch.envelope_period);
            writer.write_bool(ch.envelope_increase);
            writer.write_u8(ch.envelope_volume);
            writer.write_u8(ch.envelope_timer);
        }
        writer.write_u64(self.frame_seq_cycle_accum);
        writer.write_u8(self.frame_seq_step);
        writer.write_u64(self.ch1_timer);
        writer.write_u64(self.ch2_timer);
        writer.write_u64(self.ch3_timer);
        writer.write_u64(self.ch4_timer);
        writer.write_u8(self.ch1_duty_pos);
        writer.write_u8(self.ch2_duty_pos);
        writer.write_u8(self.ch3_wave_pos);
        writer.write_u16(self.ch4_lfsr);
    }

    pub(crate) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let mut apu = Self::new();
        reader.read_exact(&mut apu.regs)?;
        reader.read_exact(&mut apu.wave_ram)?;
        apu.nr52 = reader.read_u8()?;
        for ch in &mut apu.channels {
            ch.enabled = reader.read_bool()?;
            ch.length_enabled = reader.read_bool()?;
            ch.length_counter = reader.read_u16()?;
            ch.sweep_period = reader.read_u8()?;
            ch.sweep_negate = reader.read_bool()?;
            ch.sweep_negate_used = reader.read_bool()?;
            ch.sweep_shift = reader.read_u8()?;
            ch.sweep_timer = reader.read_u8()?;
            ch.sweep_shadow_freq = reader.read_u16()?;
            ch.sweep_enabled = reader.read_bool()?;
            ch.envelope_period = reader.read_u8()?;
            ch.envelope_increase = reader.read_bool()?;
            ch.envelope_volume = reader.read_u8()?;
            ch.envelope_timer = reader.read_u8()?;
        }
        apu.frame_seq_cycle_accum = reader.read_u64()?;
        apu.frame_seq_step = reader.read_u8()?;
        apu.ch1_timer = reader.read_u64()?;
        apu.ch2_timer = reader.read_u64()?;
        apu.ch3_timer = reader.read_u64()?;
        apu.ch4_timer = reader.read_u64()?;
        apu.ch1_duty_pos = reader.read_u8()?;
        apu.ch2_duty_pos = reader.read_u8()?;
        apu.ch3_wave_pos = reader.read_u8()?;
        apu.ch4_lfsr = reader.read_u16()?;

        apu.sample_buffer.clear();
        apu.sample_cycle_accum = 0.0;
        apu.debug_capture_enabled = false;
        apu.debug_capture_cycle_accum = 0;
        for history in &mut apu.channel_debug_history {
            history.clear();
        }
        apu.master_debug_history.clear();
        apu.channel_muted = [false; 4];
        Ok(apu)
    }
}
