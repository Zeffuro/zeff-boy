use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Read {
    pub odd: bool,
    pub ready: [bool; 2],
    pub address: [bool; 2],
    pub byte: [bool; 2],
    pub byte_delay: [bool; 2],
    pub selected: [bool; 2],
    pub selected_delay: [bool; 2],
    pub low_capture: [bool; 2],
    pub high_capture: [bool; 2],
    pub low: u8,
    pub high: u8,
}

impl Read {
    pub fn software_origin() -> Self {
        Self {
            odd: false,
            ready: [false; 2],
            address: [false; 2],
            byte: [false; 2],
            byte_delay: [false; 2],
            selected: [false; 2],
            selected_delay: [false; 2],
            low_capture: [false; 2],
            high_capture: [false; 2],
            low: 0,
            high: 0,
        }
    }

    pub fn evaluate(&mut self, input: Inputs, ordinary: bool, selected: bool) {
        if input.phases.horizontal_master && input.memory.data_window {
            self.odd = input.memory.address & 1 != 0;
        }
        if input.phases.horizontal_master {
            let low = self.selected_delay[1] && (self.byte[1] || !ordinary);
            let high = self.selected_delay[1] && !(self.byte[1] && ordinary);
            self.ready[0] = self.low_capture[1];
            self.address[0] = self.odd;
            self.byte[0] = self.address[1];
            self.byte_delay[0] = self.byte[1];
            self.selected[0] = selected;
            self.selected_delay[0] = self.selected[1];
            self.low_capture[0] = low;
            self.high_capture[0] = high;
        }
        if input.phases.horizontal_slave {
            self.ready[1] = self.ready[0];
            self.address[1] = self.address[0];
            self.byte[1] = self.byte[0];
            self.byte_delay[1] = self.byte_delay[0];
            self.selected[1] = self.selected[0];
            self.selected_delay[1] = self.selected_delay[0];
            self.low_capture[1] = self.low_capture[0];
            self.high_capture[1] = self.high_capture[0];
        }
        if input.phases.horizontal_master && self.high_capture[1] {
            self.low = input.memory.data as u8;
        }
        if input.phases.horizontal_master && self.low_capture[1] {
            self.high = if ordinary {
                input.memory.data as u8
            } else {
                (input.memory.data >> 8) as u8
            };
        }
    }

    pub fn word(self) -> u16 {
        u16::from(self.low) | (u16::from(self.high) << 8)
    }
}
