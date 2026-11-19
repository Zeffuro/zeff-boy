use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Handshake {
    pub request: bool,
    pub write_control: bool,
    pub read_control: bool,
    pub external_cycle: bool,
    pub release_delay: bool,
    pub busy_first: bool,
    pub ready: bool,
    pub busy_second: bool,
    pub request_sample: bool,
    pub cancel: bool,
    pub second: bool,
    pub second_n: bool,
    pub second_pending: bool,
    pub read_pending: bool,
    pub first_pending: bool,
    pub previous_data_read: bool,
}

impl Handshake {
    pub fn software_origin() -> Self {
        Self {
            request: false,
            write_control: false,
            read_control: false,
            external_cycle: false,
            release_delay: false,
            busy_first: false,
            ready: true,
            busy_second: false,
            request_sample: false,
            cancel: false,
            second: false,
            second_n: true,
            second_pending: false,
            read_pending: false,
            first_pending: false,
            previous_data_read: false,
        }
    }

    pub fn feedback(self) -> HandshakeFeedback {
        HandshakeFeedback {
            request_gate: self.request || self.ready,
            recovery: !self.busy_second && !self.ready,
            release_data_lanes: self.release_delay && !self.busy_first,
            data_produce_window: self.busy_first && !self.busy_second,
            write_control_latched: self.write_control,
            read_control_latched: self.read_control,
            request_sampled: self.request_sample,
            busy_first: self.busy_first,
            busy_second: self.busy_second,
            ready: self.ready,
            first_pending: self.first_pending,
            second_pending: self.second_pending,
            read_pending: self.read_pending,
            command_second: self.second,
            command_second_n: self.second_n,
        }
    }

    pub fn evaluate(&mut self, input: PrimitiveInputs, mode5: bool, code: &mut u8) -> Intake {
        let main = input.main;
        let recovery = !self.busy_second && !self.ready;
        let overlap = self.busy_first && self.ready;
        let read_data_latched = !self.read_control && main.read;
        let status_read = main.read && main.control_select;
        let second = main.write
            && input.frontend.second_word_ready
            && self.write_control
            && self.second
            && mode5;
        if main.write || main.read {
            self.request = true;
        } else if self.busy_second || input.reset_comb {
            self.request = false;
        }
        if !main.write {
            self.write_control = main.control_select;
        }
        if !main.read {
            self.read_control = main.control_select;
        }
        let first =
            main.write && self.second_n && input.frontend.first_word_ready && self.write_control;
        let data_write = !self.write_control && main.write;
        let clear = self.previous_data_read || data_write || second || status_read;
        if input.reset_comb || clear {
            self.cancel = true;
        } else if overlap {
            self.cancel = false;
        }
        let register_code_before = *code & 3 == 2;
        let set_second = recovery && self.first_pending && !register_code_before && mode5;
        let clear_second = self.cancel && recovery || input.reset_comb;
        if set_second {
            self.second = true;
        } else if clear_second {
            self.second = false;
        }
        if clear_second {
            self.second_n = true;
        } else if set_second {
            self.second_n = false;
        }
        if second {
            *code = (*code & !0x1c) | ((main.data_bus >> 2) as u8 & 0x1c);
        }
        if input.reset_comb || !mode5 {
            *code &= !0x1c;
        }
        self.previous_data_read = main.read && !main.control_select;
        if second {
            self.second_pending = true;
        } else if input.reset_comb || overlap {
            self.second_pending = false;
        }
        if read_data_latched {
            self.read_pending = true;
        } else if input.reset_comb || overlap {
            self.read_pending = false;
        }
        if first {
            self.first_pending = true;
        } else if input.reset_comb || overlap {
            self.first_pending = false;
        }
        if first {
            *code = (*code & !3) | (main.data_bus >> 14) as u8;
        }
        let intake = Intake {
            first,
            first_low: first,
            second,
            data_write,
            status_read,
            data_read: self.previous_data_read,
            control_ack: first
                || input.frontend.dma_bus_ready && input.frontend.read_cycle_ready && second,
            second_dma_bit: second && main.data_bus & 0x80 != 0,
            command_clear: clear,
            word_clear: first || clear,
            register_code_before,
            recovery_before: recovery,
            second_pending: self.second_pending,
            read_pending: self.read_pending,
            first_pending: self.first_pending,
        };
        if input.phases.data_master {
            self.external_cycle = input.frontend.external_address_cycle;
            self.release_delay = self.busy_first;
            self.ready = !self.busy_second;
            self.request_sample = self.request;
        }
        if input.phases.data_slave {
            self.busy_first = !self.ready;
            self.busy_second = self.request_sample;
        }
        intake
    }
}
