use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Vsram {
    pub data: [u16; 2],
    pub high: [u8; 2],
    pub return_gate: [bool; 2],
    pub read: [bool; 2],
    pub upper: [bool; 2],
    pub lower: [bool; 2],
    pub read_delay: [bool; 2],
    pub upper_delay: [bool; 2],
    pub lower_delay: [bool; 2],
    pub address: [u8; 2],
    pub cpu_address: [u8; 2],
    pub return_hold: u16,
    pub out: u16,
    pub even: u16,
    pub odd: u16,
}

pub(super) struct Evaluation {
    pub bus: Option<BusUpdate>,
    pub store: Option<Store>,
}

impl Vsram {
    pub fn software_origin() -> Self {
        Self {
            data: [0; 2],
            high: [0; 2],
            return_gate: [false; 2],
            read: [false; 2],
            upper: [false; 2],
            lower: [false; 2],
            read_delay: [false; 2],
            upper_delay: [false; 2],
            lower_delay: [false; 2],
            address: [0; 2],
            cpu_address: [0; 2],
            return_hold: 0,
            out: 0,
            even: 0,
            odd: 0,
        }
    }

    pub fn view(self) -> VsramView {
        VsramView {
            out: self.out,
            even: self.even,
            odd: self.odd,
        }
    }

    pub fn evaluate(
        &mut self,
        input: PlanesInputs,
        mut read_word: impl FnMut(u8) -> Result<u16>,
    ) -> Result<Evaluation> {
        let f = input.frontend;
        if input.phases.horizontal_master {
            let scheduled = self.read[1] || self.upper[1] || self.lower[1];
            let vscroll = input.profile.memory.registers[11] & 4 != 0;
            let mode5 = input.profile.memory.registers[1] & 4 != 0;
            let column = if vscroll {
                ((input.horizontal_counter >> 4) & 31) as u8
            } else {
                0
            };
            let plane = !(mode5 && vscroll) ^ (input.horizontal_counter & 8 != 0);
            self.address[0] = if scheduled {
                self.cpu_address[1]
            } else {
                (column << 1) | u8::from(plane)
            };
            self.data[0] = u16::from(f.data_low_delay) | (u16::from(self.high[1]) << 8);
            self.high[0] = ((f.data >> 8) & 7) as u8;
            self.return_gate[0] = self.read_delay[1];
            self.read[0] = f.read_vsram;
            self.upper[0] = f.vsram_upper;
            self.lower[0] = f.vsram_lower;
            self.read_delay[0] = self.read[1];
            self.upper_delay[0] = self.upper[1];
            self.lower_delay[0] = self.lower[1];
            self.cpu_address[0] = ((f.address >> 1) & 63) as u8;
        }
        if input.phases.horizontal_slave {
            self.data[1] = self.data[0];
            self.high[1] = self.high[0];
            self.return_gate[1] = self.return_gate[0];
            self.read[1] = self.read[0];
            self.upper[1] = self.upper[0];
            self.lower[1] = self.lower[0];
            self.read_delay[1] = self.read_delay[0];
            self.upper_delay[1] = self.upper_delay[0];
            self.lower_delay[1] = self.lower_delay[0];
            self.address[1] = self.address[0];
            self.cpu_address[1] = self.cpu_address[0];
        }
        // CPU return captures the prior RAM output before this primitive's read.
        if input.phases.horizontal_master && self.read_delay[1] {
            self.return_hold = self.out;
        }
        let bus = self.return_gate[1].then_some(BusUpdate {
            mask: 0x7ff,
            value: self.return_hold,
        });
        let index = self.address[1];
        let store = if index < 40 {
            let mask = if input.phases.horizontal_master {
                (if self.lower_delay[1] { 255 } else { 0 })
                    | if self.upper_delay[1] { 0x700 } else { 0 }
            } else {
                0
            };
            let mut word = read_word(index)?;
            ensure!(word & !0x7ff == 0, "invalid parent VSRAM word");
            let store = (mask != 0).then_some(Store {
                target: Target::Vsram,
                index,
                mask,
                value: self.data[1] & mask,
            });
            if let Some(store) = store {
                word = (word & !store.mask) | store.value;
            }
            self.out = word;
            if index & 1 != 0 {
                self.odd = word;
            } else {
                self.even = word;
            }
            store
        } else {
            self.out &= if index & 1 != 0 { self.odd } else { self.even };
            None
        };
        Ok(Evaluation { bus, store })
    }
}
