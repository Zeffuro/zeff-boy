use super::logic::{Delay, counter, plain, read_bits, reset, set, write_bits};
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

mod cart;
mod companion;
mod state;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Inputs {
    pub vclk: bool,
    pub zclk: bool,
    pub data: u16,
    pub address: u32,
    pub bgack: bool,
    pub bg: bool,
    pub read: bool,
    pub upper: bool,
    pub address_strobe: bool,
    pub dtack: bool,
    pub lower: bool,
    pub cas: bool,
    pub output_enable: bool,
    pub fc: [bool; 2],
    pub wait_level: bool,
    pub sound_bus_acknowledge_level: bool,
    pub sound_address: u16,
    pub sound_bank: u16,
    pub sound_read_level: bool,
    pub sound_write_level: bool,
    pub sound_memory_level: bool,
    pub sound_io_level: bool,
    pub m1: bool,
    pub m3: bool,
    pub delayed: [bool; 8],
}

impl Inputs {
    pub const fn zero() -> Self {
        Self {
            vclk: false,
            zclk: false,
            data: 0,
            address: 0,
            bgack: false,
            bg: false,
            read: false,
            upper: false,
            address_strobe: false,
            dtack: false,
            lower: false,
            cas: false,
            output_enable: false,
            fc: [false; 2],
            wait_level: false,
            sound_bus_acknowledge_level: false,
            sound_address: 0,
            sound_bank: 0,
            sound_read_level: false,
            sound_write_level: false,
            sound_memory_level: false,
            sound_io_level: false,
            m1: false,
            m3: false,
            delayed: [false; 8],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Arbiter {
    input: Inputs,
    old_input: Inputs,
    delay: [Delay; 8],
    w: [bool; 375],
    p: [[bool; 2]; 81],
    d: [[bool; 4]; 81],
    c: [[bool; 3]; 81],
    fc: [bool; 4],
    va: [bool; 4],
    reset_released: bool,
    reset_sync: bool,
    z_to_v: bool,
    v_to_z: bool,
    data8: bool,
    cart22: bool,
    sound_address_high: u16,
    pub br_level: bool,
    pub bgack_level: bool,
    pub dtack_level: bool,
    pub wait_level: bool,
    pub vpa_level: bool,
    pub ce_level: bool,
    pub lower_oe_level: bool,
    pub upper_oe_level: bool,
    pub ram_address14: bool,
    pub strobe_direction: bool,
    pub read_direction: bool,
    pub as_level: bool,
    pub upper_level: bool,
    pub lower_level: bool,
    pub bus_status_read: bool,
}

impl Arbiter {
    pub fn low_address_direction(&self) -> bool {
        self.w[131]
    }
    pub fn high_address_direction(&self) -> bool {
        self.w[142]
    }
    pub fn observed_clocks(&self) -> [bool; 2] {
        [self.input.vclk, self.input.zclk]
    }
    pub fn sound_acknowledge_level(&self) -> bool {
        self.input.sound_bus_acknowledge_level
    }

    pub fn sound_to_main_level(&self) -> bool {
        self.z_to_v
    }
    pub fn main_to_sound_level(&self) -> bool {
        self.v_to_z
    }
    pub fn sound_address_high(&self) -> u16 {
        self.sound_address_high
    }
    pub fn held_inputs(&self) -> Inputs {
        self.input
    }
    pub fn sound_request(&self) -> bool {
        self.d[14][1]
    }
    pub fn sound_reset(&self) -> bool {
        !self.w[166]
    }
    pub fn main_reset_released(&self) -> bool {
        self.input.m3 && self.w[328]
    }

    fn ff(&mut self, index: usize, clock: bool, data: bool) {
        plain(&mut self.p[index], clock, data);
    }
    fn rf(&mut self, index: usize, clock: bool, data: bool, release: bool) {
        reset(&mut self.d[index], clock, data, release);
    }
    fn sf(&mut self, index: usize, clock: bool, data: bool, release: bool) {
        set(&mut self.d[index], clock, data, release);
    }
    fn count(&mut self, index: usize, clock: bool, load: bool, value: bool, carry: bool) {
        counter(&mut self.c[index], clock, load, value, carry);
    }

    pub const fn software_origin() -> Self {
        Self {
            input: Inputs::zero(),
            old_input: Inputs::zero(),
            delay: [Delay::zero(); 8],
            w: [false; 375],
            p: [[false; 2]; 81],
            d: [[false; 4]; 81],
            c: [[false; 3]; 81],
            fc: [false; 4],
            va: [false; 4],
            reset_released: false,
            reset_sync: false,
            z_to_v: false,
            v_to_z: false,
            data8: false,
            cart22: false,
            sound_address_high: 0,
            br_level: false,
            bgack_level: false,
            dtack_level: false,
            wait_level: false,
            vpa_level: false,
            ce_level: false,
            lower_oe_level: false,
            upper_oe_level: false,
            ram_address14: false,
            strobe_direction: false,
            read_direction: false,
            as_level: false,
            upper_level: false,
            lower_level: false,
            bus_status_read: false,
        }
    }

    pub fn evaluate(&mut self, cycle: u64, input: Inputs) -> Result<()> {
        self.evaluate_physical(cycle, input, &mut ())
    }
    pub fn evaluate_physical<H: crate::sound_trace::physical::HalfCapture>(
        &mut self,
        cycle: u64,
        mut input: Inputs,
        physical: &mut H,
    ) -> Result<()> {
        ensure!(
            input.address <= 0x7fffff && input.sound_bank < 512 && input.m3,
            "unsupported arbiter companion profile"
        );
        let sources = [
            self.input.m1,
            self.w[188],
            self.w[254],
            self.w[113],
            self.w[271],
            self.w[238],
            self.w[223],
            self.input.m3,
        ];
        for (index, length) in [2, 2, 7, 2, 3, 7, 7, 2].into_iter().enumerate() {
            input.delayed[index] = self.delay[index].update(cycle, sources[index], length)?;
        }
        self.input = input;
        if self.input != self.old_input {
            for _ in 0..5 {
                self.settle(physical);
            }
            self.old_input = self.input;
        }
        ensure!(!self.d[26][1], "unsupported MAIN DRAM memory-mode bank");
        Ok(())
    }

    fn settle<H: crate::sound_trace::physical::HalfCapture>(&mut self, physical: &mut H) {
        let i = self.input;
        self.w[9] = self.reset_released;
        plain(&mut self.p[13], !i.vclk, i.upper);
        self.w[65] = !(self.p[13][1] && i.upper);
        self.w[31] = !self.w[65];
        self.reset_released = true;
        self.data8 = i.data & 256 != 0;
        self.w[95] = !(i.upper || i.read);
        self.w[96] = !(self.w[95] && self.w[90] && !self.w[122]);
        self.w[97] = !(self.w[95] && self.w[91] && !self.w[122]);
        self.w[130] = i.address_strobe || self.w[129];
        self.w[103] = !(self.p[24][1] || i.read || self.w[130]);
        plain(&mut self.p[24], i.vclk, self.w[103]);
        self.w[94] = self.w[128] || i.address_strobe;
        self.w[75] = !(self.w[112] && self.w[94]);
        self.w[126] = self.w[124] || i.address_strobe;
        self.w[131] = self.z_to_v;
        self.w[137] = !self.w[106];
        self.w[142] = !self.w[137];
        reset(&mut self.d[31], self.w[96], self.data8, self.w[328]);
        self.w[166] = self.d[31][1];
        plain(&mut self.p[32], i.vclk, self.reset_released);
        self.w[118] = self.w[122] || i.address_strobe;
        self.w[133] = !(self.w[132]
            && self.w[118]
            && self.w[130]
            && self.w[126]
            && self.w[94]
            && self.w[134]);
        self.w[200] = !(self.w[133] && self.w[249]);
        self.w[222] = !self.w[200];
        self.dtack_level = !self.w[222];
        self.w[223] = self.w[48] || i.bgack;
        self.reset_sync = self.p[32][1];
        self.w[336] = i.wait_level;
        self.w[343] = self.fc[0] && !i.delayed[7];
        self.w[363] = self.fc[1] && !i.delayed[7];
        self.w[346] = self.fc[2] && !i.delayed[7];
        self.w[332] = !(self.w[333] || i.delayed[7]);
        self.w[333] = !(i.delayed[7] || self.reset_sync);
        self.fc = [
            !i.fc[1] && !i.fc[0],
            !i.fc[1] && i.fc[0],
            i.fc[1] && !i.fc[0],
            i.fc[1] && i.fc[0],
        ];
        self.va = [
            i.address & 0x2000 != 0,
            i.address & 0x100000 != 0,
            i.address & 0x200000 != 0,
            i.address & 0x400000 != 0,
        ];
        self.w[91] = i.address & 0x180 == 0;
        self.w[99] = i.address & 0x180 == 0x80;
        self.w[90] = i.address & 0x180 == 0x100;
        self.w[119] = i.address & 0x7f8000 != 0x500000;
        self.w[128] = i.address & 0x7fff80 != 0x508000;
        self.w[122] = i.address & 0x7ffe00 != 0x508800;
        self.w[129] = i.address & 0x7fff80 != 0x509000;
        self.w[124] = i.address & 0x7fff80 != 0x509800;
        let mut high = (i.sound_address & 0x7f00) >> 8;
        high |= if self.w[86] {
            i.sound_bank << 7
        } else {
            0xc000
        };
        self.sound_address_high = if self.w[86] { high } else { high & !255 };
        self.w[86] = i.sound_address & 0xff00 != 0x7f00;
        self.ram_address14 = !self.va[0];
        self.z_to_main();
        physical.vdp(crate::sound_trace::physical::vdp::Event::Arbiter(
            crate::sound_trace::physical::vdp::Source::Sound {
                logical: i.sound_address,
                bank: i.sound_bank,
                write: !i.sound_write_level && !i.sound_memory_level,
            },
        ));
        #[cfg(test)]
        physical.vdp_raw(crate::sound_trace::physical::vdp::raw::Event::Arbiter {
            logical: i.sound_address,
            bank: i.sound_bank,
            write: !i.sound_write_level && !i.sound_memory_level,
            high: self.sound_address_high,
            levels: [self.as_level, self.upper_level, self.lower_level],
        });
        self.reset_counter();
        self.sound_clock();
        self.main_to_z();
        self.w[249] = !(self.z_to_v && self.fc[3]);
        self.vpa_level = i.address_strobe || self.w[249];
        self.cart();
        self.divider();
        self.ram_oe();
        self.bus_status_read = !self.w[12];
    }
}
