use crate::Region;

mod constants;
mod opn2;
mod output;
mod psg;
#[cfg(test)]
mod publication_mix_tests;
mod state;
pub(crate) mod trace;
use crate::constants::{FM_CHANNEL_COUNT, OPN2_CLOCK_TICKS};
use constants::*;
pub use opn2::{Opn2, Opn2DebugSnapshot};
use output::Output;
pub use psg::{Psg, PsgDebugSnapshot};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Audio {
    pub psg: Psg,
    pub opn2: Opn2,
    now: u64,
    chip_phase: u64,
    opn2_phase: u64,
    sample_phase: u64,
    sample_rate: u32,
    master_hz: u64,
    generation_enabled: bool,
    mutes: [bool; PSG_CHANNELS],
    fm_mutes: [bool; FM_CHANNEL_COUNT],
    output: Output,
    pub trace: zeff_emu_common::audio_trace::MdAudioTraceRecorder,
    pub trace_origin: Option<zeff_emu_common::audio_trace::MdTraceOrigin>,
    samples: Box<[[i16; OUTPUT_CHANNELS]]>,
    head: usize,
    len: usize,
}

pub(crate) struct AudioCheckpoint {
    psg: Psg,
    opn2: Opn2,
    now: u64,
    chip_phase: u64,
    opn2_phase: u64,
    sample_phase: u64,
    sample_rate: u32,
    master_hz: u64,
    generation_enabled: bool,
    mutes: [bool; PSG_CHANNELS],
    fm_mutes: [bool; FM_CHANNEL_COUNT],
    output: Output,
    samples: [[i16; OUTPUT_CHANNELS]; SAMPLE_CAPACITY],
    head: usize,
    len: usize,
}

impl Default for Audio {
    fn default() -> Self {
        Self::new(Region::NtscUs)
    }
}

impl Audio {
    pub fn new(region: Region) -> Self {
        Self {
            psg: Psg::default(),
            opn2: Opn2::default(),
            now: 0,
            chip_phase: 0,
            opn2_phase: 0,
            sample_phase: 0,
            sample_rate: DEFAULT_SAMPLE_RATE,
            master_hz: region.master_hz(),
            samples: vec![[0; OUTPUT_CHANNELS]; SAMPLE_CAPACITY].into_boxed_slice(),
            head: 0,
            len: 0,
            generation_enabled: true,
            mutes: [false; PSG_CHANNELS],
            fm_mutes: [false; FM_CHANNEL_COUNT],
            output: Output::at_level([0; OUTPUT_CHANNELS]),
            trace: Default::default(),
            trace_origin: None,
        }
    }
    pub fn write_psg(&mut self, at: u64, value: u8) {
        self.advance_to(at);
        self.psg.write_data(value);
        self.refresh_output();
        if let Some(origin) = self.trace_origin {
            self.record_trace(
                at,
                zeff_emu_common::audio_trace::MdTraceWrite::Psg { origin, value },
            );
        }
    }

    pub fn write_opn2(&mut self, at: u64, port: u8, value: u8) {
        self.advance_to(at);
        self.opn2.write_port(port, value);
        self.refresh_output();
        if let Some(origin) = self.trace_origin {
            self.record_trace(
                at,
                zeff_emu_common::audio_trace::MdTraceWrite::Opn2Port {
                    origin,
                    port,
                    value,
                },
            );
        }
    }

    pub fn reset_opn2(&mut self, at: u64) {
        self.advance_to(at);
        self.opn2 = Opn2::default();
        self.opn2_phase = 0;
        self.refresh_output();
        if let Some(origin) = self.trace_origin {
            self.record_trace(
                at,
                zeff_emu_common::audio_trace::MdTraceWrite::Opn2Reset { origin },
            );
        }
    }

    pub fn opn2_status_at(&self, at: u64) -> u8 {
        let elapsed = at.checked_sub(self.now).expect("ordered status prediction");
        let clocks = elapsed / OPN2_CLOCK_TICKS
            + (elapsed % OPN2_CLOCK_TICKS + self.opn2_phase) / OPN2_CLOCK_TICKS;
        self.opn2.status_after(clocks)
    }

    pub fn advance_to(&mut self, at: u64) {
        let elapsed = at.checked_sub(self.now).expect("ordered audio publication");
        let total =
            u128::from(self.sample_phase) + u128::from(elapsed) * u128::from(self.sample_rate);
        let count = (total / u128::from(self.master_hz)) as u64;
        if !self.generation_enabled {
            self.sample_phase = (total % u128::from(self.master_hz)) as u64;
            self.advance_chip(at);
            self.restart_filter();
            return;
        }
        let retained = (SAMPLE_CAPACITY + output::HISTORY) as u64;
        if count > retained {
            let delta = (u128::from(count - retained) * u128::from(self.master_hz)
                - u128::from(self.sample_phase))
            .div_ceil(u128::from(self.sample_rate)) as u64;
            self.sample_phase = ((u128::from(self.sample_phase)
                + u128::from(delta) * u128::from(self.sample_rate))
                % u128::from(self.master_hz)) as u64;
            self.advance_chip(self.now + delta);
            self.restart_filter();
            self.head = 0;
            self.len = 0;
        }
        while self.now < at {
            let psg = PSG_CLOCK_TICKS - self.chip_phase;
            let fm = self.opn2.clocks_to_sample() * OPN2_CLOCK_TICKS - self.opn2_phase;
            let sample = (self.master_hz - self.sample_phase).div_ceil(u64::from(self.sample_rate));
            let step = (at - self.now).min(psg).min(fm).min(sample);
            self.advance_chip(self.now + step);
            self.sample_phase += step * u64::from(self.sample_rate);
            if step == psg || step == fm {
                self.refresh_output();
            }
            if self.sample_phase >= self.master_hz {
                let sample = self.output.sample();
                self.push(sample);
                self.sample_phase -= self.master_hz;
            }
        }
    }

    fn mixed_level(&self) -> [i32; OUTPUT_CHANNELS] {
        let psg = i32::from(self.psg.sample_with_mutes(self.mutes)) / PSG_MIX_DIVISOR;
        self.opn2
            .sample_with_mutes(self.fm_mutes)
            .map(|fm| (fm + psg).clamp(i32::from(i16::MIN), i32::from(i16::MAX)))
    }

    fn refresh_output(&mut self) {
        self.output
            .change_level(self.mixed_level(), self.sample_phase, self.master_hz);
    }

    fn restart_filter(&mut self) {
        self.output = Output::at_level(self.mixed_level());
    }

    fn restart_output(&mut self) {
        self.sample_phase = 0;
        self.samples.fill([0; OUTPUT_CHANNELS]);
        self.head = 0;
        self.len = 0;
        self.restart_filter();
    }

    fn advance_chip(&mut self, at: u64) {
        let elapsed = at - self.now;
        let remainder = elapsed % PSG_CLOCK_TICKS + self.chip_phase;
        self.psg
            .advance(elapsed / PSG_CLOCK_TICKS + remainder / PSG_CLOCK_TICKS);
        self.chip_phase = remainder % PSG_CLOCK_TICKS;
        let remainder = elapsed % OPN2_CLOCK_TICKS + self.opn2_phase;
        self.opn2
            .advance(elapsed / OPN2_CLOCK_TICKS + remainder / OPN2_CLOCK_TICKS);
        self.opn2_phase = remainder % OPN2_CLOCK_TICKS;
        self.now = at;
    }

    fn push(&mut self, sample: [i16; OUTPUT_CHANNELS]) {
        let index = (self.head + self.len) % SAMPLE_CAPACITY;
        self.samples[index] = sample;
        if self.len == SAMPLE_CAPACITY {
            self.head = (self.head + 1) % SAMPLE_CAPACITY;
        } else {
            self.len += 1;
        }
    }

    pub fn drain_into(&mut self, output: &mut Vec<f32>) {
        output.reserve(self.len * OUTPUT_CHANNELS);
        for index in 0..self.len {
            let sample = self.samples[(self.head + index) % SAMPLE_CAPACITY];
            output.extend(sample.map(|value| f32::from(value) / SAMPLE_SCALE));
        }
        self.head = 0;
        self.len = 0;
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    pub fn set_sample_generation_enabled(&mut self, enabled: bool) {
        if self.generation_enabled == enabled {
            return;
        }
        self.generation_enabled = enabled;
        if !enabled {
            self.head = 0;
            self.len = 0;
        }
        self.restart_filter();
    }
    pub fn set_channel_mutes(&mut self, mutes: [bool; PSG_CHANNELS]) {
        if self.mutes == mutes {
            return;
        }
        self.mutes = mutes;
        self.restart_filter();
    }
    pub fn set_fm_channel_mutes(&mut self, mutes: [bool; FM_CHANNEL_COUNT]) {
        if self.fm_mutes == mutes {
            return;
        }
        self.fm_mutes = mutes;
        self.restart_filter();
    }
    pub fn set_sample_rate(&mut self, rate: u32) {
        self.sample_rate = if rate == 0 {
            DEFAULT_SAMPLE_RATE
        } else {
            rate.min(MAX_SAMPLE_RATE)
        };
        self.restart_output();
    }

    pub fn reset(&mut self) {
        self.psg = Psg::default();
        self.opn2 = Opn2::default();
        self.opn2_phase = 0;
        self.now = 0;
        self.chip_phase = 0;
        self.sample_phase = 0;
        self.samples.fill([0; OUTPUT_CHANNELS]);
        self.head = 0;
        self.len = 0;
        self.restart_filter();
    }

    pub fn checkpoint(&self) -> AudioCheckpoint {
        let mut samples = [[0; OUTPUT_CHANNELS]; SAMPLE_CAPACITY];
        samples.copy_from_slice(&self.samples);
        AudioCheckpoint {
            psg: self.psg,
            opn2: self.opn2,
            opn2_phase: self.opn2_phase,
            now: self.now,
            chip_phase: self.chip_phase,
            sample_phase: self.sample_phase,
            sample_rate: self.sample_rate,
            master_hz: self.master_hz,
            samples,
            head: self.head,
            len: self.len,
            generation_enabled: self.generation_enabled,
            mutes: self.mutes,
            fm_mutes: self.fm_mutes,
            output: self.output,
        }
    }

    pub fn restore(&mut self, saved: AudioCheckpoint) {
        assert_eq!(self.master_hz, saved.master_hz);
        self.psg = saved.psg;
        self.opn2 = saved.opn2;
        self.opn2_phase = saved.opn2_phase;
        self.now = saved.now;
        self.chip_phase = saved.chip_phase;
        self.sample_phase = saved.sample_phase;
        self.sample_rate = saved.sample_rate;
        self.generation_enabled = saved.generation_enabled;
        self.mutes = saved.mutes;
        self.fm_mutes = saved.fm_mutes;
        self.output = saved.output;
        self.samples.copy_from_slice(&saved.samples);
        self.head = saved.head;
        self.len = saved.len;
    }
}

#[cfg(test)]
mod opn2_mix_tests;
#[cfg(test)]
mod tests;
