#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EepromState {
    Standby,
    ReceiveControl,
    ReceiveAddress,
    ReceiveWriteData,
    SendReadData,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EepromReadPhase {
    OutputBits,
    AwaitMasterAck,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EepromAckPhase {
    None,
    Pending,
    Clocked,
}

pub(super) struct Eeprom {
    pub(super) data: [u8; 256],
    pub(super) scl: bool,
    pub(super) sda_in: bool,
    pub(super) read_enable: bool,
    pub(super) state: EepromState,
    pub(super) shift: u8,
    pub(super) bits: u8,
    pub(super) pointer: u8,
    pub(super) address_latched: bool,
    pub(super) read_phase: EepromReadPhase,
    pub(super) read_bit: u8,
    read_clocked: bool,
    ack_phase: EepromAckPhase,
}

impl Eeprom {
    pub(super) fn new() -> Self {
        Self {
            data: [0xFF; 256],
            scl: true,
            sda_in: true,
            read_enable: false,
            state: EepromState::Standby,
            shift: 0,
            bits: 0,
            pointer: 0,
            address_latched: false,
            read_phase: EepromReadPhase::OutputBits,
            read_bit: 0,
            read_clocked: false,
            ack_phase: EepromAckPhase::None,
        }
    }

    pub(super) fn clear_transient_bus_phase(&mut self) {
        self.ack_phase = EepromAckPhase::None;
        self.read_clocked = false;
    }

    fn begin_receive_byte(&mut self, next_state: EepromState) {
        self.state = next_state;
        self.shift = 0;
        self.bits = 0;
    }

    fn start_condition(&mut self) {
        self.begin_receive_byte(EepromState::ReceiveControl);
        self.address_latched = false;
        self.ack_phase = EepromAckPhase::None;
    }

    fn stop_condition(&mut self) {
        self.state = EepromState::Standby;
        self.read_phase = EepromReadPhase::OutputBits;
        self.bits = 0;
        self.read_clocked = false;
        self.ack_phase = EepromAckPhase::None;
    }

    fn ack_next_clock(&mut self) {
        self.ack_phase = EepromAckPhase::Pending;
    }

    fn receive_byte_bit(&mut self, sda: bool) {
        self.shift = (self.shift << 1) | u8::from(sda);
        self.bits += 1;
        if self.bits < 8 {
            return;
        }

        let byte = self.shift;
        self.shift = 0;
        self.bits = 0;

        match self.state {
            EepromState::ReceiveControl => {
                let device_match = (byte & 0xF0) == 0xA0;
                let read = byte & 0x01 != 0;
                if !device_match {
                    self.state = EepromState::Standby;
                    return;
                }
                self.ack_next_clock();
                if read {
                    self.state = EepromState::SendReadData;
                    self.read_phase = EepromReadPhase::OutputBits;
                    self.read_bit = 0;
                    self.read_clocked = false;
                } else if self.address_latched {
                    self.state = EepromState::ReceiveWriteData;
                } else {
                    self.state = EepromState::ReceiveAddress;
                }
            }
            EepromState::ReceiveAddress => {
                self.pointer = byte;
                self.address_latched = true;
                self.state = EepromState::ReceiveWriteData;
                self.ack_next_clock();
            }
            EepromState::ReceiveWriteData => {
                self.data[self.pointer as usize] = byte;
                self.pointer = self.pointer.wrapping_add(1);
                self.ack_next_clock();
            }
            _ => {}
        }
    }

    fn clock_rising_edge(&mut self, sda: bool) {
        if self.ack_phase == EepromAckPhase::Pending {
            self.ack_phase = EepromAckPhase::Clocked;
            return;
        }

        match self.state {
            EepromState::SendReadData => match self.read_phase {
                EepromReadPhase::OutputBits => {
                    self.read_clocked = true;
                }
                EepromReadPhase::AwaitMasterAck => {
                    if sda {
                        self.state = EepromState::Standby;
                    } else {
                        self.pointer = self.pointer.wrapping_add(1);
                        self.read_phase = EepromReadPhase::OutputBits;
                        self.read_bit = 0;
                        self.read_clocked = false;
                    }
                }
            },
            EepromState::ReceiveControl
            | EepromState::ReceiveAddress
            | EepromState::ReceiveWriteData => self.receive_byte_bit(sda),
            EepromState::Standby => {}
        }
    }

    pub(super) fn handle_control_write(&mut self, val: u8) {
        let scl = val & 0x20 != 0;
        let sda_in = val & 0x40 != 0;
        self.read_enable = val & 0x80 != 0;

        let prev_scl = self.scl;
        let prev_sda = self.sda_in;

        if prev_scl && scl {
            if prev_sda && !sda_in {
                self.start_condition();
            } else if !prev_sda && sda_in {
                self.stop_condition();
            }
        }

        if !prev_scl && scl {
            self.clock_rising_edge(sda_in);
        }

        if prev_scl && !scl && self.ack_phase == EepromAckPhase::Clocked {
            self.ack_phase = EepromAckPhase::None;
        }

        if prev_scl
            && !scl
            && self.state == EepromState::SendReadData
            && self.read_phase == EepromReadPhase::OutputBits
            && self.read_clocked
        {
            self.read_clocked = false;
            self.read_bit = self.read_bit.saturating_add(1);
            if self.read_bit >= 8 {
                self.read_phase = EepromReadPhase::AwaitMasterAck;
            }
        }

        self.scl = scl;
        self.sda_in = sda_in;
    }

    pub(super) fn data_out(&self) -> bool {
        if !self.read_enable {
            return true;
        }

        if self.ack_phase != EepromAckPhase::None {
            return false;
        }

        match self.state {
            EepromState::SendReadData => match self.read_phase {
                EepromReadPhase::OutputBits => {
                    let byte = self.data[self.pointer as usize];
                    let bit = 7u8.saturating_sub(self.read_bit);
                    ((byte >> bit) & 1) != 0
                }
                EepromReadPhase::AwaitMasterAck => true,
            },
            _ => true,
        }
    }
}
