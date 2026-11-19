macro_rules! stages {
    ($($field:ident => $name:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        struct Stages { $($field: [i32; 2],)+ }

        impl Stages {
            fn commit(&mut self) {
                $(self.$field[1] = self.$field[0];)+
                self.vertical_step_n[1] ^= 1;
            }

            fn write_state(&self, w: &mut StateWriter) {
                $(for value in self.$field { w.write_u16(value as u16); })+
            }

            fn read_state(r: &mut StateReader<'_>) -> anyhow::Result<Self> {
                let mut value = Self::default();
                $(
                    let limit = match stringify!($field) {
                        "horizontal" | "vertical" => 511,
                        "vertical_delay" | "composite_delay" | "blank_delay" => 255,
                        "horizontal_delay" => 127,
                        _ => 1,
                    };
                    for item in &mut value.$field {
                        *item = i32::from(r.read_u16()?);
                        anyhow::ensure!(*item <= limit, "invalid horizontal/vertical pipeline cell");
                    }
                )+
                Ok(value)
            }
        }
    }
}

stages! {
    horizontal => "horizontal",
    vertical => "vertical",
    horizontal_reload => "horizontal_reload",
    vertical_step_n => "vertical_step_n",
    vertical_reload => "vertical_reload",
    sound_end_n => "sound_end_n",
    height_n => "height_n",
    vertical_delay => "vertical_delay",
    composite_delay => "composite_delay",
    horizontal_delay => "horizontal_delay",
    blank_delay => "blank_delay",
    composite_release => "composite_release",
    horizontal_release => "horizontal_release",
    sync_end => "sync_end",
    sync_start => "sync_start",
    half_start => "half_start",
    short_end => "short_end",
    blank_start => "blank_start",
    half_clear => "half_clear",
    horizontal_form => "horizontal_form",
    composite_form => "composite_form",
    blank_end => "blank_end",
    half_end => "half_end",
    horizontal_lag => "horizontal_lag",
    alternating => "alternating",
    previous_composite => "previous_composite",
    vertical_clear => "vertical_clear",
    vertical_start => "vertical_start",
    vertical_phase => "vertical_phase",
    vertical_end => "vertical_end",
    vertical_blank_clear => "vertical_blank_clear",
    vertical_source => "vertical_source",
    blank_source => "blank_source",
}

struct HorizontalProfile {
    reload_at: i32,
    reload_to: i32,
    vertical_at: i32,
    sound_end: i32,
    sync_start: i32,
    sync_end: i32,
    half_start: i32,
    short_end: [i32; 2],
    blank_start: i32,
    half_clear: i32,
    blank_end: i32,
    half_end: i32,
}

const H32: HorizontalProfile = HorizontalProfile {
    reload_at: 294,
    reload_to: 466,
    vertical_at: 264,
    sound_end: 258,
    sync_start: 292,
    sync_end: 488,
    half_start: 266,
    short_end: [134, 475],
    blank_start: 284,
    half_clear: 121,
    blank_end: 0,
    half_end: 95,
};

const H40: HorizontalProfile = HorizontalProfile {
    reload_at: 363,
    reload_to: 457,
    vertical_at: 328,
    sound_end: 322,
    sync_start: 358,
    sync_end: 482,
    half_start: 330,
    short_end: [164, 466],
    blank_start: 348,
    half_clear: 148,
    blank_end: 1,
    half_end: 120,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct HeldSync {
    blank: bool,
    half: bool,
    short: bool,
    normal: bool,
    vertical_first: bool,
    vertical_middle: bool,
    vertical_last: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HvCone {
    pal: bool,
    sampled_m5: bool,
    sampled_v30: bool,
    live_m5: bool,
    stages: Stages,
    held: HeldSync,
    hsync: i32,
    csync: i32,
    service: [bool; 2],
    beat_n: [bool; 2],
    refresh: [bool; 2],
    vertical_blank: bool,
}

impl HvCone {
    pub fn new(pal: bool) -> Self {
        let stages = Stages {
            vertical_step_n: [1, 0],
            sound_end_n: [1, 1],
            height_n: [1, 1],
            horizontal_release: [1, 1],
            ..Stages::default()
        };
        Self {
            pal,
            sampled_m5: true,
            sampled_v30: false,
            live_m5: true,
            stages,
            held: HeldSync::default(),
            hsync: 2,
            csync: 2,
            service: [false; 2],
            beat_n: [false; 2],
            refresh: [false; 2],
            vertical_blank: false,
        }
    }

    pub fn sample_mode(&mut self, m5: bool, v30: bool) {
        self.sampled_m5 = m5;
        self.sampled_v30 = v30;
    }

    pub fn mode_samples(&self) -> (bool, bool, bool) {
        (self.sampled_m5, self.sampled_v30, self.live_m5)
    }

    pub fn sampled_height(&self) -> u16 {
        if !self.live_m5 {
            192
        } else if self.sampled_v30 {
            240
        } else {
            224
        }
    }

    pub fn eval(&mut self, h1: bool, h2: bool, registers: [u8; 24], active: bool) {
        let rs = registers[12] & 1 != 0;
        self.live_m5 = registers[1] & 4 != 0;
        if h1 {
            let h = self.stages.horizontal[1] as u16;
            self.service[0] = slots::available(h, rs, active, self.live_m5);
            self.refresh[0] = slots::refresh(h, rs, active, self.live_m5);
            self.beat_n[0] = h & 1 == 0;
            self.prepare(if rs { &H40 } else { &H32 }, registers);
        }
        if h2 {
            self.stages.commit();
            self.service[1] = self.service[0];
            self.beat_n[1] = !self.beat_n[0];
            self.refresh[1] = self.refresh[0];
        }
        self.update_held();
        if self.stages.vertical_end[1] != 0 {
            self.vertical_blank = true;
        } else if self.stages.vertical_blank_clear[1] != 0 {
            self.vertical_blank = false;
        }
        self.hsync = 2 * self.stages.horizontal_release[1];
        self.csync = 2 * self.stages.composite_release[1];
    }

    fn prepare(&mut self, p: &HorizontalProfile, registers: [u8; 24]) {
        let m5 = self.live_m5;
        let v28 = self.sampled_m5 && !self.sampled_v30;
        let v30 = self.sampled_m5 && self.sampled_v30;
        let s = &mut self.stages;
        let h = s.horizontal[1];
        let v = s.vertical[1];
        let composite = s.composite_form[1] ^ s.vertical_source[1];
        let in_vertical = m5
            && (self.held.vertical_first || self.held.vertical_middle || self.held.vertical_last);
        let outside_normal = !in_vertical && self.held.normal;
        let v_step = s.vertical_step_n[1] != 0;

        s.horizontal[0] = if s.horizontal_reload[1] != 0 {
            if m5 {
                p.reload_to
            } else if registers[12] & 1 != 0 {
                464
            } else {
                466
            }
        } else {
            (h + 1) & 511
        };
        s.vertical[0] = if v_step && s.vertical_reload[1] != 0 {
            let pal_m4 = self.pal && !m5;
            384 | if !pal_m4 { 64 } else { 0 }
                | if (!self.pal && v28) || pal_m4 { 32 } else { 0 }
                | if !v28 { 16 } else { 0 }
                | if self.pal && (v28 || !m5) { 8 } else { 0 }
                | if self.pal { 2 } else { 5 }
        } else {
            (v + i32::from(v_step)) & 511
        };
        s.horizontal_reload[0] = i32::from(h == p.reload_at);
        s.vertical_step_n[0] = i32::from(h != if m5 { p.vertical_at } else { 488 });
        let vertical_match = |pal28, pal30, pal4, ntsc28, ntsc4| {
            if self.pal {
                (v28 && v == pal28) || (v30 && v == pal30) || (!m5 && v == pal4)
            } else {
                (v28 && v == ntsc28) || (!m5 && v == ntsc4)
            }
        };
        s.vertical_reload[0] = i32::from(vertical_match(258, 266, 242, 234, 218));
        s.sound_end_n[0] = i32::from(h != p.sound_end);
        s.height_n[0] = i32::from(!((v28 && v == 224) || (v30 && v == 240) || (!m5 && v == 192)));

        s.vertical_delay[0] = ((s.vertical_delay[1] << 1) | s.vertical_source[1]) & 255;
        s.composite_delay[0] = ((s.composite_delay[1] << 1) | composite) & 255;
        let horizontal = if m5 && registers[0] & 8 != 0 {
            s.alternating[1]
        } else {
            s.horizontal_lag[1]
        };
        s.horizontal_delay[0] = ((s.horizontal_delay[1] << 1) | horizontal) & 127;
        s.blank_delay[0] = ((s.blank_delay[1] << 1) | s.blank_source[1]) & 255;
        let composite_active = if m5 {
            s.composite_delay[1] & 128 != 0
        } else {
            composite != 0
        };
        let horizontal_active = if m5 {
            s.horizontal_delay[1] & 64 != 0
        } else {
            horizontal != 0
        };
        s.composite_release[0] = i32::from(!composite_active);
        s.horizontal_release[0] = i32::from(!horizontal_active || registers[12] & 32 != 0);

        s.horizontal_lag[0] = s.horizontal_form[1];
        s.horizontal_form[0] = i32::from(outside_normal || (in_vertical && self.held.short));
        s.composite_form[0] = i32::from(
            outside_normal
                || (m5 && (self.held.vertical_first || self.held.vertical_last) && self.held.short)
                || (m5 && self.held.vertical_middle && self.held.half),
        );
        s.alternating[0] =
            s.alternating[1] ^ i32::from(composite != 0 && s.previous_composite[1] == 0);
        s.previous_composite[0] = composite;
        s.vertical_source[0] = i32::from(self.held.vertical_middle);
        s.blank_source[0] = i32::from(self.held.blank);

        s.sync_start[0] = i32::from(h == p.sync_start);
        s.sync_end[0] = i32::from(h == p.sync_end);
        s.half_start[0] = i32::from(h == p.half_start);
        s.short_end[0] = i32::from(p.short_end.contains(&h));
        s.blank_start[0] = i32::from(h == p.blank_start);
        s.half_clear[0] = i32::from(h == p.half_clear);
        s.blank_end[0] = i32::from(h == p.blank_end);
        s.half_end[0] = i32::from(h == p.half_end);
        s.vertical_clear[0] = i32::from(vertical_match(464, 472, 448, 491, 475));
        s.vertical_start[0] = i32::from(vertical_match(461, 469, 445, 488, 472));
        s.vertical_phase[0] = i32::from(vertical_match(458, 466, 442, 485, 469));
        s.vertical_end[0] = i32::from(vertical_match(256, 264, 240, 232, 216));
        s.vertical_blank_clear[0] = i32::from(vertical_match(474, 482, 458, 501, 485));
    }

    fn update_held(&mut self) {
        let s = &self.stages;
        let held = &mut self.held;
        let half_clear = s.sync_start[1] != 0 || s.half_clear[1] != 0;
        if s.blank_start[1] != 0 {
            held.blank = true;
        } else if s.blank_end[1] != 0 {
            held.blank = false;
        }
        if s.half_start[1] != 0 || s.half_end[1] != 0 {
            held.half = true;
        } else if half_clear {
            held.half = false;
        }
        if half_clear {
            held.short = true;
        } else if s.short_end[1] != 0 {
            held.short = false;
        }
        if s.sync_start[1] != 0 {
            held.normal = true;
        } else if s.sync_end[1] != 0 {
            held.normal = false;
        }
        if s.vertical_start[1] != 0 {
            held.vertical_first = true;
        } else if s.vertical_clear[1] != 0 {
            held.vertical_first = false;
        }
        if s.vertical_phase[1] != 0 {
            held.vertical_middle = true;
        } else if s.vertical_start[1] != 0 {
            held.vertical_middle = false;
        }
        if s.vertical_end[1] != 0 {
            held.vertical_last = true;
        } else if s.vertical_phase[1] != 0 {
            held.vertical_last = false;
        }
    }

    pub fn hsync_drive(&self) -> i32 {
        self.hsync
    }

    pub fn csync_drive(&self) -> i32 {
        self.csync
    }

    pub fn counters(&self) -> (u16, u16) {
        (
            self.stages.horizontal[1] as u16,
            self.stages.vertical[1] as u16,
        )
    }

    pub fn irq_inputs(&self) -> super::irq::HvInputs {
        super::irq::HvInputs {
            h: self.stages.horizontal[1],
            v: self.stages.vertical[1],
            step: self.stages.vertical_step_n[1] != 0,
            height: self.stages.height_n[1] == 0,
            sound_end: self.stages.sound_end_n[1] == 0,
        }
    }

    pub fn memory_gates(&self) -> (bool, bool) {
        (self.service[1], self.beat_n[1])
    }

    pub fn refresh(&self) -> bool {
        self.refresh[1]
    }

    pub fn hblank(&self) -> bool {
        if self.live_m5 {
            self.stages.blank_delay[1] & 128 != 0
        } else {
            self.stages.blank_source[1] != 0
        }
    }

    pub fn visible_gate(&self) -> bool {
        !self.hblank() && !self.vertical_blank
    }

    pub fn write_state(&self, w: &mut StateWriter) {
        w.write_u8(3);
        w.write_bool(self.pal);
        w.write_bool(self.sampled_m5);
        w.write_bool(self.sampled_v30);
        w.write_bool(self.live_m5);
        w.write_bool(self.vertical_blank);
        self.stages.write_state(w);
        for bit in [
            self.held.blank,
            self.held.half,
            self.held.short,
            self.held.normal,
            self.held.vertical_first,
            self.held.vertical_middle,
            self.held.vertical_last,
        ] {
            w.write_bool(bit);
        }
        w.write_u8(self.hsync as u8);
        w.write_u8(self.csync as u8);
        for bit in self
            .service
            .into_iter()
            .chain(self.beat_n)
            .chain(self.refresh)
        {
            w.write_bool(bit);
        }
    }

    pub fn read_state(r: &mut StateReader<'_>, pal: bool) -> anyhow::Result<Self> {
        anyhow::ensure!(r.read_u8()? == 3, "unsupported horizontal/vertical state");
        anyhow::ensure!(r.read_bool()? == pal, "vertical region differs");
        let sampled_m5 = r.read_bool()?;
        let sampled_v30 = r.read_bool()?;
        let live_m5 = r.read_bool()?;
        let vertical_blank = r.read_bool()?;
        let stages = Stages::read_state(r)?;
        let held = HeldSync {
            blank: r.read_bool()?,
            half: r.read_bool()?,
            short: r.read_bool()?,
            normal: r.read_bool()?,
            vertical_first: r.read_bool()?,
            vertical_middle: r.read_bool()?,
            vertical_last: r.read_bool()?,
        };
        let hsync = i32::from(r.read_u8()?);
        let csync = i32::from(r.read_u8()?);
        anyhow::ensure!(
            matches!(hsync, 0 | 2) && matches!(csync, 0 | 2),
            "invalid sync drive"
        );
        let service = [r.read_bool()?, r.read_bool()?];
        let beat_n = [r.read_bool()?, r.read_bool()?];
        let refresh = [r.read_bool()?, r.read_bool()?];
        anyhow::ensure!(
            if stages.vertical_end[1] != 0 {
                vertical_blank
            } else {
                stages.vertical_blank_clear[1] == 0 || !vertical_blank
            },
            "inconsistent vertical blank latch"
        );
        Ok(Self {
            pal,
            sampled_m5,
            sampled_v30,
            live_m5,
            stages,
            held,
            hsync,
            csync,
            service,
            beat_n,
            refresh,
            vertical_blank,
        })
    }
}
mod slots;
use crate::hardware::vdp::state::{StateReader, StateWriter};
