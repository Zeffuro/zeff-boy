mod chip_constants;
mod constants;
mod dac;
mod envelope_clock;
mod idle;
mod lfo;
mod operator;
mod publication;
mod scheduler;
mod state;
mod tables;
mod timer;
mod voice;
use chip_constants::*;
use envelope_clock::EnvelopeClock;
use lfo::Lfo;
use scheduler::Scheduler;
use timer::{TimerA, TimerB};
use voice::Channel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opn2 {
    channels: [Channel; CHANNELS],
    address: u16,
    frequency_high: u8,
    special_high: u8,
    special: [(u16, u8); 3],
    timer_a: TimerA,
    timer_b: TimerB,
    mode: u8,
    flags: u8,
    busy: u64,
    phase: u64,
    dac: u8,
    dac_enabled: bool,
    output: [i32; CHANNELS],
    output_pan: [u8; CHANNELS],
    lfo: Lfo,
    envelope_clock: EnvelopeClock,
    scheduler: Scheduler,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opn2DebugSnapshot {
    pub address: u16,
    pub status: u8,
    pub timer_a: u16,
    pub timer_b: u16,
    pub mode: u8,
    pub dac: u8,
    pub dac_enabled: bool,
    pub output: [i32; CHANNELS],
}

impl Default for Opn2 {
    fn default() -> Self {
        Self {
            channels: [Channel::default(); CHANNELS],
            address: 0,
            frequency_high: 0,
            special_high: 0,
            special: [(0, 0); 3],
            timer_a: TimerA::default(),
            timer_b: TimerB::default(),
            mode: 0,
            flags: 0,
            busy: 0,
            phase: 0,
            dac: 0x80,
            dac_enabled: false,
            output: [0; CHANNELS],
            output_pan: [PAN_LEFT | PAN_RIGHT; CHANNELS],
            lfo: Lfo::default(),
            envelope_clock: EnvelopeClock::default(),
            scheduler: Scheduler::default(),
        }
    }
}

impl Opn2 {
    pub fn write_port(&mut self, port: u8, value: u8) {
        if port & 1 == 0 {
            self.address = u16::from(port & 2) << 7 | u16::from(value);
            return;
        }
        self.busy = BUSY_CLOCKS;
        let bank = usize::from(self.address >> 8);
        let address = self.address as u8;
        if bank == 0 {
            match address {
                0x22 => {
                    self.lfo.write(value);
                    return;
                }
                0x24 => {
                    self.timer_a.reload = (self.timer_a.reload & 3) | u16::from(value) << 2;
                    return;
                }
                0x25 => {
                    self.timer_a.reload = (self.timer_a.reload & !3) | u16::from(value & 3);
                    return;
                }
                0x26 => {
                    self.timer_b.reload = value;
                    return;
                }
                0x27 => {
                    self.timer_a.clear(value & 0x10 != 0);
                    self.timer_b.clear(value & 0x20 != 0);
                    self.mode = value & !0x30;
                    return;
                }
                0x28 => {
                    self.scheduler.key(value);
                    return;
                }
                0x2a => {
                    self.dac = value;
                    return;
                }
                0x2b => {
                    self.dac_enabled = value & DAC_ENABLE != 0;
                    return;
                }
                _ => {}
            }
        }
        if address & 3 == 3 {
            return;
        }
        let channel = bank * CHANNELS_PER_BANK + usize::from(address & 3);
        match address {
            0x30..=0x9f => self.channels[channel].write_operator(address, value),
            0xa4..=0xa6 => self.frequency_high = value & 0x3f,
            0xa0..=0xa2 => self.channels[channel].set_frequency(
                u16::from(self.frequency_high & 7) << 8 | u16::from(value),
                self.frequency_high >> 3,
            ),
            0xac..=0xae if bank == 0 => self.special_high = value & 0x3f,
            0xa8..=0xaa if bank == 0 => {
                let slot = match address & 3 {
                    0 => 2,
                    1 => 0,
                    _ => 1,
                };
                self.special[slot] = (
                    u16::from(self.special_high & 7) << 8 | u16::from(value),
                    self.special_high >> 3,
                );
            }
            0xb0..=0xb2 | 0xb4..=0xb6 => self.channels[channel].write_channel(address, value),
            _ => {}
        }
    }

    pub fn advance(&mut self, clocks: u64) {
        self.advance_timers(clocks);
        let total = clocks / scheduler::SLOT_CLOCKS
            + (clocks % scheduler::SLOT_CLOCKS + self.phase % scheduler::SLOT_CLOCKS)
                / scheduler::SLOT_CLOCKS;
        self.phase = (clocks % FM_CLOCK_DIVIDER + self.phase) % FM_CLOCK_DIVIDER;
        self.advance_slots(total);
    }

    fn advance_timers(&mut self, clocks: u64) {
        self.busy = self.busy.saturating_sub(clocks);
    }

    pub fn status(&self, port: u8) -> u8 {
        if port & 3 == 0 {
            self.flags | if self.busy != 0 { BUSY_STATUS } else { 0 }
        } else {
            0
        }
    }

    pub fn status_after(&self, clocks: u64) -> u8 {
        let mut copy = *self;
        copy.advance_timers(clocks);
        let slots = clocks / scheduler::SLOT_CLOCKS
            + (clocks % scheduler::SLOT_CLOCKS + self.phase % scheduler::SLOT_CLOCKS)
                / scheduler::SLOT_CLOCKS;
        copy.timer_a.advance_slots(
            self.scheduler.step as usize % scheduler::SLOTS,
            slots,
            self.mode,
            &mut copy.flags,
        );
        copy.timer_b.advance_slots(
            self.scheduler.step as usize % scheduler::SLOTS,
            slots,
            self.mode,
            &mut copy.flags,
        );
        copy.status(0)
    }

    pub fn sample_with_mutes(&self, mutes: [bool; CHANNELS]) -> [i32; 2] {
        let mut sample = [0; 2];
        for (index, channel) in self.channels.iter().enumerate() {
            if mutes[index] {
                continue;
            }
            let value = if index == DAC_CHANNEL && self.dac_enabled {
                (i32::from(self.dac) - DAC_CENTER) * 2
            } else {
                dac::quantize(self.output[index])
            };
            let pan = if index == DAC_CHANNEL && self.dac_enabled {
                channel.pan()
            } else {
                self.output_pan[index]
            };
            sample[0] += dac::transfer(value, pan & PAN_LEFT != 0);
            sample[1] += dac::transfer(value, pan & PAN_RIGHT != 0);
        }
        sample
    }

    pub fn debug_snapshot(&self) -> Opn2DebugSnapshot {
        Opn2DebugSnapshot {
            address: self.address,
            status: self.status(0),
            timer_a: self.timer_a.reload,
            timer_b: u16::from(self.timer_b.reload),
            mode: self.mode,
            dac: self.dac,
            dac_enabled: self.dac_enabled,
            output: self.output,
        }
    }
}

#[cfg(test)]
mod chip_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod timer_tests;
