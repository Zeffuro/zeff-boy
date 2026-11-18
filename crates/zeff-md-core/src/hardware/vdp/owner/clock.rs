#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Latch([bool; 2]);

impl Latch {
    fn level(&mut self, high: bool, data: bool) {
        if high {
            self.0[1] = self.0[0];
        } else {
            self.0[0] = data;
        }
    }

    fn cleared(&mut self, high: bool, data: bool, clear: bool) {
        self.level(high, data);
        if clear {
            self.0 = [false; 2];
        }
    }

    fn q(self) -> bool {
        self.0[1]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counter {
    latch: Latch,
    carry: bool,
}

impl Counter {
    fn level(&mut self, high: bool, advance: bool, load: bool, carry: bool) {
        self.carry = self.latch.q() && carry;
        let next = if advance {
            self.latch.q() ^ carry
        } else {
            load
        };
        self.latch.level(high, next);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Control {
    latch: Latch,
    q: bool,
    nq: bool,
}

impl Control {
    fn level(&mut self, high: bool, data: bool, clear: bool) {
        if clear {
            self.latch.0 = [false; 2];
        } else {
            self.latch.level(high, data);
        }
        self.q = self.latch.q();
        self.nq = !self.q;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ClockCone {
    m: [Latch; 17],
    m_in: [bool; 9],
    m_cache: [bool; 9],
    m_out: [bool; 14],
    e: [Counter; 7],
    control: [Control; 2],
    e_in: [bool; 3],
    e_cache: [bool; 3],
    ew: [bool; 12],
    edclk: bool,
    toggle: Latch,
    phase: [bool; 2],
    gate: [Latch; 2],
    h: [bool; 2],
}

impl ClockCone {
    pub fn software_origin(pal: bool) -> Self {
        let mut m = [Latch::default(); 17];
        m[4] = Latch([true; 2]);
        m[5] = Latch([true; 2]);
        let m_in = [true, true, true, false, false, !pal, false, false, false];
        let mut e = [Counter::default(); 7];
        for index in [0, 1, 3] {
            e[index].latch = Latch([true; 2]);
        }
        e[1].carry = true;
        let control = [Control {
            nq: true,
            ..Control::default()
        }; 2];
        let mut ew = [false; 12];
        for index in [1, 7, 11] {
            ew[index] = true;
        }
        Self {
            m,
            m_in,
            m_cache: m_in,
            m_out: [false; 14],
            e,
            control,
            e_in: [true; 3],
            e_cache: [true; 3],
            ew,
            edclk: false,
            toggle: Latch([true; 2]),
            phase: [false, true],
            gate: [Latch([true; 2]), Latch::default()],
            h: [false, true],
        }
    }

    pub fn eval_edclk(&mut self, mclk: bool, hsync: bool) {
        self.e_in = [mclk, true, hsync];
        if self.e_in != self.e_cache {
            for _ in 0..3 {
                self.settle_edclk();
            }
            self.e_cache = self.e_in;
        }
    }

    fn settle_edclk(&mut self) {
        let high = self.e_in[0];
        let advance = self.ew[1];
        self.e[0].level(high, advance, true, self.ew[3]);
        self.e[1].level(high, advance, !self.control[1].latch.q(), true);
        let middle = self.e[1].latch.q();
        self.e[2].level(high, advance, false, middle);
        let q = [
            self.e[0].latch.q(),
            self.e[1].latch.q(),
            self.e[2].latch.q(),
        ];
        self.ew[1] = q.into_iter().any(|bit| bit);
        self.ew[2] = q[2];
        self.ew[3] = q[1] && q[2];
        self.ew[4] = self.ew[2];
        self.ew[5] = self.e[3..].iter().all(|cell| cell.latch.q());
        let active = self.control[1].latch.q();
        self.e[3].level(self.ew[4], active, true, active);
        for index in 4..7 {
            let carry = self.e[index - 1].carry;
            self.e[index].level(self.ew[4], active, false, carry);
        }
        self.edclk = self.ew[2];
        self.ew[7] = self.control[0].nq;
        self.ew[11] = self.e_in[2] && !self.control[1].latch.q();
        self.ew[10] = !self.ew[11];
        self.control[1].level(self.ew[2], self.ew[10], !self.ew[7]);
        self.control[0].level(self.ew[2], self.ew[5], false);
    }

    pub fn eval_mclk(&mut self, mclk: bool, edclk: bool, vclk: bool, r12: u8) {
        self.m_in[0] = mclk;
        self.m_in[3] = vclk;
        self.m_in[4] = edclk;
        self.m_in[6] = r12 & 0x80 != 0;
        self.m_in[7] = r12 & 1 != 0;
        if self.m_in != self.m_cache {
            for _ in 0..2 {
                self.settle_mclk();
            }
            self.m_cache = self.m_in;
        }
    }

    fn write_m(&mut self, index: usize, high: bool, data: bool) {
        self.m[index].cleared(high, data, self.m_out[0]);
    }

    fn settle_mclk(&mut self) {
        let high = self.m_in[0];
        self.m[0].level(high, false);
        let first = self.m[0].q();
        self.m[1].level(high, first);
        self.m_out[0] = self.m[1].q() && !self.m[0].q();
        self.write_m(2, high, self.m[3].q());
        self.write_m(3, high, !self.m[2].q());
        self.m_out[1] = self.m[3].q();
        self.write_m(4, high, self.m[6].q());
        self.write_m(5, high, self.m[4].q());
        self.write_m(6, high, !(self.m[4].q() && self.m[5].q()));
        self.m_out[2] = self.m[6].q();
        self.write_m(7, high, self.m[10].q());
        self.write_m(8, high, self.m[7].q());
        self.write_m(9, high, !(self.m[7].q() && self.m[8].q()));
        self.write_m(10, high, self.m[9].q());
        self.m_out[3] = !self.m[10].q();
        for (base, clock, output) in [(11, self.m_out[1], 4), (14, self.m_out[2], 5)] {
            self.write_m(base, clock, !(self.m[base].q() || self.m[base + 1].q()));
            self.write_m(base + 1, clock, self.m[base].q());
            self.write_m(base + 2, !clock, self.m[base + 1].q());
            self.m_out[output] = self.m[base + 1].q() || self.m[base + 2].q();
        }
        self.m_out[6] = if self.m_in[5] {
            self.m_out[4]
        } else {
            self.m_out[5]
        };
        self.m_out[7] = self.m_out[5];
        self.m_out[8] = !self.m_out[3];
        self.m_out[9] = if self.m_in[6] {
            self.m_in[4]
        } else if self.m_in[7] {
            self.m_out[1]
        } else {
            self.m_out[2]
        };
        self.m_out[10] = self.m_out[7];
        self.m_out[12] = self.m_out[6];
    }

    pub fn eval_dclk(&mut self, d1: bool, d2: bool) {
        if d1 {
            self.phase = [self.toggle.q(), !self.toggle.q()];
            self.toggle.0[0] = !self.toggle.q();
        }
        if d2 {
            self.toggle.0[1] = self.toggle.0[0];
        }
        for index in 0..2 {
            self.gate[index].cleared(!d1, true, d2 && self.phase[index]);
            self.h[index] = !self.gate[index].q();
        }
    }

    pub fn edclk(&self) -> bool {
        self.edclk
    }
    pub fn dclk(&self) -> bool {
        self.m_out[9]
    }
    pub fn cpu_clk0(&self) -> bool {
        self.m_out[7]
    }
    pub fn cpu_clk1(&self) -> bool {
        self.m_out[8]
    }
    pub fn h1(&self) -> bool {
        self.h[0]
    }
    pub fn h2(&self) -> bool {
        self.h[1]
    }

    fn validate_pins(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.m_in[1]
                && self.m_in[2]
                && !self.m_in[8]
                && self.e_in[1]
                && self.phase[0] != self.phase[1],
            "invalid fixed VDP clock pins"
        );
        Ok(())
    }

    pub fn validate_profile(&self, pal: bool) -> anyhow::Result<()> {
        self.validate_pins()?;
        anyhow::ensure!(self.m_in[5] != pal, "VDP clock region differs");
        Ok(())
    }

    pub fn write_state(&self, w: &mut StateWriter) {
        for cell in self.m {
            bits_write(w, cell.0);
        }
        bits_write(w, self.m_in);
        bits_write(w, self.m_cache);
        bits_write(w, self.m_out);
        for cell in self.e {
            bits_write(w, cell.latch.0);
            w.write_bool(cell.carry);
        }
        for cell in self.control {
            bits_write(w, cell.latch.0);
            w.write_bool(cell.q);
            w.write_bool(cell.nq);
        }
        bits_write(w, self.e_in);
        bits_write(w, self.e_cache);
        bits_write(w, self.ew);
        w.write_bool(self.edclk);
        bits_write(w, self.toggle.0);
        bits_write(w, self.phase);
        for cell in self.gate {
            bits_write(w, cell.0);
        }
        bits_write(w, self.h);
    }

    pub fn read_state(r: &mut StateReader<'_>) -> anyhow::Result<Self> {
        let mut m = [Latch::default(); 17];
        for cell in &mut m {
            cell.0 = bits_read(r)?;
        }
        let m_in = bits_read(r)?;
        let m_cache = bits_read(r)?;
        let m_out = bits_read(r)?;
        let mut e = [Counter::default(); 7];
        for cell in &mut e {
            cell.latch.0 = bits_read(r)?;
            cell.carry = r.read_bool()?;
        }
        let mut control = [Control::default(); 2];
        for cell in &mut control {
            cell.latch.0 = bits_read(r)?;
            cell.q = r.read_bool()?;
            cell.nq = r.read_bool()?;
            anyhow::ensure!(
                cell.q == cell.latch.q() && cell.q != cell.nq,
                "invalid control clock latch"
            );
        }
        let e_in = bits_read(r)?;
        let e_cache = bits_read(r)?;
        let ew = bits_read(r)?;
        let edclk = r.read_bool()?;
        let toggle = Latch(bits_read(r)?);
        let phase = bits_read(r)?;
        let mut gate = [Latch::default(); 2];
        for cell in &mut gate {
            cell.0 = bits_read(r)?;
        }
        let h = bits_read(r)?;
        anyhow::ensure!(
            m_in == m_cache && e_in == e_cache,
            "stale clock input cache"
        );
        anyhow::ensure!(
            h == [!gate[0].q(), !gate[1].q()],
            "invalid horizontal clock gates"
        );
        let value = Self {
            m,
            m_in,
            m_cache,
            m_out,
            e,
            control,
            e_in,
            e_cache,
            ew,
            edclk,
            toggle,
            phase,
            gate,
            h,
        };
        value.validate_pins()?;
        Ok(value)
    }
}
use crate::hardware::vdp::state::{StateReader, StateWriter};

fn bits_write<const N: usize>(w: &mut StateWriter, values: [bool; N]) {
    for value in values {
        w.write_bool(value);
    }
}

fn bits_read<const N: usize>(r: &mut StateReader<'_>) -> anyhow::Result<[bool; N]> {
    let mut values = [false; N];
    for value in &mut values {
        *value = r.read_bool()?;
    }
    Ok(values)
}
