use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct Sample {
    pub v: u16,
    pub select: bool,
    pub reload: bool,
    pub q: bool,
    pub z: bool,
    pub last: bool,
    pub h: bool,
    pub e: bool,
    pub c: u8,
    pub overflow: bool,
    pub a: bool,
    pub b: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct Core {
    pub epoch: MasterTicks,
    pub updated_at: MasterTicks,
    pub v: u16,
    pub select: bool,
    pub reload: bool,
    pub q: bool,
    pub z: bool,
    pub last: bool,
    pub h: bool,
    pub e: bool,
    pub c: u8,
    pub overflow: bool,
    pub active: bool,
    pub hp: bool,
    pub vp: bool,
    pub sound: bool,
    pub t6: bool,
    pub a: bool,
    pub b: bool,
    pub clear: bool,
    pub prepared: Sample,
    pub field_id: u64,
    pub field_start: MasterTicks,
    pub line_start: MasterTicks,
    pub row: u16,
    pub wrap: bool,
    pub coverage: bool,
    pub canonical: bool,
}

impl Core {
    pub fn new(epoch: MasterTicks) -> Self {
        let prepared = Sample {
            v: 0,
            select: false,
            reload: false,
            q: false,
            z: true,
            last: false,
            h: true,
            e: false,
            c: 255,
            overflow: false,
            a: false,
            b: false,
        };
        Self {
            epoch,
            updated_at: epoch,
            v: 0,
            select: false,
            reload: false,
            q: false,
            z: false,
            last: false,
            h: false,
            e: false,
            c: 255,
            overflow: false,
            active: false,
            hp: false,
            vp: false,
            sound: false,
            t6: false,
            a: false,
            b: false,
            clear: false,
            prepared,
            field_id: 0,
            field_start: epoch,
            line_start: epoch,
            row: 0,
            wrap: false,
            coverage: true,
            canonical: true,
        }
    }

    pub fn h_counter(&self, at: MasterTicks) -> u16 {
        let dot = (at.get() - self.epoch.get()) % H32_LINE_TICKS / H32_DOT_TICKS;
        if dot < H32_COUNTER_LOW_COUNT {
            dot as u16
        } else {
            (dot - H32_COUNTER_LOW_COUNT + H32_COUNTER_HIGH_START) as u16
        }
    }

    pub fn commit(&mut self, at: MasterTicks, _region: Region) -> Result<(), VdpTimingError> {
        self.commit_at(at, self.h_counter(at))
    }

    pub fn commit_at(&mut self, at: MasterTicks, h: u16) -> Result<(), VdpTimingError> {
        let old_v = self.v;
        let p = self.prepared;
        self.v = p.v;
        self.select = p.select;
        self.reload = p.reload;
        self.q = p.q;
        self.z = p.z;
        self.last = p.last;
        self.h = p.h;
        self.e = p.e;
        self.c = p.c;
        self.overflow = p.overflow;
        self.a = p.a;
        self.b = p.b;
        self.clear = self.b && !self.a;
        if self.z {
            self.active = true;
        } else if self.q {
            self.active = false;
        }
        if self.h && self.q {
            self.sound = true;
        } else if self.e {
            self.sound = false;
        }
        self.wrap |= old_v == 511 && self.v == 0;
        if h == 0 {
            self.line_start = at;
            if self.wrap {
                self.field_id = self
                    .field_id
                    .checked_add(1)
                    .ok_or(VdpTimingError::ClockOverflow)?;
                self.field_start = at;
                self.row = 0;
                self.wrap = false;
                self.coverage = self.canonical && self.v == 0;
                self.canonical = self.v == 0;
            } else {
                self.row = self
                    .row
                    .checked_add(1)
                    .ok_or(VdpTimingError::ClockOverflow)?;
            }
        }
        Ok(())
    }

    pub fn prepare(&mut self, region: Region, registers: &VdpRegisterPipeline, h: u16) {
        self.prepare_clock(region, registers, h, None);
    }

    pub fn prepare_clock(
        &mut self,
        region: Region,
        registers: &VdpRegisterPipeline,
        h: u16,
        clock: Option<super::super::clock::LineClock>,
    ) {
        let mode = registers.registers()[1].unwrap();
        let m5 = mode & MODE5_ENABLE != 0;
        let v30 = clock.is_some() && mode & V30_ENABLE != 0;
        let (compare, reload, height) = match (m5, region == Region::Pal, v30) {
            (true, false, true) => (512, 469, 240),
            (true, false, false) => (234, 485, 224),
            (true, true, _) => (258, 458, 224),
            (false, false, _) => (218, 469, 192),
            (false, true, _) => (242, 442, 192),
        };
        let v = if self.select && self.reload {
            reload
        } else {
            (self.v + u16::from(self.select)) & 511
        };
        let active = self.active || self.last;
        let base = if !active || self.overflow {
            !registers.registers()[10].unwrap()
        } else {
            self.c
        };
        let total = u16::from(base) + u16::from(self.select);
        self.prepared = Sample {
            v,
            select: (m5
                && h == if clock.is_some_and(|c| c != super::super::clock::LineClock::H32) {
                    328
                } else {
                    264
                })
                || (!m5 && h == 488),
            reload: self.v == compare,
            q: self.v == height,
            z: self.v == 0,
            last: self.v == 511,
            h: h == 0,
            e: h == if clock.is_some_and(|c| c != super::super::clock::LineClock::H32) {
                322
            } else {
                258
            },
            c: total as u8,
            overflow: active && total > 255,
            a: self.t6,
            b: self.a,
        };
        if !m5 {
            self.coverage = false;
            self.canonical = false;
        } else if h == 0 && self.v == 0 && self.row == 0 && self.line_start == self.field_start {
            self.canonical = true;
            self.coverage = true;
        }
    }

    pub fn settle(&mut self, mode: u8) {
        let clear = self.clear && mode & MODE5_ENABLE == 0;
        if self.overflow {
            self.hp = true;
        } else if clear {
            self.hp = false;
        }
        if self.h && self.q {
            self.vp = true;
        } else if clear {
            self.vp = false;
        }
    }

    pub fn hblank(&self, at: MasterTicks, mode: u8) -> bool {
        let h = self.h_counter(at);
        if mode & MODE5_ENABLE != 0 {
            !(10..294).contains(&h)
        } else {
            !(2..286).contains(&h)
        }
    }

    pub fn status(&self, region: Region, registers: &VdpRegisterPipeline) -> u16 {
        let mode = registers.registers()[1].unwrap();
        FIFO_EMPTY_STATUS
            | if region == Region::Pal { PAL_STATUS } else { 0 }
            | if self.vp { VINT_PENDING_STATUS } else { 0 }
            | if mode & DISPLAY_ENABLE == 0 || !(self.last || self.active) {
                VBLANK_STATUS
            } else {
                0
            }
            | if self.hblank(self.updated_at, mode) {
                HBLANK_STATUS
            } else {
                0
            }
    }
}
