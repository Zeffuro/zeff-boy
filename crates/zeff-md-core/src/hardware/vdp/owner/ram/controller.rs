use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Controller {
    pub address_idle: bool,
    pub address_phase: [bool; 2],
    pub column_open: bool,
    pub column_close: bool,
    pub write_hold: [bool; 2],
    pub write_idle: bool,
    pub write_late: bool,
    pub write_stage: [bool; 2],
    pub service_idle: bool,
    pub service_late: bool,
    pub service_stage: [bool; 2],
    pub output_idle: bool,
    pub data_phase: [bool; 2],
    pub data_idle: bool,
    pub data_open: bool,
    pub data_close: bool,
    pub ram_idle: bool,
    pub copy_delay: [bool; 2],
    pub copy_address: [bool; 2],
    pub read_stage: [bool; 2],
    pub upper_hold: [bool; 2],
    pub upper_stage: [bool; 2],
    pub bank_select: [bool; 2],
    pub refresh_output: [bool; 2],
    pub refresh_stage: [bool; 2],
    pub refresh_late: [bool; 2],
    pub service_late_stage: [bool; 2],
    pub copy_select: [bool; 2],
    pub write_hold_wire: bool,
    pub write_active_wire: bool,
    pub drive_decode_wire: bool,
    pub read_decode_wire: bool,
    pub read_idle_wire: bool,
    pub read_capture_wire: bool,
    pub output_idle_wire: bool,
    pub read_stage_wire: bool,
    pub bank_select_wire: bool,
    pub refresh_wire: bool,
    pub upper_hold_wire: bool,
    pub data_select_wire: u8,
    pub address_row: u8,
    pub address_column: u8,
    pub ad_hold: u8,
    pub data_low: u8,
    pub data_high: u8,
    pub rd_hold: u8,
    pub ram_ad: u8,
    pub ram_rd: u8,
    pub pins: Pins,
}

impl Controller {
    pub fn software_origin() -> Self {
        Self {
            address_idle: true,
            address_phase: [true; 2],
            column_open: false,
            column_close: true,
            write_hold: [false; 2],
            write_idle: true,
            write_late: false,
            write_stage: [false; 2],
            service_idle: true,
            service_late: false,
            service_stage: [false; 2],
            output_idle: true,
            data_phase: [false; 2],
            data_idle: true,
            data_open: false,
            data_close: false,
            ram_idle: true,
            copy_delay: [false; 2],
            copy_address: [false; 2],
            read_stage: [false; 2],
            upper_hold: [false; 2],
            upper_stage: [false; 2],
            bank_select: [false; 2],
            refresh_output: [false; 2],
            refresh_stage: [false; 2],
            refresh_late: [false; 2],
            service_late_stage: [false; 2],
            copy_select: [false; 2],
            write_hold_wire: false,
            write_active_wire: false,
            drive_decode_wire: false,
            read_decode_wire: false,
            read_idle_wire: true,
            read_capture_wire: true,
            output_idle_wire: false,
            read_stage_wire: false,
            bank_select_wire: false,
            refresh_wire: false,
            upper_hold_wire: false,
            data_select_wire: 0,
            address_row: 0,
            address_column: 0,
            ad_hold: 0,
            data_low: 0,
            data_high: 0,
            rd_hold: 0,
            ram_ad: 0,
            ram_rd: 0,
            pins: Pins::software_origin(),
        }
    }

    fn clocks(&mut self, input: Inputs) {
        let p = input.phases;
        if p.horizontal_master {
            self.address_idle = !input.memory_window;
            self.write_hold[0] = self.write_hold_wire;
            self.write_idle = !self.write_active_wire;
            self.write_stage[0] = input.frontend.lower_write;
            self.service_idle = !self.service_late_stage[1];
            self.service_stage[0] = input.opportunity;
            self.output_idle = !self.output_idle_wire;
            self.ram_idle = !self.read_idle_wire;
            self.copy_delay[0] = self.copy_address[1];
            self.copy_address[0] = input.copy_read_address_selected;
            self.read_stage[0] = self.read_stage_wire;
            self.upper_hold[0] = self.upper_hold_wire;
            self.upper_stage[0] = input.frontend.upper_write;
            self.refresh_output[0] = self.refresh_wire;
            self.refresh_stage[0] = input.refresh;
            self.refresh_late[0] = self.refresh_stage[1];
            self.service_late_stage[0] = self.service_stage[1];
            self.copy_select[0] = input.frontend.copy;
        }
        if p.horizontal_slave {
            self.write_hold[1] = self.write_hold[0];
            self.write_late = !self.write_idle;
            self.write_stage[1] = self.write_stage[0];
            self.service_late = !self.service_idle;
            self.service_stage[1] = self.service_stage[0];
            self.copy_delay[1] = self.copy_delay[0];
            self.copy_address[1] = self.copy_address[0];
            self.read_stage[1] = self.read_stage[0];
            self.upper_hold[1] = self.upper_hold[0];
            self.upper_stage[1] = self.upper_stage[0];
            self.refresh_output[1] = self.refresh_output[0];
            self.refresh_stage[1] = self.refresh_stage[0];
            self.refresh_late[1] = self.refresh_late[0];
            self.service_late_stage[1] = self.service_late_stage[0];
            self.copy_select[1] = self.copy_select[0];
        }
        if p.data_master {
            self.address_phase[0] = self.address_idle;
            self.column_open = !self.address_phase[1];
            self.data_phase[0] = !self.address_idle;
            self.data_idle = !self.data_phase[1];
            self.data_close = self.data_open;
            self.bank_select[0] = self.bank_select_wire;
        }
        if p.data_slave {
            self.address_phase[1] = self.address_phase[0];
            self.column_close = !self.column_open;
            self.data_phase[1] = self.data_phase[0];
            self.data_open = !self.data_idle;
            self.bank_select[1] = self.bank_select[0];
        }
    }

    pub fn evaluate(&mut self, input: Inputs) -> Option<u16> {
        self.clocks(input);
        let oe = self.address_phase[1] && self.read_decode_wire && self.data_close
            || self.output_idle && self.data_phase[1];
        let we0 = self.write_hold[1] && self.data_idle && self.data_close;
        let we1 = self.data_idle && self.upper_hold[1] && self.data_close;
        let cas = self.column_open && self.refresh_output[1] || self.data_close;
        let ras = self.column_open && !self.refresh_output[1] || self.data_open;
        let drive = self.drive_decode_wire && self.data_close || self.data_phase[1];
        self.write_hold_wire = input.frontend.lower_write || self.write_stage[1];
        self.drive_decode_wire = self.write_late || !self.write_idle;
        self.read_decode_wire =
            !self.drive_decode_wire && (self.service_late || !self.service_idle);
        self.read_idle_wire =
            !(input.memory_window && !input.frontend.copy && !input.frontend.fill
                || input.frontend.copy && self.copy_delay[1]);
        let high = self.data_phase[1] && self.column_close;
        let low = self.data_open && self.data_phase[1];
        let read_bus = self.data_idle && self.data_close;
        self.upper_hold_wire = input.frontend.upper_write || self.upper_stage[1];
        self.write_active_wire = self.upper_stage[1] || self.write_stage[1];
        self.output_idle_wire =
            self.service_stage[1] || self.service_late_stage[1] || self.refresh_wire;
        let capture_address = input.memory_window && input.phases.horizontal_master;
        self.read_capture_wire = input.phases.horizontal_slave && self.ram_idle;
        self.read_stage_wire = input.memory_window && input.frontend.read_kind_delay;
        self.bank_select_wire = input.profile.mode128 && input.phases.horizontal_master;
        self.refresh_wire = self.refresh_stage[1] || self.refresh_late[1];
        if capture_address {
            let address = input.frontend.address;
            let mode5 = input.profile.registers[1] & 4 != 0;
            let pair_bit = mode5 && address & 2 != 0;
            self.address_row = if input.profile.mode128 {
                let high = if input.profile.registers[11] & 16 != 0 {
                    input.profile.registers[11] & 32 != 0
                } else {
                    address & 0x10000 != 0
                };
                (u32::from(pair_bit) | (address >> 9 & 0x7e) | (u32::from(high) << 7)) as u8
            } else {
                let row_bit = pair_bit || !mode5 && address & 0x200 != 0;
                ((address & 1) | u32::from(row_bit) << 1 | (address >> 8 & 0xfc)) as u8
            };
            self.address_column = (address >> if mode5 { 2 } else { 1 }) as u8;
        }
        let mut ad = 0;
        if low {
            ad |= self.address_row;
        }
        if high {
            ad |= self.address_column;
        }
        if self.address_phase[1] {
            ad |= self.ad_hold;
        }
        // AD samples the held selection before the low-data latch is replaced.
        if self.read_capture_wire {
            self.ad_hold = self.data_select_wire;
        }
        self.data_select_wire = if self.copy_select[1] {
            self.ram_ad
        } else {
            self.data_low
        };
        if capture_address {
            self.data_low = input.frontend.data as u8;
            self.data_high = (input.frontend.data >> 8) as u8;
        }
        let rd = if self.copy_select[1] {
            self.ram_rd
        } else {
            self.data_high
        };
        if self.read_capture_wire {
            self.rd_hold = rd;
        }
        if read_bus {
            self.ram_ad = input.bus.ad;
            self.ram_rd = input.bus.rd;
        }
        self.pins.ras = !ras;
        self.pins.cas = !cas;
        self.pins.we0 = !we0;
        self.pins.we1 = !we1;
        self.pins.oe = !oe;
        self.pins.sc = !(input.phases.data_master
            && (!input.profile.mode128 || input.phases.horizontal_slave));
        self.pins.se0 = self.bank_select[1];
        self.pins.se1 = !self.bank_select[1];
        self.pins.driven = drive;
        if drive {
            self.pins.ad = ad;
            self.pins.rd = self.rd_hold;
        }
        self.read_stage[1].then_some(u16::from(self.ram_ad) | (u16::from(self.ram_rd) << 8))
    }
}
