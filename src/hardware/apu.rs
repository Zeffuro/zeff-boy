use crate::hardware::types::constants::*;

const APU_T_CYCLES_PER_SECOND: f64 = 4_194_304.0;
const DEBUG_SAMPLE_HISTORY_LEN: usize = 512;
const DEBUG_CAPTURE_DECIMATION_T_CYCLES: u64 = 64;
const DUTY_TABLE: [[bool; 8]; 4] = [
	[false, false, false, false, false, false, false, true],
	[true, false, false, false, false, false, false, true],
	[true, false, false, false, false, true, true, true],
	[false, true, true, true, true, true, true, false],
];

#[derive(Clone, Copy, Default)]
struct ChannelState {
	enabled: bool,
	length_enabled: bool,
	length_counter: u16,
	sweep_period: u8,
	sweep_negate: bool,
	sweep_negate_used: bool,
	sweep_shift: u8,
	sweep_timer: u8,
	sweep_shadow_freq: u16,
	sweep_enabled: bool,
	envelope_period: u8,
	envelope_increase: bool,
	envelope_volume: u8,
	envelope_timer: u8,
}

#[derive(Clone, Copy)]
pub(crate) struct ChannelDebugSamples {
	samples: [f32; DEBUG_SAMPLE_HISTORY_LEN],
	write_pos: usize,
}

impl Default for ChannelDebugSamples {
	fn default() -> Self {
		Self {
			samples: [0.0; DEBUG_SAMPLE_HISTORY_LEN],
			write_pos: 0,
		}
	}
}

impl ChannelDebugSamples {
	fn push(&mut self, sample: f32) {
		self.samples[self.write_pos] = sample;
		self.write_pos = (self.write_pos + 1) % DEBUG_SAMPLE_HISTORY_LEN;
	}

	fn clear(&mut self) {
		self.samples = [0.0; DEBUG_SAMPLE_HISTORY_LEN];
		self.write_pos = 0;
	}

	fn ordered(&self) -> [f32; DEBUG_SAMPLE_HISTORY_LEN] {
		let mut out = [0.0; DEBUG_SAMPLE_HISTORY_LEN];
		for (i, slot) in out.iter_mut().enumerate() {
			*slot = self.samples[(self.write_pos + i) % DEBUG_SAMPLE_HISTORY_LEN];
		}
		out
	}
}

pub(crate) struct Apu {
	regs: [u8; 0x17],
	wave_ram: [u8; 0x10],
	nr52: u8,
	channels: [ChannelState; 4],
	frame_seq_cycle_accum: u64,
	frame_seq_step: u8,
	ch1_timer: u64,
	ch2_timer: u64,
	ch3_timer: u64,
	ch4_timer: u64,
	ch1_duty_pos: u8,
	ch2_duty_pos: u8,
	ch3_wave_pos: u8,
	ch4_lfsr: u16,
	pub(crate) sample_rate: u32,
	sample_buffer: Vec<f32>,
	sample_cycle_accum: f64,
	debug_capture_cycle_accum: u64,
	channel_debug_history: [ChannelDebugSamples; 4],
	master_debug_history: ChannelDebugSamples,
	channel_muted: [bool; 4],
}

impl Apu {
	pub(crate) fn new() -> Self {
		Self {
			regs: [0; 0x17],
			wave_ram: [0; 0x10],
			nr52: 0,
			channels: [ChannelState::default(); 4],
			frame_seq_cycle_accum: 0,
			frame_seq_step: 0,
			ch1_timer: 0,
			ch2_timer: 0,
			ch3_timer: 0,
			ch4_timer: 0,
			ch1_duty_pos: 0,
			ch2_duty_pos: 0,
			ch3_wave_pos: 0,
			ch4_lfsr: 0x7FFF,
			sample_rate: 48_000,
			sample_buffer: Vec::new(),
			sample_cycle_accum: 0.0,
			debug_capture_cycle_accum: 0,
			channel_debug_history: [ChannelDebugSamples::default(); 4],
			master_debug_history: ChannelDebugSamples::default(),
			channel_muted: [false; 4],
		}
	}

	pub(crate) fn step(&mut self, t_cycles: u64) {
		if !self.powered() {
			return;
		}

		self.frame_seq_cycle_accum = self.frame_seq_cycle_accum.wrapping_add(t_cycles);
		while self.frame_seq_cycle_accum >= 8192 {
			self.frame_seq_cycle_accum -= 8192;
			self.frame_sequencer_step();
			self.frame_seq_step = (self.frame_seq_step + 1) & 0x07;
		}

		self.step_wave_generators(t_cycles);
		self.capture_debug_samples(t_cycles);
		self.generate_samples(t_cycles);

		self.update_nr52_status();
	}

	pub(crate) fn set_sample_rate(&mut self, sample_rate: u32) {
		self.sample_rate = sample_rate.max(8_000);
	}

	pub(crate) fn drain_samples(&mut self) -> Vec<f32> {
		std::mem::take(&mut self.sample_buffer)
	}

	pub(crate) fn read(&self, addr: u16) -> u8 {
		match addr {
			NR10..=NR52 => {
				if addr == NR52 {
					return NR52_READ_MASK | (self.nr52 & 0x8F);
				}

				let val = self.regs[(addr - NR10) as usize];
				val | read_mask(addr)
			}
			WAVE_RAM_START..=WAVE_RAM_END => self.wave_ram[(addr - WAVE_RAM_START) as usize],
			CGB_PCM12 | CGB_PCM34 => 0,
			_ => 0xFF,
		}
	}

	pub(crate) fn write(&mut self, addr: u16, value: u8) {
		if addr == NR52 {
			if value & 0x80 == 0 {
				self.nr52 = 0;
				self.regs.fill(0);
				self.frame_seq_cycle_accum = 0;
				self.frame_seq_step = 0;
				self.ch1_timer = 0;
				self.ch2_timer = 0;
				self.ch3_timer = 0;
				self.ch4_timer = 0;
				self.ch1_duty_pos = 0;
				self.ch2_duty_pos = 0;
				self.ch3_wave_pos = 0;
				self.ch4_lfsr = 0x7FFF;
				self.sample_cycle_accum = 0.0;
				self.sample_buffer.clear();
				self.debug_capture_cycle_accum = 0;
				for history in &mut self.channel_debug_history {
					history.clear();
				}
				self.master_debug_history.clear();
				self.channel_muted = [false; 4];
				for channel in &mut self.channels {
					channel.enabled = false;
					channel.length_enabled = false;
					channel.length_counter = 0;
				}
			} else {
				self.nr52 |= 0x80;
				self.update_nr52_status();
			}
			return;
		}

		if !self.powered() {
			if (WAVE_RAM_START..=WAVE_RAM_END).contains(&addr) {
				self.wave_ram[(addr - WAVE_RAM_START) as usize] = value;
			}
			return;
		}

		match addr {
			NR10..=NR51 => {
				self.regs[(addr - NR10) as usize] = value;
				self.maybe_write_length(addr, value);
				let length_enable_clocked = self.maybe_write_length_enable(addr, value);
				self.maybe_write_sweep(addr, value);
				self.maybe_write_envelope(addr, value);
				self.maybe_apply_dac_gate(addr);

				if value & 0x80 != 0 {
					if let Some((channel_index, channel_mask)) = trigger_channel(addr) {
						self.channels[channel_index].enabled = self.channel_dac_enabled(channel_index);
						self.reset_channel_runtime(channel_index);
						if channel_index == 0 {
							self.init_sweep_on_trigger();
						}
						if uses_envelope(channel_index) {
							self.channels[channel_index].envelope_volume = envelope_initial_volume(self.regs[envelope_reg_index(channel_index)]);
							self.channels[channel_index].envelope_timer = envelope_period_or_8(self.channels[channel_index].envelope_period);
						}
						if self.channels[channel_index].length_counter == 0 {
							self.channels[channel_index].length_counter = channel_max_length(channel_index);
							if self.channels[channel_index].length_enabled
								&& self.frame_seq_step_is_odd()
								&& !length_enable_clocked
							{
								self.channels[channel_index].length_counter -= 1;
							}
						}
						self.nr52 |= channel_mask;
						self.update_nr52_status();
					}
				}
			}
			WAVE_RAM_START..=WAVE_RAM_END => {
				self.wave_ram[(addr - WAVE_RAM_START) as usize] = value;
			}
			_ => {}
		}
	}

	fn powered(&self) -> bool {
		(self.nr52 & 0x80) != 0
	}

	pub(crate) fn regs_snapshot(&self) -> [u8; 0x17] {
		self.regs
	}

	pub(crate) fn wave_ram_snapshot(&self) -> [u8; 0x10] {
		self.wave_ram
	}

	pub(crate) fn nr52_raw(&self) -> u8 {
		self.nr52
	}

	pub(crate) fn channel_debug_samples(&self, channel: usize) -> &[f32; DEBUG_SAMPLE_HISTORY_LEN] {
		&self.channel_debug_history[channel.min(3)].samples
	}

	pub(crate) fn channel_debug_samples_ordered(&self, channel: usize) -> [f32; DEBUG_SAMPLE_HISTORY_LEN] {
		self.channel_debug_history[channel.min(3)].ordered()
	}

	pub(crate) fn master_debug_samples_ordered(&self) -> [f32; DEBUG_SAMPLE_HISTORY_LEN] {
		self.master_debug_history.ordered()
	}

	pub(crate) fn channel_mutes(&self) -> [bool; 4] {
		self.channel_muted
	}

	pub(crate) fn set_channel_mutes(&mut self, mutes: [bool; 4]) {
		self.channel_muted = mutes;
	}

	fn frame_sequencer_step(&mut self) {
		let step = self.frame_seq_step;
		if matches!(step, 0 | 2 | 4 | 6) {
			self.clock_length();
		}
		if matches!(step, 2 | 6) {
			self.clock_sweep();
		}
		if step == 7 {
			self.clock_envelope();
		}
	}

	fn clock_length(&mut self) {
		for channel in &mut self.channels {
			if channel.length_enabled && channel.length_counter > 0 {
				channel.length_counter -= 1;
				if channel.length_counter == 0 {
					channel.enabled = false;
				}
			}
		}
		self.update_nr52_status();
	}

	fn clock_sweep(&mut self) {
		let (current_shadow, shift, negate) = {
			let ch1 = &mut self.channels[0];
			if !ch1.enabled || !ch1.sweep_enabled {
				return;
			}

			if ch1.sweep_timer > 0 {
				ch1.sweep_timer -= 1;
			}
			if ch1.sweep_timer != 0 {
				return;
			}

			ch1.sweep_timer = sweep_period_or_8(ch1.sweep_period);

			(ch1.sweep_shadow_freq, ch1.sweep_shift, ch1.sweep_negate)
		};

		let Some(new_freq) = sweep_calculation(current_shadow, shift, negate) else {
			let ch1 = &mut self.channels[0];
			ch1.enabled = false;
			ch1.sweep_enabled = false;
			self.update_nr52_status();
			return;
		};

		if shift > 0 {
			self.set_ch1_frequency(new_freq);
			let overflow = sweep_calculation(new_freq, shift, negate).is_none();
			let ch1 = &mut self.channels[0];
			if negate {
				ch1.sweep_negate_used = true;
			}
			ch1.sweep_shadow_freq = new_freq;
			if overflow {
				ch1.enabled = false;
				ch1.sweep_enabled = false;
				self.update_nr52_status();
			}
		}
	}

	fn clock_envelope(&mut self) {
		for &channel_index in &[0usize, 1, 3] {
			let channel = &mut self.channels[channel_index];
			if !channel.enabled || channel.envelope_period == 0 {
				continue;
			}

			if channel.envelope_timer > 0 {
				channel.envelope_timer -= 1;
			}

			if channel.envelope_timer == 0 {
				channel.envelope_timer = envelope_period_or_8(channel.envelope_period);
				if channel.envelope_increase {
					if channel.envelope_volume < 0x0F {
						channel.envelope_volume += 1;
					}
				} else if channel.envelope_volume > 0 {
					channel.envelope_volume -= 1;
				}
			}
		}
	}

	fn update_nr52_status(&mut self) {
		let mut active_bits = 0u8;
		for (i, channel) in self.channels.iter().enumerate() {
			if channel.enabled {
				active_bits |= 1 << i;
			}
		}
		self.nr52 = (self.nr52 & 0x80) | active_bits;
	}

	fn channel_dac_enabled(&self, channel_index: usize) -> bool {
		match channel_index {
			0 => (self.regs[(NR12 - NR10) as usize] & 0xF8) != 0,
			1 => (self.regs[(NR22 - NR10) as usize] & 0xF8) != 0,
			2 => (self.regs[(NR30 - NR10) as usize] & 0x80) != 0,
			3 => (self.regs[(NR42 - NR10) as usize] & 0xF8) != 0,
			_ => false,
		}
	}

	fn maybe_apply_dac_gate(&mut self, addr: u16) {
		let channel_index = match addr {
			NR12 => Some(0usize),
			NR22 => Some(1usize),
			NR30 => Some(2usize),
			NR42 => Some(3usize),
			_ => None,
		};

		if let Some(channel_index) = channel_index {
			if !self.channel_dac_enabled(channel_index) {
				self.channels[channel_index].enabled = false;
				if channel_index == 0 {
					self.channels[0].sweep_enabled = false;
				}
				self.update_nr52_status();
			}
		}
	}

	fn maybe_write_length(&mut self, addr: u16, value: u8) {
		if let Some(channel_index) = length_channel_from_addr(addr) {
			let max_length = channel_max_length(channel_index);
			let length_data = match addr {
				NR31 => value as u16,
				_ => (value & 0x3F) as u16,
			};
			self.channels[channel_index].length_counter = max_length.saturating_sub(length_data);
		}
	}

	fn maybe_write_length_enable(&mut self, addr: u16, value: u8) -> bool {
		if let Some(channel_index) = trigger_channel(addr).map(|(idx, _)| idx) {
			let was_enabled = self.channels[channel_index].length_enabled;
			let now_enabled = (value & 0x40) != 0;
			self.channels[channel_index].length_enabled = now_enabled;

			if !was_enabled && now_enabled && self.frame_seq_step_is_odd() {
				let clocked = self.clock_length_channel(channel_index);
				self.update_nr52_status();
				return clocked;
			}
		}

		false
	}

	fn clock_length_channel(&mut self, channel_index: usize) -> bool {
		let channel = &mut self.channels[channel_index];
		if channel.length_enabled && channel.length_counter > 0 {
			channel.length_counter -= 1;
			if channel.length_counter == 0 {
				channel.enabled = false;
			}
			return true;
		}

		false
	}

	fn frame_seq_step_is_odd(&self) -> bool {
		(self.frame_seq_step & 0x01) != 0
	}

	fn maybe_write_sweep(&mut self, addr: u16, value: u8) {
		if addr != NR10 {
			return;
		}

		let new_negate = (value & 0x08) != 0;
		let mut disable_channel = false;
		{
			let ch1 = &mut self.channels[0];
			if ch1.sweep_negate && !new_negate && ch1.sweep_negate_used {
				ch1.enabled = false;
				ch1.sweep_enabled = false;
				disable_channel = true;
			}

			ch1.sweep_period = (value >> 4) & 0x07;
			ch1.sweep_negate = new_negate;
			ch1.sweep_shift = value & 0x07;
		}

		if disable_channel {
			self.update_nr52_status();
		}
	}

	fn maybe_write_envelope(&mut self, addr: u16, value: u8) {
		if let Some(channel_index) = envelope_channel_from_addr(addr) {
			self.channels[channel_index].envelope_period = value & 0x07;
			self.channels[channel_index].envelope_increase = (value & 0x08) != 0;
			self.channels[channel_index].envelope_volume = envelope_initial_volume(value);
		}
	}

	fn init_sweep_on_trigger(&mut self) {
		let current_freq = self.ch1_frequency();
		let ch1 = &mut self.channels[0];
		ch1.sweep_shadow_freq = current_freq;
		ch1.sweep_timer = sweep_period_or_8(ch1.sweep_period);
		ch1.sweep_enabled = ch1.sweep_period != 0 || ch1.sweep_shift != 0;
		ch1.sweep_negate_used = false;
		if ch1.sweep_shift > 0 && sweep_calculation(current_freq, ch1.sweep_shift, ch1.sweep_negate).is_none() {
			ch1.enabled = false;
			ch1.sweep_enabled = false;
		}
	}

	fn ch1_frequency(&self) -> u16 {
		let low = self.regs[(NR13 - NR10) as usize] as u16;
		let high = (self.regs[(NR14 - NR10) as usize] as u16) & 0x07;
		(high << 8) | low
	}

	fn set_ch1_frequency(&mut self, freq: u16) {
		let idx13 = (NR13 - NR10) as usize;
		let idx14 = (NR14 - NR10) as usize;
		self.regs[idx13] = (freq & 0xFF) as u8;
		self.regs[idx14] = (self.regs[idx14] & !0x07) | ((freq >> 8) as u8 & 0x07);
	}

	fn reset_channel_runtime(&mut self, channel_index: usize) {
		match channel_index {
			0 => {
				self.ch1_duty_pos = 0;
				self.ch1_timer = self.square_period_t_cycles(self.ch1_frequency());
			}
			1 => {
				self.ch2_duty_pos = 0;
				self.ch2_timer = self.square_period_t_cycles(self.ch2_frequency());
			}
			2 => {
				self.ch3_wave_pos = 0;
				self.ch3_timer = self.wave_period_t_cycles(self.ch3_frequency());
			}
			3 => {
				self.ch4_lfsr = 0x7FFF;
				self.ch4_timer = self.noise_period_t_cycles();
			}
			_ => {}
		}
	}

	fn step_wave_generators(&mut self, t_cycles: u64) {
		self.advance_square_channel(0, t_cycles);
		self.advance_square_channel(1, t_cycles);
		self.advance_wave_channel(t_cycles);
		self.advance_noise_channel(t_cycles);
	}

	fn advance_square_channel(&mut self, channel_index: usize, t_cycles: u64) {
		if !self.channels[channel_index].enabled {
			return;
		}

		let period = if channel_index == 0 {
			self.square_period_t_cycles(self.ch1_frequency())
		} else {
			self.square_period_t_cycles(self.ch2_frequency())
		};

		let timer = if channel_index == 0 {
			&mut self.ch1_timer
		} else {
			&mut self.ch2_timer
		};
		if *timer == 0 {
			*timer = period;
		}

		let mut remaining = t_cycles;
		while remaining >= *timer {
			remaining -= *timer;
			*timer = period;
			if channel_index == 0 {
				self.ch1_duty_pos = (self.ch1_duty_pos + 1) & 0x07;
			} else {
				self.ch2_duty_pos = (self.ch2_duty_pos + 1) & 0x07;
			}
		}
		*timer -= remaining;
	}

	fn advance_wave_channel(&mut self, t_cycles: u64) {
		if !self.channels[2].enabled {
			return;
		}

		let period = self.wave_period_t_cycles(self.ch3_frequency());
		if self.ch3_timer == 0 {
			self.ch3_timer = period;
		}

		let mut remaining = t_cycles;
		while remaining >= self.ch3_timer {
			remaining -= self.ch3_timer;
			self.ch3_timer = period;
			self.ch3_wave_pos = (self.ch3_wave_pos + 1) & 0x1F;
		}
		self.ch3_timer -= remaining;
	}

	fn advance_noise_channel(&mut self, t_cycles: u64) {
		if !self.channels[3].enabled {
			return;
		}

		let period = self.noise_period_t_cycles();
		if self.ch4_timer == 0 {
			self.ch4_timer = period;
		}

		let mut remaining = t_cycles;
		while remaining >= self.ch4_timer {
			remaining -= self.ch4_timer;
			self.ch4_timer = period;

			let xor = (self.ch4_lfsr & 0x01) ^ ((self.ch4_lfsr >> 1) & 0x01);
			self.ch4_lfsr = (self.ch4_lfsr >> 1) | (xor << 14);
			if (self.regs[(NR43 - NR10) as usize] & 0x08) != 0 {
				self.ch4_lfsr = (self.ch4_lfsr & !(1 << 6)) | (xor << 6);
			}
		}
		self.ch4_timer -= remaining;
	}

	fn generate_samples(&mut self, t_cycles: u64) {
		let cycles_per_sample = APU_T_CYCLES_PER_SECOND / self.sample_rate as f64;
		self.sample_cycle_accum += t_cycles as f64;
		while self.sample_cycle_accum >= cycles_per_sample {
			self.sample_cycle_accum -= cycles_per_sample;
			let (left, right) = self.mix_sample();
			self.sample_buffer.push(left);
			self.sample_buffer.push(right);
		}
	}

	fn mix_sample(&self) -> (f32, f32) {
		if !self.powered() {
			return (0.0, 0.0);
		}

		let nr50 = self.regs[(NR50 - NR10) as usize];
		let nr51 = self.regs[(NR51 - NR10) as usize];
		let left_master = ((nr50 >> 4) & 0x07) as f32 / 7.0;
		let right_master = (nr50 & 0x07) as f32 / 7.0;

		let mut channel_samples = [
			self.ch1_sample(),
			self.ch2_sample(),
			self.ch3_sample(),
			self.ch4_sample(),
		];
		for (sample, muted) in channel_samples.iter_mut().zip(self.channel_muted.iter()) {
			if *muted {
				*sample = 0.0;
			}
		}

		let mut left = 0.0f32;
		let mut right = 0.0f32;
		for (i, sample) in channel_samples.iter().enumerate() {
			if (nr51 & (1 << i)) != 0 {
				right += *sample;
			}
			if (nr51 & (1 << (i + 4))) != 0 {
				left += *sample;
			}
		}

		left = (left / 4.0) * left_master;
		right = (right / 4.0) * right_master;
		(left.clamp(-1.0, 1.0), right.clamp(-1.0, 1.0))
	}

	fn channel_raw_samples(&self) -> [f32; 4] {
		[
			self.ch1_sample(),
			self.ch2_sample(),
			self.ch3_sample(),
			self.ch4_sample(),
		]
	}

	fn capture_debug_samples(&mut self, t_cycles: u64) {
		self.debug_capture_cycle_accum = self.debug_capture_cycle_accum.saturating_add(t_cycles);
		while self.debug_capture_cycle_accum >= DEBUG_CAPTURE_DECIMATION_T_CYCLES {
			self.debug_capture_cycle_accum -= DEBUG_CAPTURE_DECIMATION_T_CYCLES;
			let channels = self.channel_raw_samples();
			for (history, sample) in self.channel_debug_history.iter_mut().zip(channels.iter()) {
				history.push(*sample);
			}
			let (left, right) = self.mix_sample();
			self.master_debug_history.push((left + right) * 0.5);
		}
	}

	fn ch1_sample(&self) -> f32 {
		self.square_sample(0, self.ch1_duty_pos, self.regs[(NR11 - NR10) as usize])
	}

	fn ch2_sample(&self) -> f32 {
		self.square_sample(1, self.ch2_duty_pos, self.regs[(NR21 - NR10) as usize])
	}

	fn square_sample(&self, channel_index: usize, duty_pos: u8, duty_reg: u8) -> f32 {
		if !self.channels[channel_index].enabled {
			return 0.0;
		}
		let duty = ((duty_reg >> 6) & 0x03) as usize;
		let high = DUTY_TABLE[duty][duty_pos as usize];
		let volume = self.channels[channel_index].envelope_volume as f32 / 15.0;
		if high { volume } else { -volume }
	}

	fn ch3_sample(&self) -> f32 {
		if !self.channels[2].enabled || (self.regs[(NR30 - NR10) as usize] & 0x80) == 0 {
			return 0.0;
		}

		let wave_byte = self.wave_ram[(self.ch3_wave_pos / 2) as usize];
		let raw = if (self.ch3_wave_pos & 1) == 0 {
			wave_byte >> 4
		} else {
			wave_byte & 0x0F
		};

		let scaled = match (self.regs[(NR32 - NR10) as usize] >> 5) & 0x03 {
			0 => 0,
			1 => raw,
			2 => raw >> 1,
			_ => raw >> 2,
		};

		(scaled as f32 / 15.0) * 2.0 - 1.0
	}

	fn ch4_sample(&self) -> f32 {
		if !self.channels[3].enabled {
			return 0.0;
		}
		let volume = self.channels[3].envelope_volume as f32 / 15.0;
		if (self.ch4_lfsr & 0x01) == 0 { volume } else { -volume }
	}

	fn square_period_t_cycles(&self, freq: u16) -> u64 {
		let base = 2048u16.saturating_sub(freq.max(1));
		u64::from(base.max(1)) * 4
	}

	fn wave_period_t_cycles(&self, freq: u16) -> u64 {
		let base = 2048u16.saturating_sub(freq.max(1));
		u64::from(base.max(1)) * 2
	}

	fn noise_period_t_cycles(&self) -> u64 {
		let nr43 = self.regs[(NR43 - NR10) as usize];
		let shift = (nr43 >> 4) & 0x0F;
		let divisor_code = nr43 & 0x07;
		let divisor = match divisor_code {
			0 => 8u32,
			1 => 16,
			2 => 32,
			3 => 48,
			4 => 64,
			5 => 80,
			6 => 96,
			_ => 112,
		};
		u64::from(divisor << shift).max(8)
	}

	fn ch2_frequency(&self) -> u16 {
		let low = self.regs[(NR23 - NR10) as usize] as u16;
		let high = (self.regs[(NR24 - NR10) as usize] as u16) & 0x07;
		(high << 8) | low
	}

	fn ch3_frequency(&self) -> u16 {
		let low = self.regs[(NR33 - NR10) as usize] as u16;
		let high = (self.regs[(NR34 - NR10) as usize] as u16) & 0x07;
		(high << 8) | low
	}
}

fn trigger_channel(addr: u16) -> Option<(usize, u8)> {
	match addr {
		NR14 => Some((0, 0x01)),
		NR24 => Some((1, 0x02)),
		NR34 => Some((2, 0x04)),
		NR44 => Some((3, 0x08)),
		_ => None,
	}
}

fn length_channel_from_addr(addr: u16) -> Option<usize> {
	match addr {
		NR11 => Some(0),
		NR21 => Some(1),
		NR31 => Some(2),
		NR41 => Some(3),
		_ => None,
	}
}

fn channel_max_length(channel_index: usize) -> u16 {
	match channel_index {
		0 | 1 | 3 => 64,
		2 => 256,
		_ => 64,
	}
}

fn uses_envelope(channel_index: usize) -> bool {
	matches!(channel_index, 0 | 1 | 3)
}

fn envelope_channel_from_addr(addr: u16) -> Option<usize> {
	match addr {
		NR12 => Some(0),
		NR22 => Some(1),
		NR42 => Some(3),
		_ => None,
	}
}

fn envelope_reg_index(channel_index: usize) -> usize {
	match channel_index {
		0 => (NR12 - NR10) as usize,
		1 => (NR22 - NR10) as usize,
		3 => (NR42 - NR10) as usize,
		_ => 0,
	}
}

fn envelope_initial_volume(reg: u8) -> u8 {
	(reg >> 4) & 0x0F
}

fn envelope_period_or_8(period: u8) -> u8 {
	if period == 0 { 8 } else { period }
}

fn sweep_period_or_8(period: u8) -> u8 {
	if period == 0 { 8 } else { period }
}

fn sweep_calculation(shadow_freq: u16, shift: u8, negate: bool) -> Option<u16> {
	if shift == 0 {
		return Some(shadow_freq);
	}
	let delta = shadow_freq >> shift;
	if negate {
		shadow_freq.checked_sub(delta)
	} else {
		let next = shadow_freq.saturating_add(delta);
		if next > 2047 { None } else { Some(next) }
	}
}

fn read_mask(addr: u16) -> u8 {
	match addr {
		NR10 => NR10_READ_MASK,
		NR11 => NR11_READ_MASK,
		NR12 => NR12_READ_MASK,
		NR13 => NR13_READ_MASK,
		NR14 => NR14_READ_MASK,
		0xFF15 => NR15_READ_MASK,
		NR21 => NR21_READ_MASK,
		NR22 => NR22_READ_MASK,
		NR23 => NR23_READ_MASK,
		NR24 => NR24_READ_MASK,
		NR30 => NR30_READ_MASK,
		NR31 => NR31_READ_MASK,
		NR32 => NR32_READ_MASK,
		NR33 => NR33_READ_MASK,
		NR34 => NR34_READ_MASK,
		0xFF1F => NR35_READ_MASK,
		NR41 => NR41_READ_MASK,
		NR42 => NR42_READ_MASK,
		NR43 => NR43_READ_MASK,
		NR44 => NR44_READ_MASK,
		NR50 => NR50_READ_MASK,
		NR51 => NR51_READ_MASK,
		_ => 0xFF,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn frame_sequencer_advances_every_8192_t_cycles() {
		let mut apu = Apu::new();
		apu.write(NR52, 0x80);
		assert_eq!(apu.frame_seq_step, 0);

		apu.step(8191);
		assert_eq!(apu.frame_seq_step, 0);

		apu.step(1);
		assert_eq!(apu.frame_seq_step, 1);

		apu.step(8192 * 3);
		assert_eq!(apu.frame_seq_step, 4);
	}

	#[test]
	fn step_does_not_advance_when_powered_off() {
		let mut apu = Apu::new();
		apu.step(8192 * 4);
		assert_eq!(apu.frame_seq_step, 0);
		assert_eq!(apu.nr52_raw() & 0x80, 0);
	}

	#[test]
	fn power_off_resets_frame_sequencer_state() {
		let mut apu = Apu::new();
		apu.write(NR52, 0x80);
		apu.step(8192 * 2 + 17);
		assert_eq!(apu.frame_seq_step, 2);
		assert_eq!(apu.frame_seq_cycle_accum, 17);

		apu.write(NR52, 0x00);
		assert_eq!(apu.frame_seq_step, 0);
		assert_eq!(apu.frame_seq_cycle_accum, 0);
	}

	#[test]
	fn trigger_reloads_zero_length_counter() {
		let mut apu = Apu::new();
		apu.write(NR52, 0x80);
		apu.write(NR12, 0xF0);
		apu.write(NR14, 0x80);
		assert_eq!(apu.channels[0].length_counter, 64);
		assert_eq!(apu.nr52_raw() & 0x01, 0x01);
	}

	#[test]
	fn length_tick_requires_length_enable() {
		let mut apu = Apu::new();
		apu.write(NR52, 0x80);
		apu.write(NR12, 0xF0);
		apu.write(NR11, 0x3F);
		apu.write(NR14, 0x80);

		apu.step(8192);

		assert_eq!(apu.nr52_raw() & 0x01, 0x01);
		assert_eq!(apu.channels[0].length_counter, 1);
	}

	#[test]
	fn length_tick_disables_channel_when_enabled_and_counter_expires() {
		let mut apu = Apu::new();
		apu.write(NR52, 0x80);
		apu.write(NR12, 0xF0);
		apu.write(NR11, 0x3F);
		apu.write(NR14, 0xC0);

		apu.step(8192); // step 0 clocks length

		assert_eq!(apu.channels[0].length_counter, 0);
		assert_eq!(apu.nr52_raw() & 0x01, 0x00);
	}

	#[test]
	fn envelope_ticks_on_step_7_for_channel_1() {
		let mut apu = Apu::new();
		apu.write(NR52, 0x80);
		apu.write(NR12, 0x19);
		apu.write(NR14, 0x80);

		apu.frame_seq_step = 7;
		apu.frame_sequencer_step();

		assert_eq!(apu.channels[0].envelope_volume, 2);
	}

	#[test]
	fn envelope_decrease_clamps_at_zero() {
		let mut apu = Apu::new();
		apu.write(NR52, 0x80);
		apu.write(NR12, 0x01);
		apu.write(NR14, 0x80);

		apu.frame_seq_step = 7;
		apu.frame_sequencer_step();

		assert_eq!(apu.channels[0].envelope_volume, 0);
	}

	#[test]
	fn sweep_tick_updates_ch1_frequency() {
		let mut apu = Apu::new();
		apu.write(NR52, 0x80);
		apu.write(NR12, 0xF0);
		apu.write(NR10, 0x11);
		apu.write(NR13, 100);
		apu.write(NR14, 0x80);

		apu.frame_seq_step = 2;
		apu.frame_sequencer_step();

		assert_eq!(apu.ch1_frequency(), 150);
	}

	#[test]
	fn sweep_overflow_disables_channel_1() {
		let mut apu = Apu::new();
		apu.write(NR52, 0x80);
		apu.write(NR12, 0xF0);
		apu.write(NR10, 0x11);
		apu.write(NR13, 0xF8);
		apu.write(NR14, 0x87);

		apu.frame_seq_step = 2;
		apu.frame_sequencer_step();

		assert_eq!(apu.nr52_raw() & 0x01, 0x00);
	}

	#[test]
	fn ch1_trigger_with_dac_off_does_not_enable_channel() {
		let mut apu = Apu::new();
		apu.write(NR52, 0x80);
		apu.write(NR12, 0x00);
		apu.write(NR14, 0x80);

		assert_eq!(apu.nr52_raw() & 0x01, 0x00);
	}

	#[test]
	fn ch1_dac_off_write_disables_active_channel() {
		let mut apu = Apu::new();
		apu.write(NR52, 0x80);
		apu.write(NR12, 0xF0);
		apu.write(NR14, 0x80);
		assert_eq!(apu.nr52_raw() & 0x01, 0x01);

		apu.write(NR12, 0x00);
		assert_eq!(apu.nr52_raw() & 0x01, 0x00);
	}

	#[test]
	fn ch3_trigger_requires_dac_enable() {
		let mut apu = Apu::new();
		apu.write(NR52, 0x80);

		apu.write(NR30, 0x00);
		apu.write(NR34, 0x80);
		assert_eq!(apu.nr52_raw() & 0x04, 0x00);

		apu.write(NR30, 0x80);
		apu.write(NR34, 0x80);
		assert_eq!(apu.nr52_raw() & 0x04, 0x04);
	}

	#[test]
	fn ch3_dac_off_write_disables_active_channel() {
		let mut apu = Apu::new();
		apu.write(NR52, 0x80);
		apu.write(NR30, 0x80);
		apu.write(NR34, 0x80);
		assert_eq!(apu.nr52_raw() & 0x04, 0x04);

		apu.write(NR30, 0x00);
		assert_eq!(apu.nr52_raw() & 0x04, 0x00);
	}

	#[test]
	fn sweep_period_zero_still_ticks_with_period_8() {
		let mut apu = Apu::new();
		apu.write(NR52, 0x80);
		apu.write(NR12, 0xF0);
		apu.write(NR10, 0x01);
		apu.write(NR13, 100);
		apu.write(NR14, 0x80);

		apu.frame_seq_step = 2;
		for _ in 0..7 {
			apu.frame_sequencer_step();
		}
		assert_eq!(apu.ch1_frequency(), 100);

		apu.frame_sequencer_step();
		assert_eq!(apu.ch1_frequency(), 150);
	}

	#[test]
	fn clearing_sweep_negate_after_subtraction_disables_ch1() {
		let mut apu = Apu::new();
		apu.write(NR52, 0x80);
		apu.write(NR12, 0xF0);
		apu.write(NR10, 0x19);
		apu.write(NR13, 100);
		apu.write(NR14, 0x80);

		apu.frame_seq_step = 2;
		apu.frame_sequencer_step();
		assert_eq!(apu.nr52_raw() & 0x01, 0x01);

		apu.write(NR10, 0x11);
		assert_eq!(apu.nr52_raw() & 0x01, 0x00);
	}

	#[test]
	fn length_enable_rising_edge_on_odd_step_immediately_clocks_length() {
		let mut apu = Apu::new();
		apu.write(NR52, 0x80);
		apu.write(NR12, 0xF0);
		apu.write(NR11, 0x3F);
		apu.write(NR14, 0x80);
		assert_eq!(apu.channels[0].length_counter, 1);
		assert_eq!(apu.nr52_raw() & 0x01, 0x01);

		apu.frame_seq_step = 1;
		apu.write(NR14, 0x40);

		assert_eq!(apu.channels[0].length_counter, 0);
		assert_eq!(apu.nr52_raw() & 0x01, 0x00);
	}

	#[test]
	fn length_enable_rising_edge_on_even_step_does_not_clock_immediately() {
		let mut apu = Apu::new();
		apu.write(NR52, 0x80);
		apu.write(NR12, 0xF0);
		apu.write(NR11, 0x3F);
		apu.write(NR14, 0x80);

		apu.frame_seq_step = 0;
		apu.write(NR14, 0x40);

		assert_eq!(apu.channels[0].length_counter, 1);
		assert_eq!(apu.nr52_raw() & 0x01, 0x01);
	}

	#[test]
	fn trigger_with_zero_length_and_length_enable_on_odd_step_loads_max_minus_one() {
		let mut apu = Apu::new();
		apu.write(NR52, 0x80);
		apu.write(NR12, 0xF0);
		apu.frame_seq_step = 1;

		apu.write(NR14, 0xC0);

		assert_eq!(apu.channels[0].length_counter, 63);
		assert_eq!(apu.nr52_raw() & 0x01, 0x01);
	}

	#[test]
	fn debug_waveform_capture_advances_for_enabled_channel() {
		let mut apu = Apu::new();
		apu.write(NR52, 0x80);
		apu.write(NR50, 0x77);
		apu.write(NR51, 0x11);
		apu.write(NR12, 0xF0);
		apu.write(NR11, 0x80);
		apu.write(NR14, 0x80);

		apu.step(64 * 16);

		let ordered = apu.channel_debug_samples_ordered(0);
		assert!(ordered.iter().any(|sample| sample.abs() > 0.0001));
	}

	#[test]
	fn channel_mute_only_affects_audio_mix_output() {
		let mut apu = Apu::new();
		apu.write(NR52, 0x80);
		apu.write(NR50, 0x77);
		apu.write(NR51, 0x11);
		apu.write(NR12, 0xF0);
		apu.write(NR11, 0x80);
		apu.write(NR14, 0x80);

		let (left_on, right_on) = apu.mix_sample();
		assert!(left_on.abs() > 0.0 || right_on.abs() > 0.0);

		apu.set_channel_mutes([true, false, false, false]);
		let (left_muted, right_muted) = apu.mix_sample();
		assert_eq!(left_muted, 0.0);
		assert_eq!(right_muted, 0.0);
		assert_eq!(apu.nr52_raw() & 0x01, 0x01);
	}
}

