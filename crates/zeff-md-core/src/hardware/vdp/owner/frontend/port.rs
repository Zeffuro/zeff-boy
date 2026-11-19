use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Port {
    pub dma_ready: bool,
    pub read_cycle_ready: bool,
    pub byte_second: bool,
    pub byte_second_n: bool,
    pub byte_pending: bool,
    pub byte_pending_n: bool,
    pub upper: bool,
    pub lower: bool,
    pub pending: bool,
    pub read_ready: bool,
    pub read_request: bool,
    pub read_request_n: bool,
    pub read_enable: bool,
    pub read_selected: [bool; 2],
    pub write_selected: [bool; 2],
    pub read_kind: [bool; 2],
    pub read_kind_delay: [bool; 2],
}

impl Port {
    pub fn software_origin() -> Self {
        Self {
            dma_ready: true,
            read_cycle_ready: true,
            byte_second: false,
            byte_second_n: true,
            byte_pending: false,
            byte_pending_n: true,
            upper: false,
            lower: false,
            pending: false,
            read_ready: true,
            read_request: false,
            read_request_n: true,
            read_enable: false,
            read_selected: [false; 2],
            write_selected: [false; 2],
            read_kind: [false; 2],
            read_kind_delay: [false; 2],
        }
    }

    pub fn recover(&mut self, recovery: bool, reset: bool) {
        let set = recovery && self.byte_pending;
        let clear = recovery && self.byte_pending_n || reset;
        if set {
            self.byte_second = true;
        } else if clear {
            self.byte_second = false;
        }
        if clear {
            self.byte_second_n = true;
        } else if set {
            self.byte_second_n = false;
        }
    }

    pub fn intake(
        &mut self,
        input: Inputs,
        feedback: HandshakeFeedback,
        intake: Intake,
        data_ready: bool,
    ) -> (bool, bool, bool) {
        if input.reset_comb || intake.word_clear {
            self.byte_pending = false;
            self.byte_pending_n = true;
        }
        let release = feedback.release_data_lanes || input.reset_comb;
        if input.upper_strobe {
            self.upper = true;
        } else if release {
            self.upper = false;
        }
        if input.lower_strobe {
            self.lower = true;
        } else if release {
            self.lower = false;
        }
        let capture = self.pending && feedback.recovery;
        if intake.data_write {
            self.pending = true;
        } else if release {
            self.pending = false;
        }
        let produce = self.pending && feedback.data_produce_window;
        if feedback.recovery || input.reset_comb {
            self.read_ready = true;
        } else if data_ready {
            self.read_ready = false;
        }
        let read_ack = (input.main.read || input.hv_read) && !(self.read_ready && intake.data_read);
        (capture, produce, read_ack)
    }

    pub fn select(&mut self, context: Context, empty: bool, terminal: bool) -> (bool, bool) {
        let Context {
            input,
            control,
            code,
            dma: view,
            old_read,
            old_write,
            ..
        } = context;
        let mode5 = control.mode5_before;
        let mode128 = control.registers.before().logical()[1] & 0x80 != 0 && mode5;
        let complete = old_read || terminal || input.reset_comb;
        if input.phases.horizontal_master {
            self.read_selected[0] = self.read_request && empty && input.memory.opportunity;
            self.write_selected[0] =
                self.read_request_n && self.read_enable && empty && input.memory.opportunity;
        }
        if input.phases.horizontal_slave {
            self.read_selected[1] = self.read_selected[0];
            self.write_selected[1] = self.write_selected[0];
        }
        let ordinary = !mode128 && code & 12 == 0;
        let set = (view.copy || ordinary) && self.write_selected[1];
        if set {
            self.read_request = true;
        } else if complete {
            self.read_request = false;
        }
        if complete {
            self.read_request_n = true;
        } else if set {
            self.read_request_n = false;
        }
        let intake = control.intake;
        let inactive =
            !(intake.read_pending || intake.second_pending || intake.first_pending && !mode5);
        let enable = view.copy && code & 16 != 0
            || code & 19 == 0 && !inactive && intake.recovery_before
            || intake.recovery_before && intake.read_pending && !mode5;
        if enable {
            self.read_enable = true;
        } else if self.write_selected[1] || terminal || input.reset_comb {
            self.read_enable = false;
        }
        let combined = old_write || !view.copy && old_read;
        (combined && code & 12 == 8, combined && code & 12 == 4)
    }

    pub fn kind(&mut self, phases: PhaseLevels, read_cram: bool, read_vsram: bool) {
        if phases.horizontal_master {
            self.read_kind[0] = !(read_cram || read_vsram);
            self.read_kind_delay[0] = self.read_kind[1];
        }
        if phases.horizontal_slave {
            self.read_kind[1] = self.read_kind[0];
            self.read_kind_delay[1] = self.read_kind_delay[0];
        }
    }
}
