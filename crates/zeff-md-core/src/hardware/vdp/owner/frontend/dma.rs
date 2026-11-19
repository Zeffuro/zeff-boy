use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Dff {
    pub master: bool,
    pub slave: bool,
}

impl Dff {
    pub fn zero() -> Self {
        Self {
            master: false,
            slave: false,
        }
    }

    pub fn evaluate(&mut self, clock: bool, data: bool, reset: bool) {
        if clock {
            self.slave = self.master;
        } else {
            self.master = data;
        }
        if reset {
            self.master = false;
            self.slave = false;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Dma {
    pub grant: Dff,
    pub request_delay: Dff,
    pub active_delay: Dff,
    pub active: Dff,
    pub internal_request: bool,
    pub granted_request: bool,
    pub external_request: bool,
    pub fill_seeded: bool,
    pub external_cycle: bool,
    pub source_enabled: bool,
    pub grant_acknowledge: bool,
    pub cycle: [bool; 2],
    pub produce: [bool; 2],
    pub full: [bool; 2],
    pub retire: [bool; 2],
    pub retire_delay: [bool; 2],
    pub empty: [bool; 2],
    pub terminal: [bool; 2],
    pub selected: [bool; 2],
    pub fill_selected: [bool; 2],
}

impl Dma {
    pub fn software_origin() -> Self {
        Self {
            grant: Dff::zero(),
            request_delay: Dff::zero(),
            active_delay: Dff::zero(),
            active: Dff::zero(),
            internal_request: false,
            granted_request: false,
            external_request: false,
            fill_seeded: false,
            external_cycle: false,
            source_enabled: false,
            // The held active-low feedback starts at its literal software-origin zero.
            grant_acknowledge: true,
            cycle: [false; 2],
            produce: [false; 2],
            full: [false; 2],
            retire: [false; 2],
            retire_delay: [false; 2],
            empty: [true; 2],
            terminal: [false; 2],
            selected: [false; 2],
            fill_selected: [false; 2],
        }
    }

    pub fn early(
        &mut self,
        phases: PhaseLevels,
        external: ExternalPins,
        reset_comb: bool,
        mode: u8,
    ) -> DmaView {
        let reset = reset_comb || self.terminal[1];
        self.grant
            .evaluate(external.cpu_clock1, external.bus_grant, false);
        self.active_delay.evaluate(
            phases.horizontal_slave,
            self.internal_request || self.granted_request,
            reset,
        );
        self.active
            .evaluate(phases.horizontal_slave, self.active_delay.slave, reset);
        self.request_delay
            .evaluate(external.cpu_clock1, self.external_request, false);
        self.grant_acknowledge = self.granted_request;
        DmaView {
            active: self.active.slave,
            external_word: self.active.slave && mode == 0,
            external_byte: self.active.slave && mode == 1,
            fill: self.active.slave && mode == 2,
            copy: self.active.slave && mode == 3,
            bus_request: self.request_delay.slave,
            bus_grant_acknowledge: self.grant_acknowledge,
        }
    }

    pub fn request(&mut self, input: Inputs, registers: [u8; 24], second_dma: bool) {
        let reset = input.reset_comb || self.terminal[1];
        let enabled = registers[1] & 0x14 == 0x14;
        let internal = registers[23] & 0x80 != 0;
        if enabled && second_dma && internal {
            self.internal_request = true;
        } else if reset {
            self.internal_request = false;
        }
        let grant = self.request_delay.slave
            && input.external.bus_grant_acknowledge_level
            && input.external.data_acknowledge_level
            && self.grant.slave
            && !input.external.address_strobe;
        if enabled && second_dma && !internal {
            self.external_request = true;
        } else if reset || grant {
            self.external_request = false;
        }
        if grant {
            self.granted_request = true;
        } else if reset {
            self.granted_request = false;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn service(
        &mut self,
        input: Inputs,
        view: DmaView,
        equal: bool,
        data_produce: bool,
        terminal: bool,
        consumer_half: bool,
    ) -> bool {
        let external_capture = self.cycle[1] && input.external_address_cycle;
        let produced = external_capture || data_produce;
        let retiring = self.retire[1] && !self.retire_delay[1] && !consumer_half;
        if input.phases.data_master {
            self.cycle[0] = input.external_address_cycle;
            self.produce[0] = produced;
            self.full[0] = !input.reset_comb && equal && (self.full[1] || self.produce[1]);
            self.retire_delay[0] = self.retire[1];
            self.empty[0] = input.reset_comb || equal && (self.empty[1] || retiring);
        }
        if input.phases.data_slave {
            self.cycle[1] = self.cycle[0];
            self.produce[1] = self.produce[0];
            self.full[1] = self.full[0];
            self.retire_delay[1] = self.retire_delay[0];
            self.empty[1] = self.empty[0];
        }
        if input.phases.horizontal_master {
            self.retire[0] = self.selected[1];
            self.terminal[0] = terminal;
        }
        if input.phases.horizontal_slave {
            self.external_cycle = input.external_address_cycle;
            self.retire[1] = self.retire[0];
            self.terminal[1] = self.terminal[0];
        }
        self.source_enabled = view.external_word || view.external_byte;
        if input.phases.horizontal_master {
            self.selected[0] = input.memory.opportunity && !self.empty[1];
            self.fill_selected[0] =
                self.fill_seeded && view.fill && self.empty[1] && input.memory.opportunity;
        }
        if input.phases.horizontal_slave {
            self.selected[1] = self.selected[0];
            self.fill_selected[1] = self.fill_selected[0];
        }
        if view.active && self.selected[1] {
            self.fill_seeded = true;
        } else if input.reset_comb || self.terminal[1] {
            self.fill_seeded = false;
        }
        external_capture
    }
}
