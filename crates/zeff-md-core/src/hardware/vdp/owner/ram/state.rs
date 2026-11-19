use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};

fn write_pair(writer: &mut StateWriter, pair: [bool; 2]) {
    writer.write_bool(pair[0]);
    writer.write_bool(pair[1]);
}

fn read_pair(reader: &mut StateReader<'_>) -> Result<[bool; 2]> {
    Ok([reader.read_bool()?, reader.read_bool()?])
}

fn write_pins(writer: &mut StateWriter, pins: Pins) {
    for value in [
        pins.ras,
        pins.cas,
        pins.we0,
        pins.we1,
        pins.oe,
        pins.sc,
        pins.se0,
        pins.se1,
        pins.driven,
    ] {
        writer.write_bool(value);
    }
    writer.write_u8(pins.ad);
    writer.write_u8(pins.rd);
}

fn read_pins(reader: &mut StateReader<'_>) -> Result<Pins> {
    Ok(Pins {
        ras: reader.read_bool()?,
        cas: reader.read_bool()?,
        we0: reader.read_bool()?,
        we1: reader.read_bool()?,
        oe: reader.read_bool()?,
        sc: reader.read_bool()?,
        se0: reader.read_bool()?,
        se1: reader.read_bool()?,
        driven: reader.read_bool()?,
        ad: reader.read_u8()?,
        rd: reader.read_u8()?,
    })
}

impl Ram {
    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(3);
        let c = self.controller;
        for value in [
            c.address_idle,
            c.column_open,
            c.column_close,
            c.write_idle,
            c.write_late,
            c.service_idle,
            c.service_late,
            c.output_idle,
            c.data_idle,
            c.data_open,
            c.data_close,
            c.ram_idle,
            c.write_hold_wire,
            c.write_active_wire,
            c.drive_decode_wire,
            c.read_decode_wire,
            c.read_idle_wire,
            c.read_capture_wire,
            c.output_idle_wire,
            c.read_stage_wire,
            c.bank_select_wire,
            c.refresh_wire,
            c.upper_hold_wire,
        ] {
            writer.write_bool(value);
        }
        for pair in [
            c.address_phase,
            c.write_hold,
            c.write_stage,
            c.service_stage,
            c.data_phase,
            c.copy_delay,
            c.copy_address,
            c.read_stage,
            c.upper_hold,
            c.upper_stage,
            c.bank_select,
            c.refresh_output,
            c.refresh_stage,
            c.refresh_late,
            c.service_late_stage,
            c.copy_select,
        ] {
            write_pair(writer, pair);
        }
        writer.write_bytes(&[
            c.data_select_wire,
            c.address_row,
            c.address_column,
            c.ad_hold,
            c.data_low,
            c.data_high,
            c.rd_hold,
            c.ram_ad,
            c.ram_rd,
        ]);
        write_pins(writer, c.pins);
        writer.write_u16(self.responder.address);
        write_pins(writer, self.responder.previous);
        writer.write_bool(self.responder.transfer);
        writer.write_u64(self.responder.write_sequence);
    }

    pub fn read_state(reader: &mut StateReader<'_>, profile: Profile) -> Result<Self> {
        ensure!(
            reader.read_u8()? == 3,
            "unsupported VDP VRAM backend version"
        );
        let controller = Controller {
            address_idle: reader.read_bool()?,
            column_open: reader.read_bool()?,
            column_close: reader.read_bool()?,
            write_idle: reader.read_bool()?,
            write_late: reader.read_bool()?,
            service_idle: reader.read_bool()?,
            service_late: reader.read_bool()?,
            output_idle: reader.read_bool()?,
            data_idle: reader.read_bool()?,
            data_open: reader.read_bool()?,
            data_close: reader.read_bool()?,
            ram_idle: reader.read_bool()?,
            write_hold_wire: reader.read_bool()?,
            write_active_wire: reader.read_bool()?,
            drive_decode_wire: reader.read_bool()?,
            read_decode_wire: reader.read_bool()?,
            read_idle_wire: reader.read_bool()?,
            read_capture_wire: reader.read_bool()?,
            output_idle_wire: reader.read_bool()?,
            read_stage_wire: reader.read_bool()?,
            bank_select_wire: reader.read_bool()?,
            refresh_wire: reader.read_bool()?,
            upper_hold_wire: reader.read_bool()?,
            address_phase: read_pair(reader)?,
            write_hold: read_pair(reader)?,
            write_stage: read_pair(reader)?,
            service_stage: read_pair(reader)?,
            data_phase: read_pair(reader)?,
            copy_delay: read_pair(reader)?,
            copy_address: read_pair(reader)?,
            read_stage: read_pair(reader)?,
            upper_hold: read_pair(reader)?,
            upper_stage: read_pair(reader)?,
            bank_select: read_pair(reader)?,
            refresh_output: read_pair(reader)?,
            refresh_stage: read_pair(reader)?,
            refresh_late: read_pair(reader)?,
            service_late_stage: read_pair(reader)?,
            copy_select: read_pair(reader)?,
            data_select_wire: reader.read_u8()?,
            address_row: reader.read_u8()?,
            address_column: reader.read_u8()?,
            ad_hold: reader.read_u8()?,
            data_low: reader.read_u8()?,
            data_high: reader.read_u8()?,
            rd_hold: reader.read_u8()?,
            ram_ad: reader.read_u8()?,
            ram_rd: reader.read_u8()?,
            pins: read_pins(reader)?,
        };
        let responder = Responder {
            address: reader.read_u16()?,
            previous: read_pins(reader)?,
            transfer: reader.read_bool()?,
            write_sequence: reader.read_u64()?,
        };
        let value = Self {
            controller,
            responder,
        };
        value.validate_state(profile)?;
        Ok(value)
    }

    pub fn validate_state(&self, profile: Profile) -> Result<()> {
        profile.validate()?;
        ensure!(
            self.controller.pins.se0 == self.controller.bank_select[1],
            "VRAM select pins differ from retained bank latch"
        );
        for pins in [self.controller.pins, self.responder.previous] {
            ensure!(pins.se0 != pins.se1, "invalid VRAM select pins");
        }
        let pins = self.responder.previous;
        ensure!(
            pins.ras || pins.cas || pins.we0 || self.responder.write_sequence != 0,
            "missing VRAM write receipt history"
        );
        self.validate_frontier()
    }
}
