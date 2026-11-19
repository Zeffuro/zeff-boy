use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};

fn pair(writer: &mut StateWriter, value: [bool; 2]) {
    writer.write_bool(value[0]);
    writer.write_bool(value[1]);
}

fn read_pair(reader: &mut StateReader<'_>) -> Result<[bool; 2]> {
    Ok([reader.read_bool()?, reader.read_bool()?])
}

impl Wiring {
    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(1);
        writer.write_u32(self.address);
        let p = self.pins;
        writer.write_u32(p.address);
        writer.write_u16(p.data);
        writer.write_u8(p.function_code as u8);
        for bit in [
            p.address_strobe,
            p.read,
            p.upper_strobe,
            p.lower_strobe,
            p.interrupt_acknowledge,
            p.sound_vdp_request,
            p.test_mode,
        ] {
            writer.write_bool(bit);
        }
        let e = self.external;
        for value in e.refresh_history {
            pair(writer, value);
        }
        pair(writer, e.quiet);
        pair(writer, e.source_delay);
        for bit in [e.blocked, e.release, e.selected] {
            writer.write_bool(bit);
        }
        pair(writer, e.write_enable.0);
        for cell in e.stretch {
            pair(writer, cell.0);
        }
        for cell in [
            e.address_delay,
            e.address_seen,
            e.address_released,
            e.divider_enabled,
            e.terminal_delay,
            e.terminal,
        ] {
            pair(writer, cell.0);
        }
        for cell in e.recovery {
            pair(writer, cell.0);
        }
        for cell in e.divider {
            pair(writer, cell.0);
        }
        let h = self.held;
        for bit in [
            h.acknowledge,
            h.output_enable,
            h.column_strobe,
            h.row_strobe,
            h.upper_write,
            h.lower_write,
            h.address22,
            h.data_read,
            h.status_read,
            h.hv_read,
            h.psg_write,
        ] {
            writer.write_bool(bit);
        }
        writer.write_u8(h.ram_address);
    }

    pub fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        ensure!(reader.read_u8()? == 1, "unsupported VDP wiring version");
        let address = reader.read_u32()?;
        let pin_address = reader.read_u32()?;
        let data = reader.read_u16()?;
        let function_code = match reader.read_u8()? {
            1 => FunctionCode::UserData,
            2 => FunctionCode::UserProgram,
            5 => FunctionCode::SupervisorData,
            6 => FunctionCode::SupervisorProgram,
            7 => FunctionCode::CpuSpace,
            _ => anyhow::bail!("invalid MAIN function code"),
        };
        let pins = BoardPins {
            address: pin_address,
            data,
            function_code,
            address_strobe: reader.read_bool()?,
            read: reader.read_bool()?,
            upper_strobe: reader.read_bool()?,
            lower_strobe: reader.read_bool()?,
            interrupt_acknowledge: reader.read_bool()?,
            sound_vdp_request: reader.read_bool()?,
            test_mode: reader.read_bool()?,
        };
        let mut external = external::External::default();
        for value in &mut external.refresh_history {
            *value = read_pair(reader)?;
        }
        external.quiet = read_pair(reader)?;
        external.source_delay = read_pair(reader)?;
        external.blocked = reader.read_bool()?;
        external.release = reader.read_bool()?;
        external.selected = reader.read_bool()?;
        external.write_enable.0 = read_pair(reader)?;
        for cell in &mut external.stretch {
            cell.0 = read_pair(reader)?;
        }
        for cell in [
            &mut external.address_delay,
            &mut external.address_seen,
            &mut external.address_released,
            &mut external.divider_enabled,
            &mut external.terminal_delay,
            &mut external.terminal,
        ] {
            cell.0 = read_pair(reader)?;
        }
        for cell in &mut external.recovery {
            cell.0 = read_pair(reader)?;
        }
        for cell in &mut external.divider {
            cell.0 = read_pair(reader)?;
        }
        let held = PinLevels {
            acknowledge: reader.read_bool()?,
            output_enable: reader.read_bool()?,
            column_strobe: reader.read_bool()?,
            row_strobe: reader.read_bool()?,
            upper_write: reader.read_bool()?,
            lower_write: reader.read_bool()?,
            address22: reader.read_bool()?,
            data_read: reader.read_bool()?,
            status_read: reader.read_bool()?,
            hv_read: reader.read_bool()?,
            psg_write: reader.read_bool()?,
            ram_address: reader.read_u8()?,
        };
        let value = Self {
            external,
            pins,
            address,
            held,
        };
        value.validate_state()?;
        Ok(value)
    }

    pub fn validate_state(&self) -> Result<()> {
        self.pins.validate()?;
        ensure!(self.address <= 0x7fffff, "invalid held VDP-side address");
        let h = self.held;
        ensure!(
            u8::from(h.data_read) + u8::from(h.status_read) + u8::from(h.hv_read) <= 1,
            "conflicting VDP read drives"
        );
        ensure!(
            !h.psg_write || !(h.data_read || h.status_read || h.hv_read),
            "conflicting VDP/PSG drives"
        );
        Ok(())
    }
}
