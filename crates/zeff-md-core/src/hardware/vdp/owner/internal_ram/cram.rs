use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Cram {
    pub upper: [bool; 2],
    pub lower: [bool; 2],
    pub address: [u8; 2],
    pub blue: [u8; 2],
    pub read_delay: [u8; 2],
    pub palette: Palette,
    pub rgb: Rgb,
    pub address_wire: u8,
    pub out: u16,
    pub return_hold: u16,
    pub return_capture: bool,
    pub address_tag: [Option<PixelTag>; 2],
    pub wire_tag: Option<PixelTag>,
    pub out_tag: Option<PixelTag>,
}

pub(super) struct Evaluation {
    pub bus: Option<BusUpdate>,
    pub store: Option<Store>,
}

impl Cram {
    pub fn software_origin() -> Self {
        Self {
            upper: [false; 2],
            lower: [false; 2],
            address: [0; 2],
            blue: [0; 2],
            read_delay: [0; 2],
            palette: Palette::software_origin(),
            rgb: Rgb::software_origin(),
            address_wire: 0,
            out: 0,
            return_hold: 0,
            return_capture: false,
            address_tag: [None; 2],
            wire_tag: None,
            out_tag: None,
        }
    }

    pub fn evaluate(
        &mut self,
        input: VideoInputs,
        mut read_word: impl FnMut(u8) -> Result<u16>,
    ) -> Result<Evaluation> {
        let f = input.frontend;
        let mode5 = input.profile.memory.registers[1] & 4 != 0;
        self.rgb
            .clock(input, self.out, self.palette.intensity(), self.out_tag);
        if input.phases.horizontal_master {
            self.upper[0] = f.cram_upper;
            self.lower[0] = f.cram_lower;
            self.address[0] = self.address_wire;
            self.address_tag[0] = self.wire_tag;
            self.blue[0] = ((f.data >> 9) & 7) as u8;
            self.read_delay[0] = ((self.read_delay[1] << 1) | u8::from(f.read_cram)) & 7;
        }
        if input.phases.horizontal_slave {
            self.upper[1] = self.upper[0];
            self.lower[1] = self.lower[0];
            self.address[1] = self.address[0];
            self.address_tag[1] = self.address_tag[0];
            self.blue[1] = self.blue[0];
            self.read_delay[1] = self.read_delay[0];
        }
        self.palette.evaluate(
            input.phases,
            input.palette,
            input.profile.memory.registers,
            input.geometry,
        )?;
        self.wire_tag = self.palette.tag();
        self.address_wire = if f.cram_upper || f.cram_lower || f.read_cram {
            if mode5 {
                ((f.address >> 1) & 63) as u8
            } else {
                (f.address & 31) as u8
            }
        } else {
            self.palette.address()
        };
        let mask = if input.phases.horizontal_master {
            (if self.lower[1] { 0xee } else { 0 }) | if self.upper[1] { 0xe00 } else { 0 }
        } else {
            0
        };
        let low = u16::from(f.data_low_delay);
        let colors = if mode5 {
            low & 0xee
        } else {
            ((low & 7) << 1) | ((low & 0x38) << 2)
        };
        let value = colors | (u16::from(self.blue[1]) << 9);
        let index = self.address[1];
        let mut word = read_word(index)?;
        ensure!(word & !0xeee == 0, "invalid parent CRAM word");
        let store = (mask != 0).then_some(Store {
            target: Target::Cram,
            index,
            mask,
            value: value & mask,
        });
        if let Some(store) = store {
            word = (word & !store.mask) | store.value;
        }
        self.out = pack(word);
        self.out_tag = self.address_tag[1];
        if self.return_capture {
            self.return_hold = self.out;
        }
        let bus = (self.read_delay[1] & 4 != 0).then_some(BusUpdate {
            mask: 0xeee,
            value: unpack(self.return_hold),
        });
        self.return_capture = self.read_delay[1] & 1 != 0 && input.phases.horizontal_master;
        Ok(Evaluation { bus, store })
    }
}

fn pack(word: u16) -> u16 {
    ((word >> 1) & 7) | ((word >> 2) & 0x38) | ((word >> 3) & 0x1c0)
}
fn unpack(word: u16) -> u16 {
    ((word & 7) << 1) | ((word & 0x38) << 2) | ((word & 0x1c0) << 3)
}
