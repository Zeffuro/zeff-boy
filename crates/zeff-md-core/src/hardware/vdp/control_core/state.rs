use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

impl ControlCore {
    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(1);
        writer.write_u8(self.code);
        for value in [
            self.address.original,
            self.address.master,
            self.address.slave,
        ] {
            writer.write_u32(value);
        }
        writer.write_bool(self.address.select_original);
        let h = self.command;
        for value in [
            h.request,
            h.write_control,
            h.read_control,
            h.external_cycle,
            h.release_delay,
            h.busy_first,
            h.ready,
            h.busy_second,
            h.request_sample,
            h.cancel,
            h.second,
            h.second_n,
            h.second_pending,
            h.read_pending,
            h.first_pending,
            h.previous_data_read,
            self.load.enable,
            self.load.decode,
            self.load.master,
            self.load.slave,
            self.load.idle,
        ] {
            writer.write_bool(value);
        }
        writer.write_bytes(&self.registers.bytes);
        for value in self.registers.dma.length {
            writer.write_u16(value);
        }
        for value in self.registers.dma.source {
            writer.write_u16(value);
        }
        writer.write_u8(self.registers.dma.control);
    }

    pub fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        ensure!(
            reader.read_u8()? == 1,
            "unsupported VDP control core version"
        );
        let code = reader.read_u8()?;
        let address = Address {
            original: reader.read_u32()?,
            master: reader.read_u32()?,
            slave: reader.read_u32()?,
            select_original: reader.read_bool()?,
        };
        let command = Handshake {
            request: reader.read_bool()?,
            write_control: reader.read_bool()?,
            read_control: reader.read_bool()?,
            external_cycle: reader.read_bool()?,
            release_delay: reader.read_bool()?,
            busy_first: reader.read_bool()?,
            ready: reader.read_bool()?,
            busy_second: reader.read_bool()?,
            request_sample: reader.read_bool()?,
            cancel: reader.read_bool()?,
            second: reader.read_bool()?,
            second_n: reader.read_bool()?,
            second_pending: reader.read_bool()?,
            read_pending: reader.read_bool()?,
            first_pending: reader.read_bool()?,
            previous_data_read: reader.read_bool()?,
        };
        let load = LoadPipeline {
            enable: reader.read_bool()?,
            decode: reader.read_bool()?,
            master: reader.read_bool()?,
            slave: reader.read_bool()?,
            idle: reader.read_bool()?,
        };
        let mut bytes = [0; 19];
        reader.read_exact(&mut bytes)?;
        let dma = DmaBanks {
            length: [reader.read_u16()?, reader.read_u16()?],
            source: [reader.read_u16()?, reader.read_u16()?],
            control: reader.read_u8()?,
        };
        let value = Self {
            command,
            address,
            load,
            registers: RegisterBank { bytes, dma },
            code,
        };
        value.validate_state()?;
        Ok(value)
    }

    pub fn validate_state(&self) -> Result<()> {
        ensure!(self.code & !31 == 0, "invalid VDP control code");
        ensure!(
            [
                self.address.original,
                self.address.master,
                self.address.slave
            ]
            .into_iter()
            .all(|address| address <= 0x1ffff),
            "invalid VDP control address"
        );
        ensure!(
            self.command.second || self.command.second_n,
            "invalid VDP second-word latch pair"
        );
        self.registers.validate()
    }
}
