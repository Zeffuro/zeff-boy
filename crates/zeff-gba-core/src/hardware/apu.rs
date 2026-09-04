use std::collections::VecDeque;

mod direct_sound;
mod psg;

use psg::Psg;

const FIFO_CAPACITY: usize = 32;
const DEBUG_SAMPLE_HISTORY_LEN: usize = 512;

#[derive(Clone, Copy, Debug)]
struct DebugSamples {
    samples: [f32; DEBUG_SAMPLE_HISTORY_LEN],
    write_pos: usize,
}

impl Default for DebugSamples {
    fn default() -> Self {
        Self {
            samples: [0.0; DEBUG_SAMPLE_HISTORY_LEN],
            write_pos: 0,
        }
    }
}

impl DebugSamples {
    fn push(&mut self, sample: f32) {
        self.samples[self.write_pos] = sample;
        self.write_pos = (self.write_pos + 1) % DEBUG_SAMPLE_HISTORY_LEN;
    }

    fn ordered(&self) -> [f32; DEBUG_SAMPLE_HISTORY_LEN] {
        let mut out = [0.0; DEBUG_SAMPLE_HISTORY_LEN];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = self.samples[(self.write_pos + i) % DEBUG_SAMPLE_HISTORY_LEN];
        }
        out
    }

    fn clear(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, Debug)]
pub struct Apu {
    sample_rate: u32,
    sample_generation_enabled: bool,
    channel_mutes: [bool; 6],
    fifo_a: VecDeque<i8>,
    fifo_b: VecDeque<i8>,
    current_a: i8,
    current_b: i8,
    output_phase: f64,
    dac_phase: f64,
    dac_accum_left: f32,
    dac_accum_right: f32,
    dac_accum_count: u32,
    last_dac_left: f32,
    last_dac_right: f32,
    output_filter_left: f32,
    output_filter_right: f32,
    psg_cycle_accum: u32,
    sample_buffer: Vec<f32>,
    psg: Psg,
    output_pairs_generated: u64,
    direct_pairs_generated: u64,
    psg_pairs_generated: u64,
    debug_capture_enabled: bool,
    direct_debug_history: [DebugSamples; 2],
    master_debug_history: DebugSamples,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApuDebugSnapshot {
    pub sample_rate: u32,
    pub psg_sample_rate: u32,
    pub sample_generation_enabled: bool,
    pub debug_capture_enabled: bool,
    pub sample_buffer_len: usize,
    pub fifo_len: [usize; 2],
    pub current_sample: [i8; 2],
    pub output_pairs_generated: u64,
    pub direct_pairs_generated: u64,
    pub psg_pairs_generated: u64,
    pub psg_enabled: [bool; 4],
    pub psg_frequency: [u16; 3],
    pub psg_volume: [u8; 4],
    pub channel_mutes: [bool; 6],
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ApuSaveState {
    pub fifo_a: Vec<u8>,
    pub fifo_b: Vec<u8>,
    pub current_a: i8,
    pub current_b: i8,
    pub output_phase: f64,
    pub dac_phase: f64,
    pub dac_accum_left: f32,
    pub dac_accum_right: f32,
    pub dac_accum_count: u32,
    pub last_dac_left: f32,
    pub last_dac_right: f32,
    pub output_filter_left: f32,
    pub output_filter_right: f32,
    pub psg_cycle_accum: u32,
    pub output_pairs_generated: u64,
    pub direct_pairs_generated: u64,
    pub psg_pairs_generated: u64,
}

impl Default for Apu {
    fn default() -> Self {
        Self::new(crate::hardware::constants::GBA_DEFAULT_HOST_SAMPLE_RATE_HZ)
    }
}

impl Apu {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            sample_generation_enabled: true,
            channel_mutes: [false; 6],
            fifo_a: VecDeque::with_capacity(FIFO_CAPACITY),
            fifo_b: VecDeque::with_capacity(FIFO_CAPACITY),
            current_a: 0,
            current_b: 0,
            output_phase: 0.0,
            dac_phase: 0.0,
            dac_accum_left: 0.0,
            dac_accum_right: 0.0,
            dac_accum_count: 0,
            last_dac_left: 0.0,
            last_dac_right: 0.0,
            output_filter_left: 0.0,
            output_filter_right: 0.0,
            psg_cycle_accum: 0,
            sample_buffer: Vec::new(),
            psg: Psg::new(sample_rate),
            output_pairs_generated: 0,
            direct_pairs_generated: 0,
            psg_pairs_generated: 0,
            debug_capture_enabled: false,
            direct_debug_history: [DebugSamples::default(); 2],
            master_debug_history: DebugSamples::default(),
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate.max(1);
        self.output_phase = 0.0;
        self.dac_phase = 0.0;
        self.dac_accum_left = 0.0;
        self.dac_accum_right = 0.0;
        self.dac_accum_count = 0;
        self.output_filter_left = 0.0;
        self.output_filter_right = 0.0;
        self.psg.set_sample_rate(self.sample_rate);
    }

    pub fn set_sample_generation_enabled(&mut self, enabled: bool) {
        self.sample_generation_enabled = enabled;
        self.psg.disable_host_sample_generation();
    }

    pub fn set_debug_capture_enabled(&mut self, enabled: bool) {
        self.debug_capture_enabled = enabled;
        self.psg.set_debug_capture_enabled(enabled);
    }

    pub fn set_channel_mutes(&mut self, mutes: [bool; 6]) {
        self.channel_mutes = mutes;
        self.psg
            .set_channel_mutes([mutes[0], mutes[1], mutes[2], mutes[3]]);
    }

    pub(crate) fn reset_hardware(&mut self) {
        let sample_rate = self.sample_rate;
        let sample_generation_enabled = self.sample_generation_enabled;
        let channel_mutes = self.channel_mutes;
        let debug_capture_enabled = self.debug_capture_enabled;
        *self = Self::new(sample_rate);
        self.set_sample_generation_enabled(sample_generation_enabled);
        self.set_channel_mutes(channel_mutes);
        self.set_debug_capture_enabled(debug_capture_enabled);
    }

    pub fn drain_samples_into(&mut self, buf: &mut Vec<f32>) {
        buf.clear();
        buf.append(&mut self.sample_buffer);
    }

    #[cfg(test)]
    pub(crate) fn fifo_len(&self, fifo: usize) -> usize {
        if fifo == 0 {
            self.fifo_a.len()
        } else {
            self.fifo_b.len()
        }
    }

    pub(crate) fn channel_mutes(&self) -> [bool; 6] {
        self.channel_mutes
    }

    pub fn psg_regs_snapshot(&self) -> [u8; 0x17] {
        self.psg.regs_snapshot()
    }

    pub fn psg_wave_ram_snapshot(&self) -> [u8; 0x10] {
        self.psg.wave_ram_snapshot()
    }

    pub fn psg_nr52_raw(&self) -> u8 {
        self.psg.nr52_raw()
    }

    pub fn psg_channel_debug_samples_ordered(&self, channel: usize) -> [f32; 512] {
        self.psg.channel_debug_samples_ordered(channel)
    }

    pub fn psg_master_debug_samples_ordered(&self) -> [f32; 512] {
        self.psg.master_debug_samples_ordered()
    }

    pub fn direct_debug_samples_ordered(&self, fifo: usize) -> [f32; 512] {
        self.direct_debug_history[fifo.min(1)].ordered()
    }

    pub fn master_debug_samples_ordered(&self) -> [f32; 512] {
        self.master_debug_history.ordered()
    }

    pub fn debug_snapshot(&self) -> ApuDebugSnapshot {
        let psg = self.psg.channel_snapshot();
        ApuDebugSnapshot {
            sample_rate: self.sample_rate,
            psg_sample_rate: self.psg.sample_rate(),
            sample_generation_enabled: self.sample_generation_enabled,
            debug_capture_enabled: self.debug_capture_enabled,
            sample_buffer_len: self.sample_buffer.len(),
            fifo_len: [self.fifo_a.len(), self.fifo_b.len()],
            current_sample: [self.current_a, self.current_b],
            output_pairs_generated: self.output_pairs_generated,
            direct_pairs_generated: self.direct_pairs_generated,
            psg_pairs_generated: self.psg_pairs_generated,
            psg_enabled: [
                psg.ch1_enabled,
                psg.ch2_enabled,
                psg.ch3_enabled,
                psg.ch4_enabled,
            ],
            psg_frequency: [psg.ch1_frequency, psg.ch2_frequency, psg.ch3_frequency],
            psg_volume: [
                psg.ch1_volume,
                psg.ch2_volume,
                psg.ch3_output_level,
                psg.ch4_volume,
            ],
            channel_mutes: self.channel_mutes,
        }
    }

    pub(crate) fn save_state(&self) -> ApuSaveState {
        ApuSaveState {
            fifo_a: self.fifo_a.iter().map(|&sample| sample as u8).collect(),
            fifo_b: self.fifo_b.iter().map(|&sample| sample as u8).collect(),
            current_a: self.current_a,
            current_b: self.current_b,
            output_phase: self.output_phase,
            dac_phase: self.dac_phase,
            dac_accum_left: self.dac_accum_left,
            dac_accum_right: self.dac_accum_right,
            dac_accum_count: self.dac_accum_count,
            last_dac_left: self.last_dac_left,
            last_dac_right: self.last_dac_right,
            output_filter_left: self.output_filter_left,
            output_filter_right: self.output_filter_right,
            psg_cycle_accum: self.psg_cycle_accum,
            output_pairs_generated: self.output_pairs_generated,
            direct_pairs_generated: self.direct_pairs_generated,
            psg_pairs_generated: self.psg_pairs_generated,
        }
    }

    pub(crate) fn load_save_state(&mut self, state: ApuSaveState) {
        self.fifo_a.clear();
        self.fifo_b.clear();
        self.fifo_a.extend(
            state
                .fifo_a
                .into_iter()
                .take(FIFO_CAPACITY)
                .map(|b| b as i8),
        );
        self.fifo_b.extend(
            state
                .fifo_b
                .into_iter()
                .take(FIFO_CAPACITY)
                .map(|b| b as i8),
        );
        self.current_a = state.current_a;
        self.current_b = state.current_b;
        self.output_phase = state.output_phase;
        self.dac_phase = state.dac_phase;
        self.dac_accum_left = state.dac_accum_left;
        self.dac_accum_right = state.dac_accum_right;
        self.dac_accum_count = state.dac_accum_count;
        self.last_dac_left = state.last_dac_left;
        self.last_dac_right = state.last_dac_right;
        self.output_filter_left = state.output_filter_left;
        self.output_filter_right = state.output_filter_right;
        self.psg_cycle_accum = state.psg_cycle_accum;
        self.output_pairs_generated = state.output_pairs_generated;
        self.direct_pairs_generated = state.direct_pairs_generated;
        self.psg_pairs_generated = state.psg_pairs_generated;
    }

    pub(crate) fn clear_host_output_after_state_load(&mut self) {
        self.sample_buffer.clear();
        for history in &mut self.direct_debug_history {
            history.clear();
        }
        self.master_debug_history.clear();
        self.psg.clear_host_output_after_state_load();
    }

    #[cfg(test)]
    pub(crate) fn seed_host_output_for_state_load_test(&mut self) {
        self.sample_buffer.extend([0.25, -0.5]);
        for (index, history) in self.direct_debug_history.iter_mut().enumerate() {
            history.push((index + 1) as f32);
        }
        self.master_debug_history.push(3.0);
        self.psg.seed_host_output_for_state_load_test();
    }

    #[cfg(test)]
    pub(crate) fn psg_host_output_state_for_test(&self) -> (usize, u64) {
        self.psg.host_output_state_for_test()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FifoDmaRequests {
    pub a: bool,
    pub b: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::constants::CYCLES_PER_FRAME;

    #[test]
    fn fifo_timer_pop_requests_dma_at_half_empty() {
        let mut apu = Apu::new(48_000);
        apu.write_fifo_halfword(0, 0x807F);

        let requests = apu.on_timer_overflows([1, 0, 0, 0], (1 << 8) | (1 << 9));

        assert!(requests.a);
        assert_eq!(apu.current_a, 0x7F);
        assert_eq!(apu.fifo_len(0), 1);
    }

    #[test]
    fn fifo_consumes_one_sample_per_selected_timer_overflow() {
        let mut apu = Apu::new(48_000);
        for pair in 0..10u16 {
            let lo = pair * 2 + 1;
            let hi = lo + 1;
            apu.write_fifo_halfword(0, lo | (hi << 8));
        }

        let requests = apu.on_timer_overflows([4, 0, 0, 0], (1 << 8) | (1 << 9));

        assert!(requests.a);
        assert_eq!(apu.current_a, 4);
        assert_eq!(apu.fifo_len(0), 16);
    }

    #[test]
    fn direct_sound_mixes_stereo_samples() {
        let mut apu = Apu::new(48_000);
        apu.current_a = 64;
        apu.step_output(
            CYCLES_PER_FRAME,
            (1 << 2) | (1 << 8) | (1 << 9),
            0x0080,
            0x0200,
        );

        assert!(!apu.sample_buffer.is_empty());
        assert!(apu.sample_buffer.iter().any(|&sample| sample > 0.0));
    }

    #[test]
    fn debug_capture_records_direct_and_master_waveforms() {
        let mut apu = Apu::new(48_000);
        apu.current_a = 64;
        apu.set_debug_capture_enabled(true);
        apu.step_output(
            CYCLES_PER_FRAME,
            (1 << 2) | (1 << 8) | (1 << 9),
            0x0080,
            0x0200,
        );

        let snapshot = apu.debug_snapshot();
        assert!(snapshot.debug_capture_enabled);
        assert!(
            apu.direct_debug_samples_ordered(0)
                .iter()
                .any(|sample| *sample > 0.0)
        );
        assert!(
            apu.master_debug_samples_ordered()
                .iter()
                .any(|sample| *sample > 0.0)
        );
    }

    #[test]
    fn direct_sound_full_scale_is_below_host_clip_level() {
        let mut apu = Apu::new(48_000);
        apu.current_a = 127;

        apu.step_output(1024, (1 << 2) | (1 << 8) | (1 << 9), 0x0080, 0x0200);

        let peak = apu
            .sample_buffer
            .iter()
            .copied()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        assert!(
            peak <= 0.75,
            "GBA Direct Sound should leave host headroom, got peak {peak}"
        );
        assert!(peak > 0.35);
    }

    #[test]
    fn direct_sound_channels_saturate_before_host_output_gain() {
        let mut apu = Apu::new(48_000);
        apu.current_a = 127;
        apu.current_b = 127;

        apu.step_output(
            CYCLES_PER_FRAME,
            (1 << 2) | (1 << 3) | (1 << 8) | (1 << 9) | (1 << 12) | (1 << 13),
            0x0080,
            0x0200,
        );

        let peak = apu
            .sample_buffer
            .iter()
            .copied()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        assert!(peak <= 0.75);
        assert!(peak > 0.70);
    }

    #[test]
    fn output_rate_matches_configured_sample_rate() {
        let mut apu = Apu::new(48_000);
        apu.current_a = 64;
        apu.step_output(
            CYCLES_PER_FRAME,
            (1 << 2) | (1 << 8) | (1 << 9),
            0x0080,
            0x0200,
        );

        let stereo_pairs = apu.sample_buffer.len() / 2;
        assert!(
            (802..=805).contains(&stereo_pairs),
            "expected about 804 stereo pairs per GBA frame at 48 kHz, got {stereo_pairs}"
        );
    }

    #[test]
    fn output_rate_matches_configured_sample_rate_with_psg_powered() {
        let mut apu = Apu::new(48_000);
        apu.write_psg(0xFF26, 0x80);
        apu.write_psg(0xFF24, 0x77);
        apu.write_psg(0xFF25, 0xFF);

        apu.step_output(CYCLES_PER_FRAME, 0, 0x0080, 0x0200);

        let stereo_pairs = apu.sample_buffer.len() / 2;
        assert!(
            (802..=805).contains(&stereo_pairs),
            "expected about 804 stereo pairs per GBA frame at 48 kHz, got {stereo_pairs}"
        );
    }

    #[test]
    fn output_rate_matches_configured_sample_rate_with_higher_soundbias_resolution() {
        let mut apu = Apu::new(48_000);
        apu.current_a = 64;

        apu.step_output(
            CYCLES_PER_FRAME,
            (1 << 2) | (1 << 8) | (1 << 9),
            0x0080,
            0x4200,
        );

        let stereo_pairs = apu.sample_buffer.len() / 2;
        assert!(
            (802..=805).contains(&stereo_pairs),
            "expected about 804 stereo pairs per GBA frame at 48 kHz, got {stereo_pairs}"
        );
    }

    #[test]
    fn output_rate_is_stable_across_small_steps() {
        let mut apu = Apu::new(48_000);
        apu.current_a = 64;
        apu.write_psg(0xFF26, 0x80);
        apu.write_psg(0xFF24, 0x77);
        apu.write_psg(0xFF25, 0xFF);

        let mut remaining = CYCLES_PER_FRAME;
        while remaining > 0 {
            let step = remaining.min(64);
            apu.step_output(step, (1 << 2) | (1 << 8) | (1 << 9), 0x0080, 0x0200);
            remaining -= step;
        }

        let stereo_pairs = apu.sample_buffer.len() / 2;
        assert!(
            (802..=805).contains(&stereo_pairs),
            "expected about 804 stereo pairs per GBA frame at 48 kHz, got {stereo_pairs}"
        );
    }

    #[test]
    fn output_rate_is_stable_with_triggered_psg_channel() {
        let mut apu = Apu::new(48_000);
        apu.write_psg(0xFF26, 0x80);
        apu.write_psg(0xFF24, 0x77);
        apu.write_psg(0xFF25, 0xFF);
        apu.write_psg(0xFF16, 0x80);
        apu.write_psg(0xFF17, 0xF0);
        apu.write_psg(0xFF18, 0xC3);
        apu.write_psg(0xFF19, 0x87);

        for _ in 0..(CYCLES_PER_FRAME / 64) {
            apu.step_output(64, 0, 0x0080, 0x0200);
        }

        let stereo_pairs = apu.sample_buffer.len() / 2;
        assert!(
            (802..=805).contains(&stereo_pairs),
            "expected about 804 stereo pairs per GBA frame at 48 kHz, got {stereo_pairs}"
        );
    }
}
