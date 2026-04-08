use crate::save_state::{StateReader, StateWriter};

const NUM_CHANNELS: usize = 6;

#[rustfmt::skip]
static PATCHES: [[u8; 8]; 15] = [
    [0x03, 0x21, 0x05, 0x06, 0xE8, 0x81, 0x42, 0x27],
    [0x13, 0x41, 0x14, 0x0D, 0xD8, 0xF6, 0x23, 0x12],
    [0x11, 0x11, 0x08, 0x08, 0xFA, 0xB2, 0x20, 0x12],
    [0x31, 0x61, 0x0C, 0x07, 0xA8, 0x64, 0x61, 0x27],
    [0x32, 0x21, 0x1E, 0x06, 0xE1, 0x76, 0x01, 0x28],
    [0x02, 0x01, 0x06, 0x00, 0xA3, 0xE2, 0xF4, 0xF4],
    [0x21, 0x61, 0x1D, 0x07, 0x82, 0x81, 0x11, 0x07],
    [0x23, 0x21, 0x22, 0x17, 0xA2, 0x72, 0x01, 0x17],
    [0x35, 0x11, 0x25, 0x00, 0x40, 0x73, 0x72, 0x01],
    [0xB5, 0x01, 0x0F, 0x0F, 0xA8, 0xA5, 0x51, 0x02],
    [0x17, 0xC1, 0x24, 0x07, 0xF8, 0xF8, 0x22, 0x12],
    [0x71, 0x23, 0x11, 0x06, 0x65, 0x74, 0x18, 0x16],
    [0x01, 0x02, 0xD3, 0x05, 0xC9, 0x95, 0x03, 0x02],
    [0x61, 0x63, 0x0C, 0x00, 0x94, 0xC0, 0x33, 0xF6],
    [0x21, 0x72, 0x0D, 0x00, 0xC1, 0xD5, 0x56, 0x06],
];

static MULTI_TABLE: [u8; 16] = [1, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 20, 24, 24, 30, 30];
static KSL_TABLE: [u8; 16] = [
    0, 32, 40, 45, 48, 51, 53, 55, 56, 58, 59, 60, 61, 62, 63, 64,
];

use std::sync::LazyLock;

static LOG_SIN: LazyLock<[u16; 256]> = LazyLock::new(|| {
    let mut t = [0u16; 256];
    for (i, slot) in t.iter_mut().enumerate() {
        let x = ((i as f64 + 0.5) / 256.0) * std::f64::consts::FRAC_PI_2;
        let sin_val = x.sin();
        *slot = if sin_val < 1e-15 {
            0x1FF
        } else {
            let l = -sin_val.log2() * 256.0;
            if l > 0x1FF as f64 { 0x1FF } else { l as u16 }
        };
    }
    t
});

static EXP_TABLE: LazyLock<[u16; 256]> = LazyLock::new(|| {
    let mut t = [0u16; 256];
    for (i, slot) in t.iter_mut().enumerate() {
        let x = (255 - i) as f64 / 256.0;
        *slot = (2.0_f64.powf(x) * 1024.0 + 0.5) as u16;
    }
    t
});

fn log_sin(phase: u16) -> u16 {
    let idx = (phase & 0xFF) as usize;
    let quadrant = (phase >> 8) & 3;
    match quadrant {
        0 => LOG_SIN[idx],
        1 => LOG_SIN[255 - idx],
        2 => LOG_SIN[idx],
        _ => LOG_SIN[255 - idx],
    }
}

fn exp_to_linear(log_val: u16) -> i16 {
    let clipped = if log_val > 0x1FFF { 0x1FFF } else { log_val };
    let frac = (clipped & 0xFF) as usize;
    let int_part = (clipped >> 8) as u32;
    let mantissa = (EXP_TABLE[frac] as u32 + 1024) >> 1;
    let shifted = mantissa >> int_part;
    shifted as i16
}

struct Operator {
    phase: u32,
    env_level: u16,
    env_state: EnvState,
    key_on: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum EnvState {
    Attack,
    Decay,
    Sustain,
    Release,
}

impl Operator {
    fn new() -> Self {
        Self {
            phase: 0,
            env_level: 0x7F,
            env_state: EnvState::Release,
            key_on: false,
        }
    }
}

#[allow(dead_code)]
struct Patch {
    tl: u8,
    fb: u8,
    mul_m: u8,
    mul_c: u8,
    ar_m: u8,
    dr_m: u8,
    sl_m: u8,
    rr_m: u8,
    ar_c: u8,
    dr_c: u8,
    sl_c: u8,
    rr_c: u8,
    ksl_m: u8,
    ksl_c: u8,
    rect_m: bool,
    rect_c: bool,
    am_m: bool,
    am_c: bool,
    vib_m: bool,
    vib_c: bool,
    eg_m: bool,
    eg_c: bool,
}

impl Patch {
    fn from_bytes(d: &[u8; 8]) -> Self {
        Self {
            am_m: d[0] & 0x80 != 0,
            vib_m: d[0] & 0x40 != 0,
            eg_m: d[0] & 0x20 != 0,
            rect_m: d[0] & 0x10 != 0,
            mul_m: d[0] & 0x0F,
            am_c: d[1] & 0x80 != 0,
            vib_c: d[1] & 0x40 != 0,
            eg_c: d[1] & 0x20 != 0,
            rect_c: d[1] & 0x10 != 0,
            mul_c: d[1] & 0x0F,
            ksl_m: d[2] >> 6,
            tl: d[2] & 0x3F,
            ksl_c: d[3] >> 6,
            fb: d[3] & 0x07,
            ar_m: d[4] >> 4,
            dr_m: d[4] & 0x0F,
            sl_m: d[6] >> 4,
            rr_m: d[6] & 0x0F,
            ar_c: d[5] >> 4,
            dr_c: d[5] & 0x0F,
            sl_c: d[7] >> 4,
            rr_c: d[7] & 0x0F,
        }
    }
}

pub(crate) struct Channel {
    pub(crate) fnum: u16,
    pub(crate) block: u8,
    pub(crate) key_on: bool,
    pub(crate) sustain: bool,
    pub(crate) instrument: u8,
    pub(crate) volume: u8,
    mod_op: Operator,
    car_op: Operator,
    mod_feedback: [i16; 2],
}

impl Channel {
    fn new() -> Self {
        Self {
            fnum: 0,
            block: 0,
            key_on: false,
            sustain: false,
            instrument: 0,
            volume: 0,
            mod_op: Operator::new(),
            car_op: Operator::new(),
            mod_feedback: [0; 2],
        }
    }
}

pub struct Vrc7Audio {
    addr: u8,
    pub(crate) channels: [Channel; NUM_CHANNELS],
    custom_patch: [u8; 8],
    prescaler: u16,
    output: f32,
}

impl Vrc7Audio {
    pub fn new() -> Self {
        Self {
            addr: 0,
            channels: std::array::from_fn(|_| Channel::new()),
            custom_patch: [0; 8],
            prescaler: 0,
            output: 0.0,
        }
    }

    pub fn write_addr(&mut self, val: u8) {
        self.addr = val & 0x3F;
    }

    pub fn write_data(&mut self, val: u8) {
        let a = self.addr as usize;
        match a {
            0x00..=0x07 => {
                self.custom_patch[a] = val;
            }
            0x10..=0x15 => {
                let ch = a - 0x10;
                self.channels[ch].fnum = (self.channels[ch].fnum & 0x100) | val as u16;
            }
            0x20..=0x25 => {
                let ch = a - 0x20;
                let c = &mut self.channels[ch];
                c.fnum = (c.fnum & 0xFF) | (((val & 0x01) as u16) << 8);
                c.block = (val >> 1) & 0x07;
                let new_key = val & 0x10 != 0;
                c.sustain = val & 0x20 != 0;

                if new_key && !c.key_on {
                    c.mod_op.env_state = EnvState::Attack;
                    c.mod_op.phase = 0;
                    c.mod_op.key_on = true;
                    c.car_op.env_state = EnvState::Attack;
                    c.car_op.phase = 0;
                    c.car_op.key_on = true;
                } else if !new_key && c.key_on {
                    c.mod_op.env_state = EnvState::Release;
                    c.mod_op.key_on = false;
                    c.car_op.env_state = EnvState::Release;
                    c.car_op.key_on = false;
                }
                c.key_on = new_key;
            }
            0x30..=0x35 => {
                let ch = a - 0x30;
                self.channels[ch].instrument = val >> 4;
                self.channels[ch].volume = val & 0x0F;
            }
            _ => {}
        }
    }

    pub fn tick(&mut self) {
        self.prescaler += 1;
        if self.prescaler < 36 {
            return;
        }
        self.prescaler = 0;

        let mut sum: i32 = 0;
        for i in 0..NUM_CHANNELS {
            sum += self.clock_channel(i) as i32;
        }
        self.output = (sum as f32) / (6.0 * 256.0) * 0.5;
    }

    fn get_patch(&self, inst: u8) -> Patch {
        if inst == 0 {
            Patch::from_bytes(&self.custom_patch)
        } else {
            Patch::from_bytes(&PATCHES[(inst - 1) as usize])
        }
    }

    fn clock_channel(&mut self, idx: usize) -> i16 {
        let p = self.get_patch(self.channels[idx].instrument);
        let ch = &mut self.channels[idx];

        let freq = (ch.fnum as u32) << ch.block;

        let mod_multi = MULTI_TABLE[p.mul_m as usize] as u32;
        ch.mod_op.phase = ch.mod_op.phase.wrapping_add(freq * mod_multi);

        let car_multi = MULTI_TABLE[p.mul_c as usize] as u32;
        ch.car_op.phase = ch.car_op.phase.wrapping_add(freq * car_multi);

        Self::clock_envelope(
            &mut ch.mod_op,
            p.ar_m,
            p.dr_m,
            p.sl_m,
            p.rr_m,
            p.eg_m,
            ch.sustain,
        );
        Self::clock_envelope(
            &mut ch.car_op,
            p.ar_c,
            p.dr_c,
            p.sl_c,
            p.rr_c,
            p.eg_c,
            ch.sustain,
        );

        let mod_phase = (ch.mod_op.phase >> 16) & 0x3FF;

        let fb_val = if p.fb > 0 {
            let fb_sum = ch.mod_feedback[0] + ch.mod_feedback[1];
            fb_sum >> (8 - p.fb)
        } else {
            0
        };

        let mod_input = mod_phase.wrapping_add(fb_val as u32) & 0x3FF;
        let mod_log = log_sin(mod_input as u16);

        let ksl_m = ksl_attenuation(ch.fnum, ch.block, p.ksl_m);
        let mod_total_atten = ch.mod_op.env_level * 8 + (p.tl as u16) * 8 + ksl_m;
        let mod_out_log = mod_log + mod_total_atten;

        let is_neg_mod = (mod_input >> 9) & 1 != 0;
        let mod_out = if p.rect_m && is_neg_mod {
            0i16
        } else {
            let v = exp_to_linear(mod_out_log);
            if is_neg_mod { -v } else { v }
        };

        ch.mod_feedback[0] = ch.mod_feedback[1];
        ch.mod_feedback[1] = mod_out;

        let car_phase = (ch.car_op.phase >> 16) & 0x3FF;
        let mod_as_phase = ((mod_out as i32 >> 1) & 0x3FF) as u32;
        let car_input = car_phase.wrapping_add(mod_as_phase) & 0x3FF;
        let car_log = log_sin(car_input as u16);

        let ksl_c = ksl_attenuation(ch.fnum, ch.block, p.ksl_c);
        let car_total_atten = ch.car_op.env_level * 8 + (ch.volume as u16) * 16 + ksl_c;
        let car_out_log = car_log + car_total_atten;

        let is_neg_car = (car_input >> 9) & 1 != 0;
        if p.rect_c && is_neg_car {
            0
        } else {
            let v = exp_to_linear(car_out_log);
            if is_neg_car { -v } else { v }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn clock_envelope(
        op: &mut Operator,
        ar: u8,
        dr: u8,
        sl: u8,
        rr: u8,
        eg_type: bool,
        sustain: bool,
    ) {
        match op.env_state {
            EnvState::Attack => {
                if ar == 0 {
                    return;
                }
                let rate = if ar == 15 { 63 } else { ar as u16 * 4 };
                if op.env_level > 0 {
                    let shift = rate.min(63) / 4;
                    let dec = (op.env_level >> shift.max(1)).max(1);
                    op.env_level = op.env_level.saturating_sub(dec);
                }
                if op.env_level == 0 {
                    op.env_state = EnvState::Decay;
                }
            }
            EnvState::Decay => {
                let sl_level = sl as u16 * 8;
                let rate = if dr == 0 { 0 } else { dr as u16 * 4 };
                if rate > 0 {
                    let shift = (12u16.saturating_sub(rate / 4)).min(15);
                    op.env_level = op.env_level.saturating_add(1 << shift >> 4);
                }
                if op.env_level >= sl_level {
                    op.env_level = sl_level;
                    op.env_state = EnvState::Sustain;
                }
            }
            EnvState::Sustain => {
                if !eg_type && !sustain {
                    op.env_state = EnvState::Release;
                }
            }
            EnvState::Release => {
                let rate = if rr == 0 { 0 } else { rr as u16 * 4 };
                if rate > 0 {
                    let shift = (12u16.saturating_sub(rate / 4)).min(15);
                    op.env_level = op.env_level.saturating_add(1 << shift >> 4);
                }
                op.env_level = op.env_level.min(0x7F);
            }
        }
    }

    pub fn output(&self) -> f32 {
        self.output
    }

    pub fn write_state(&self, w: &mut StateWriter) {
        w.write_u8(self.addr);
        w.write_bytes(&self.custom_patch);
        w.write_u16(self.prescaler);
        for ch in &self.channels {
            w.write_u16(ch.fnum);
            w.write_u8(ch.block);
            w.write_bool(ch.key_on);
            w.write_bool(ch.sustain);
            w.write_u8(ch.instrument);
            w.write_u8(ch.volume);
            w.write_u32(ch.mod_op.phase);
            w.write_u16(ch.mod_op.env_level);
            w.write_u8(ch.mod_op.env_state as u8);
            w.write_bool(ch.mod_op.key_on);
            w.write_u32(ch.car_op.phase);
            w.write_u16(ch.car_op.env_level);
            w.write_u8(ch.car_op.env_state as u8);
            w.write_bool(ch.car_op.key_on);
            w.write_u16(ch.mod_feedback[0] as u16);
            w.write_u16(ch.mod_feedback[1] as u16);
        }
    }

    pub fn read_state(&mut self, r: &mut StateReader) -> anyhow::Result<()> {
        self.addr = r.read_u8()?;
        r.read_exact(&mut self.custom_patch)?;
        self.prescaler = r.read_u16()?;
        for ch in &mut self.channels {
            ch.fnum = r.read_u16()?;
            ch.block = r.read_u8()?;
            ch.key_on = r.read_bool()?;
            ch.sustain = r.read_bool()?;
            ch.instrument = r.read_u8()?;
            ch.volume = r.read_u8()?;
            ch.mod_op.phase = r.read_u32()?;
            ch.mod_op.env_level = r.read_u16()?;
            ch.mod_op.env_state = match r.read_u8()? {
                0 => EnvState::Attack,
                1 => EnvState::Decay,
                2 => EnvState::Sustain,
                _ => EnvState::Release,
            };
            ch.mod_op.key_on = r.read_bool()?;
            ch.car_op.phase = r.read_u32()?;
            ch.car_op.env_level = r.read_u16()?;
            ch.car_op.env_state = match r.read_u8()? {
                0 => EnvState::Attack,
                1 => EnvState::Decay,
                2 => EnvState::Sustain,
                _ => EnvState::Release,
            };
            ch.car_op.key_on = r.read_bool()?;
            ch.mod_feedback[0] = r.read_u16()? as i16;
            ch.mod_feedback[1] = r.read_u16()? as i16;
        }
        Ok(())
    }
}

fn ksl_attenuation(fnum: u16, block: u8, ksl: u8) -> u16 {
    if ksl == 0 {
        return 0;
    }
    let idx = ((fnum >> 5) & 0x0F) as usize;
    let base = KSL_TABLE[idx] as i16;
    let atten = (base - (7i16 - block as i16) * 8).max(0) as u16;
    match ksl {
        1 => atten,
        2 => atten >> 1,
        3 => atten >> 2,
        _ => 0,
    }
}
