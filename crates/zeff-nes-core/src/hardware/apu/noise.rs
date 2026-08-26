use super::pulse::LENGTH_TABLE;
use crate::hardware::timing::NesTiming;

pub struct Noise {
    pub enabled: bool,
    pub length_counter: u8,

    length_halt: bool,
    constant_volume: bool,
    envelope_volume: u8,

    mode: bool,
    tonal_mode_supported: bool,
    timer_period: u16,
    timer_counter: u16,

    shift_register: u16,

    envelope_start: bool,
    envelope_divider: u8,
    envelope_decay: u8,
    length_halt_override: Option<bool>,
    skip_length_clock: bool,
    timing: NesTiming,
}

#[rustfmt::skip]
const NTSC_NOISE_TIMER_PERIOD_CPU_CYCLES: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160,
    202, 254, 380, 508, 762, 1016, 2034, 4068,
];

#[rustfmt::skip]
const PAL_NOISE_TIMER_PERIOD_CPU_CYCLES: [u16; 16] = [
    4, 8, 14, 30, 60, 88, 118, 148,
    188, 236, 354, 472, 708, 944, 1890, 3778,
];

impl Noise {
    pub fn new() -> Self {
        Self::new_with_timing(NesTiming::Ntsc)
    }

    pub(crate) fn new_with_timing(timing: NesTiming) -> Self {
        Self {
            enabled: false,
            length_counter: 0,
            length_halt: false,
            constant_volume: false,
            envelope_volume: 0,
            mode: false,
            tonal_mode_supported: true,
            timer_period: 0,
            timer_counter: 0,
            shift_register: 1, // must be nonzero
            envelope_start: false,
            envelope_divider: 0,
            envelope_decay: 0,
            length_halt_override: None,
            skip_length_clock: false,
            timing,
        }
    }

    pub fn write(&mut self, offset: u16, val: u8, length_clock_due: bool) {
        match offset {
            0 => {
                if length_clock_due {
                    self.length_halt_override = Some(self.length_halt);
                }
                self.length_halt = val & 0x20 != 0;
                self.constant_volume = val & 0x10 != 0;
                self.envelope_volume = val & 0x0F;
            }
            2 => {
                self.mode = self.tonal_mode_supported && val & 0x80 != 0;
                let periods = match self.timing {
                    NesTiming::Pal => &PAL_NOISE_TIMER_PERIOD_CPU_CYCLES,
                    NesTiming::Ntsc | NesTiming::Dendy => &NTSC_NOISE_TIMER_PERIOD_CPU_CYCLES,
                };
                self.timer_period = periods[(val & 0x0F) as usize];
            }
            3 => {
                let old_length = self.length_counter;
                if self.enabled && (!length_clock_due || old_length == 0) {
                    self.length_counter = LENGTH_TABLE[(val >> 3) as usize];
                }
                self.skip_length_clock = length_clock_due && self.enabled && old_length == 0;
                self.envelope_start = true;
            }
            _ => {}
        }
    }

    pub fn set_tonal_mode_supported(&mut self, supported: bool) {
        self.tonal_mode_supported = supported;
        if !supported {
            self.mode = false;
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.length_counter = 0;
        }
    }

    #[inline]
    pub fn tick(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period;
            let feedback_bit = if self.mode { 6 } else { 1 };
            let feedback = (self.shift_register ^ (self.shift_register >> feedback_bit)) & 1;
            self.shift_register = (self.shift_register >> 1) | (feedback << 14);
        } else {
            self.timer_counter -= 1;
        }
    }

    pub fn clock_envelope(&mut self) {
        if self.envelope_start {
            self.envelope_start = false;
            self.envelope_decay = 15;
            self.envelope_divider = self.envelope_volume;
        } else if self.envelope_divider == 0 {
            self.envelope_divider = self.envelope_volume;
            if self.envelope_decay > 0 {
                self.envelope_decay -= 1;
            } else if self.length_halt {
                self.envelope_decay = 15;
            }
        } else {
            self.envelope_divider -= 1;
        }
    }

    pub fn clock_length(&mut self) {
        let halted = self.length_halt_override.take().unwrap_or(self.length_halt);
        if !self.skip_length_clock && !halted && self.length_counter > 0 {
            self.length_counter -= 1;
        }
        self.skip_length_clock = false;
    }

    pub fn clear_length_clock_collision(&mut self) {
        self.length_halt_override = None;
        self.skip_length_clock = false;
    }

    #[inline]
    pub fn output(&self) -> u8 {
        if !self.enabled || self.length_counter == 0 || self.shift_register & 1 != 0 {
            return 0;
        }
        if self.constant_volume {
            self.envelope_volume
        } else {
            self.envelope_decay
        }
    }

    pub fn midi_active(&self) -> bool {
        self.enabled && self.length_counter > 0
    }

    pub fn midi_volume(&self) -> u8 {
        if self.constant_volume {
            self.envelope_volume
        } else {
            self.envelope_decay
        }
    }

    pub fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bool(self.enabled);
        w.write_u8(self.length_counter);
        w.write_bool(self.length_halt);
        w.write_bool(self.constant_volume);
        w.write_u8(self.envelope_volume);
        w.write_bool(self.mode);
        w.write_u16(self.timer_period);
        w.write_u16(self.timer_counter);
        w.write_u16(self.shift_register);
        w.write_bool(self.envelope_start);
        w.write_u8(self.envelope_divider);
        w.write_u8(self.envelope_decay);
    }

    pub fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.enabled = r.read_bool()?;
        self.length_counter = r.read_u8()?;
        self.length_halt = r.read_bool()?;
        self.constant_volume = r.read_bool()?;
        self.envelope_volume = r.read_u8()?;
        self.mode = r.read_bool()?;
        self.timer_period = r.read_u16()?;
        self.timer_counter = r.read_u16()?;
        self.shift_register = r.read_u16()?;
        self.envelope_start = r.read_bool()?;
        self.envelope_divider = r.read_u8()?;
        self.envelope_decay = r.read_u8()?;
        self.clear_length_clock_collision();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{NTSC_NOISE_TIMER_PERIOD_CPU_CYCLES, Noise, PAL_NOISE_TIMER_PERIOD_CPU_CYCLES};
    use crate::hardware::timing::NesTiming;

    #[test]
    fn tonal_mode_can_be_disabled_for_vs_system_cpu() {
        let mut noise = Noise::new();
        noise.set_tonal_mode_supported(false);

        noise.write(2, 0x80, false);

        assert!(!noise.mode);
    }

    #[test]
    fn tonal_mode_defaults_to_supported_for_nes_cpu() {
        let mut noise = Noise::new();

        noise.write(2, 0x80, false);

        assert!(noise.mode);
    }

    #[test]
    fn region_selects_noise_timer_periods() {
        let mut ntsc = Noise::new_with_timing(NesTiming::Ntsc);
        let mut pal = Noise::new_with_timing(NesTiming::Pal);
        let mut dendy = Noise::new_with_timing(NesTiming::Dendy);

        for index in 0..16 {
            ntsc.write(2, index, false);
            pal.write(2, index, false);
            dendy.write(2, index, false);
            assert_eq!(
                ntsc.timer_period,
                NTSC_NOISE_TIMER_PERIOD_CPU_CYCLES[index as usize]
            );
            assert_eq!(
                pal.timer_period,
                PAL_NOISE_TIMER_PERIOD_CPU_CYCLES[index as usize]
            );
            assert_eq!(
                dendy.timer_period,
                NTSC_NOISE_TIMER_PERIOD_CPU_CYCLES[index as usize]
            );
        }
    }
}
