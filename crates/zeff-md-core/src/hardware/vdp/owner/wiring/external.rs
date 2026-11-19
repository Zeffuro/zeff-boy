use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Cell(pub [bool; 2]);

impl Cell {
    fn sample(&mut self, high: bool, data: bool, clear: bool) {
        if high {
            self.0[1] = self.0[0];
        } else {
            self.0[0] = data;
        }
        if clear {
            self.0 = [false; 2];
        }
    }

    fn value(self) -> bool {
        self.0[1]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct External {
    pub refresh_history: [[bool; 2]; 3],
    pub quiet: [bool; 2],
    pub source_delay: [bool; 2],
    pub blocked: bool,
    pub release: bool,
    pub selected: bool,
    pub write_enable: Cell,
    pub stretch: [Cell; 5],
    pub address_delay: Cell,
    pub address_seen: Cell,
    pub address_released: Cell,
    pub divider_enabled: Cell,
    pub terminal_delay: Cell,
    pub terminal: Cell,
    pub recovery: [Cell; 6],
    pub divider: [Cell; 7],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Access {
    #[cfg(test)]
    pub full_pressure: bool,
    pub external_address_cycle: bool,
    pub acknowledge: bool,
    pub read_enable: bool,
    pub write_enable: bool,
    pub ram_selected: bool,
    pub access_window: bool,
    pub row_window: bool,
    pub column_window: bool,
    pub first_recovery: bool,
    pub second_recovery: bool,
    pub divider_clear: bool,
    pub read_lanes: bool,
}

impl External {
    pub fn early(&mut self, input: Inputs, pins: BoardPins, address: u32) -> Access {
        let phase = input.phases;
        #[cfg(test)]
        let mut full_pressure = false;
        if phase.horizontal_master {
            self.refresh_history[0][0] = input.refresh;
            self.refresh_history[1][0] = self.refresh_history[0][1];
            self.refresh_history[2][0] = self.refresh_history[1][1];
            self.quiet[0] = !(input.refresh || self.refresh_history.iter().any(|p| p[1]));
            self.source_delay[0] = input.old_external.source_enabled;
            let full_blocks =
                !(self.refresh_history[0][1] || self.refresh_history[2][1]) && input.fifo_full;
            #[cfg(test)]
            {
                full_pressure = full_blocks
                    && self.source_delay[1]
                    && input.old_external.source_enabled
                    && input.memory_beat;
            }
            self.blocked = !full_blocks
                && self.source_delay[1]
                && input.old_external.source_enabled
                && input.memory_beat;
        }
        if phase.horizontal_slave {
            for pair in &mut self.refresh_history {
                pair[1] = pair[0];
            }
            self.quiet[1] = self.quiet[0];
            self.source_delay[1] = self.source_delay[0];
        }
        if phase.data_master {
            self.release = !self.blocked;
        }
        if phase.data_slave {
            self.selected = !self.release;
        }
        let external_address_cycle =
            input.old_external.source_enabled && self.quiet[1] && self.selected;
        let read_lanes = pins.read && (pins.upper_strobe || pins.lower_strobe);
        let write_enable = !pins.read && self.write_enable.value();
        let first_recovery = self.recovery[1].value() && !self.recovery[3].value();
        let second_recovery = self.recovery[0].value() && !self.recovery[3].value();
        let clear = !self.divider_enabled.value();
        let initial = !(clear || self.stretch[4].value());
        let divider_window = self.stretch[1].value() || !self.stretch[3].value();
        let cpu = input.cpu_clock1;
        self.stretch[0].sample(!cpu, self.stretch[1].value(), clear);
        self.stretch[1].sample(cpu, self.stretch[2].value(), clear);
        self.stretch[2].sample(!cpu, self.stretch[3].value(), clear);
        self.stretch[3].sample(!cpu, self.stretch[4].value(), clear);
        let terminal_clear =
            input.reset_comb || self.recovery[5].value() || self.divider_enabled.value();
        let latched_read = self.address_seen.value() && read_lanes;
        let ram_selected = !pins.interrupt_acknowledge && address & 0x700000 == 0x700000;
        let recovery_clear = !self.terminal.value();
        let address_active = pins.address_strobe && (recovery_clear || self.recovery[5].value());
        let access_window = ram_selected && address_active;
        let acknowledge =
            access_window && (self.stretch[1].value() || self.address_seen.value() && clear);
        let column_window = access_window && self.address_delay.value();
        let row_window = access_window && divider_window;
        self.stretch[4].sample(cpu, column_window, clear);
        let write_gate = !((clear && column_window) || self.stretch[0].value() && access_window);
        self.write_enable.sample(!cpu, write_gate, false);
        self.address_delay
            .sample(!cpu, self.address_seen.value(), !address_active);
        self.address_seen
            .sample(cpu, address_active, !address_active);
        self.address_released
            .sample(!pins.address_strobe, true, clear);
        let overflow = self.divider[4..].iter().all(|cell| cell.value());
        let final_count = overflow && self.divider[1..4].iter().all(|cell| cell.value());
        self.divider_enabled.sample(
            access_window,
            overflow,
            self.address_released.value() || input.reset_comb,
        );
        self.terminal_delay.sample(cpu, final_count, false);
        self.terminal
            .sample(self.terminal_delay.value(), overflow, terminal_clear);
        self.recovery[0].sample(cpu, true, recovery_clear);
        self.recovery[1].sample(cpu, self.recovery[0].value(), recovery_clear);
        self.recovery[2].sample(cpu, self.recovery[1].value(), recovery_clear);
        self.recovery[3].sample(!cpu, self.recovery[2].value(), recovery_clear);
        self.recovery[4].sample(cpu, self.recovery[3].value(), recovery_clear);
        self.recovery[5].sample(cpu, self.recovery[4].value(), false);
        let divider_clear = self.recovery[5].value() || !divider_window;
        let mut carry = !input.old_external.bus_grant_acknowledge;
        for cell in &mut self.divider {
            let old = cell.value();
            cell.sample(cpu, old ^ carry, divider_clear);
            carry &= old;
        }
        let read_enable = latched_read
            && (input.register11_before & 64 != 0 && column_window || acknowledge)
            || initial && access_window;
        Access {
            #[cfg(test)]
            full_pressure,
            external_address_cycle,
            acknowledge,
            read_enable,
            write_enable,
            ram_selected,
            access_window,
            row_window,
            column_window,
            first_recovery,
            second_recovery,
            divider_clear: clear,
            read_lanes,
        }
    }

    pub fn ram_pins(&self, access: Access, byte_dma: bool, wide: bool) -> (bool, bool, bool) {
        let byte_window = self.quiet[1] && byte_dma;
        let external_row = (!self.release || self.selected) && byte_window;
        let mux = external_row || self.address_seen.value();
        let (column, row) = if wide {
            (
                !self.quiet[1] && self.blocked
                    || self.selected && byte_window
                    || access.second_recovery
                    || access.column_window,
                access.row_window
                    || !self.release
                    || self.blocked && byte_window
                    || access.first_recovery,
            )
        } else {
            (
                self.selected || access.read_lanes && !access.ram_selected,
                (if access.divider_clear {
                    access.row_window
                } else {
                    access.access_window && self.stretch[2].value()
                }) || external_row,
            )
        };
        (mux, column, row)
    }

    pub fn address22(&self, word_dma: bool) -> bool {
        !(self.quiet[1] && word_dma && (self.blocked || !self.release))
    }
}
