use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Address {
    pub original: u32,
    pub master: u32,
    pub slave: u32,
    pub select_original: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct AddressEvaluation {
    pub cycle_enable: bool,
    pub original_selected: bool,
    pub memory_cycle: bool,
}

impl Address {
    pub fn software_origin() -> Self {
        Self {
            original: 0,
            master: 0,
            slave: 0,
            select_original: true,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn evaluate(
        &mut self,
        input: PrimitiveInputs,
        intake: Intake,
        handshake: Handshake,
        mode5: bool,
        increment: u8,
        memory_cycle: bool,
    ) -> AddressEvaluation {
        if input.phases.horizontal_master {
            self.select_original = !memory_cycle;
        }
        if intake.first {
            self.original = (self.original & !0x3f00) | (u32::from(input.main.data_bus) & 0x3f00);
        }
        if intake.first_low {
            self.original = (self.original & !0xff) | u32::from(input.main.data_bus & 0xff);
        }
        let original_selected = self.select_original && handshake.busy_second;
        let cycle_enable = handshake.external_cycle
            || intake.first
            || intake.second
            || memory_cycle
            || input.frontend.cpu_data_capture;
        if cycle_enable {
            self.master = if original_selected {
                self.original
            } else {
                (self.slave + u32::from(increment) + u32::from(!mode5)) & 0x1ffff
            };
        } else {
            self.slave = self.master;
        }
        if input.reset_comb || !mode5 {
            self.master &= !0x1c000;
            self.slave &= !0x1c000;
        }
        if input.reset_comb {
            self.master &= !0x3fff;
            self.slave &= !0x3fff;
        }
        // The second word's high address enters after the shared-address evaluation.
        if intake.second {
            self.original = (self.original & !0x1c000) | (u32::from(input.main.data_bus & 7) << 14);
        }
        if input.reset_comb || !mode5 {
            self.original &= !0x1c000;
        }
        AddressEvaluation {
            cycle_enable,
            original_selected,
            memory_cycle,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LoadPipeline {
    pub enable: bool,
    pub decode: bool,
    pub master: bool,
    pub slave: bool,
    pub idle: bool,
}

impl LoadPipeline {
    pub fn software_origin() -> Self {
        Self {
            enable: false,
            decode: false,
            master: false,
            slave: false,
            idle: true,
        }
    }

    pub fn evaluate(&mut self, phases: PhaseLevels, intake: Intake, handshake: Handshake) -> bool {
        if phases.horizontal_slave {
            self.enable = !self.idle;
        }
        if phases.data_master {
            self.master = self.decode;
            self.idle = !(self.decode || self.slave);
        }
        if phases.data_slave {
            self.decode = intake.register_code_before
                && !(handshake.ready || handshake.request_sample)
                && handshake.first_pending;
            self.slave = self.master;
        }
        self.enable && phases.horizontal_master
    }
}
