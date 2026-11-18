use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};

fn write_pair(writer: &mut StateWriter, pair: [bool; 2]) {
    writer.write_bool(pair[0]);
    writer.write_bool(pair[1]);
}

fn read_pair(reader: &mut StateReader<'_>) -> Result<[bool; 2]> {
    Ok([reader.read_bool()?, reader.read_bool()?])
}

impl Frontend {
    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(1);
        let d = self.dma;
        for dff in [d.grant, d.request_delay, d.active_delay, d.active] {
            write_pair(writer, [dff.master, dff.slave]);
        }
        for value in [
            d.internal_request,
            d.granted_request,
            d.external_request,
            d.fill_seeded,
            d.external_cycle,
            d.source_enabled,
            d.grant_acknowledge,
        ] {
            writer.write_bool(value);
        }
        for pair in [
            d.cycle,
            d.produce,
            d.full,
            d.retire,
            d.retire_delay,
            d.empty,
            d.terminal,
            d.selected,
            d.fill_selected,
        ] {
            write_pair(writer, pair);
        }
        let p = self.port;
        for value in [
            p.dma_ready,
            p.read_cycle_ready,
            p.byte_second,
            p.byte_second_n,
            p.byte_pending,
            p.byte_pending_n,
            p.upper,
            p.lower,
            p.pending,
            p.read_ready,
            p.read_request,
            p.read_request_n,
            p.read_enable,
        ] {
            writer.write_bool(value);
        }
        for pair in [
            p.read_selected,
            p.write_selected,
            p.read_kind,
            p.read_kind_delay,
        ] {
            write_pair(writer, pair);
        }
        let f = self.fifo;
        writer.write_bytes(&f.producer);
        writer.write_bytes(&f.consumer);
        for cell in f.cells {
            writer.write_u32(cell.address);
            writer.write_u16(cell.word);
            writer.write_bool(cell.upper);
            writer.write_bool(cell.lower);
            writer.write_u8(cell.code);
        }
        writer.write_u16(f.bus_word);
        writer.write_u32(f.previous_address);
        writer.write_u8(f.code_bits);
        writer.write_bool(f.write);
        writer.write_bytes(&f.low_delay);
        let r = self.read;
        writer.write_bool(r.odd);
        for pair in [
            r.ready,
            r.address,
            r.byte,
            r.byte_delay,
            r.selected,
            r.selected_delay,
            r.low_capture,
            r.high_capture,
        ] {
            write_pair(writer, pair);
        }
        writer.write_u8(r.low);
        writer.write_u8(r.high);
        writer.write_u64(self.sequence);
        writer.write_bool(self.launched.is_some());
        if let Some(receipt) = self.launched {
            writer.write_u64(receipt.sequence);
            writer.write_u8(match receipt.write.target {
                MemoryTarget::Vram => 0,
                MemoryTarget::Cram => 1,
                MemoryTarget::Vsram => 2,
            });
            writer.write_u32(receipt.write.address);
            writer.write_u16(receipt.write.data);
            writer.write_bool(receipt.write.upper);
            writer.write_bool(receipt.write.lower);
        }
    }

    pub fn read_state(reader: &mut StateReader<'_>, control: &ControlCore) -> Result<Self> {
        ensure!(reader.read_u8()? == 1, "unsupported VDP frontend version");
        let mut dffs = [dma::Dff::zero(); 4];
        for dff in &mut dffs {
            let pair = read_pair(reader)?;
            *dff = dma::Dff {
                master: pair[0],
                slave: pair[1],
            };
        }
        let dma = Dma {
            grant: dffs[0],
            request_delay: dffs[1],
            active_delay: dffs[2],
            active: dffs[3],
            internal_request: reader.read_bool()?,
            granted_request: reader.read_bool()?,
            external_request: reader.read_bool()?,
            fill_seeded: reader.read_bool()?,
            external_cycle: reader.read_bool()?,
            source_enabled: reader.read_bool()?,
            grant_acknowledge: reader.read_bool()?,
            cycle: read_pair(reader)?,
            produce: read_pair(reader)?,
            full: read_pair(reader)?,
            retire: read_pair(reader)?,
            retire_delay: read_pair(reader)?,
            empty: read_pair(reader)?,
            terminal: read_pair(reader)?,
            selected: read_pair(reader)?,
            fill_selected: read_pair(reader)?,
        };
        let port = Port {
            dma_ready: reader.read_bool()?,
            read_cycle_ready: reader.read_bool()?,
            byte_second: reader.read_bool()?,
            byte_second_n: reader.read_bool()?,
            byte_pending: reader.read_bool()?,
            byte_pending_n: reader.read_bool()?,
            upper: reader.read_bool()?,
            lower: reader.read_bool()?,
            pending: reader.read_bool()?,
            read_ready: reader.read_bool()?,
            read_request: reader.read_bool()?,
            read_request_n: reader.read_bool()?,
            read_enable: reader.read_bool()?,
            read_selected: read_pair(reader)?,
            write_selected: read_pair(reader)?,
            read_kind: read_pair(reader)?,
            read_kind_delay: read_pair(reader)?,
        };
        let mut producer = [0; 2];
        let mut consumer = [0; 2];
        reader.read_exact(&mut producer)?;
        reader.read_exact(&mut consumer)?;
        let mut cells = Fifo::software_origin().cells;
        for cell in &mut cells {
            cell.address = reader.read_u32()?;
            cell.word = reader.read_u16()?;
            cell.upper = reader.read_bool()?;
            cell.lower = reader.read_bool()?;
            cell.code = reader.read_u8()?;
        }
        let bus_word = reader.read_u16()?;
        let previous_address = reader.read_u32()?;
        let code_bits = reader.read_u8()?;
        let write = reader.read_bool()?;
        let mut low_delay = [0; 2];
        reader.read_exact(&mut low_delay)?;
        let fifo = Fifo {
            producer,
            consumer,
            cells,
            bus_word,
            previous_address,
            code_bits,
            write,
            low_delay,
        };
        let read = Read {
            odd: reader.read_bool()?,
            ready: read_pair(reader)?,
            address: read_pair(reader)?,
            byte: read_pair(reader)?,
            byte_delay: read_pair(reader)?,
            selected: read_pair(reader)?,
            selected_delay: read_pair(reader)?,
            low_capture: read_pair(reader)?,
            high_capture: read_pair(reader)?,
            low: reader.read_u8()?,
            high: reader.read_u8()?,
        };
        let sequence = reader.read_u64()?;
        let launched = if reader.read_bool()? {
            let sequence = reader.read_u64()?;
            let target = match reader.read_u8()? {
                0 => MemoryTarget::Vram,
                1 => MemoryTarget::Cram,
                2 => MemoryTarget::Vsram,
                _ => anyhow::bail!("invalid VDP launched write target"),
            };
            Some(WriteReceipt {
                sequence,
                write: MemoryWrite {
                    target,
                    address: reader.read_u32()?,
                    data: reader.read_u16()?,
                    upper: reader.read_bool()?,
                    lower: reader.read_bool()?,
                },
            })
        } else {
            None
        };
        let value = Self {
            port,
            dma,
            fifo,
            read,
            sequence,
            launched,
        };
        value.validate_state(control)?;
        Ok(value)
    }

    pub fn validate_state(&self, control: &ControlCore) -> Result<()> {
        control.validate_state()?;
        ensure!(
            !self.port.byte_second && self.port.byte_second_n,
            "invalid VDP byte intake pair"
        );
        ensure!(
            !self.port.byte_pending && self.port.byte_pending_n,
            "invalid VDP byte pending pair"
        );
        ensure!(
            self.port.read_request || self.port.read_request_n,
            "invalid VDP read request pair"
        );
        ensure!(
            self.fifo.producer.into_iter().all(|value| value < 4)
                && self.fifo.consumer.into_iter().all(|value| value < 8),
            "invalid VDP FIFO pointer"
        );
        ensure!(
            self.fifo.previous_address <= 0x1ffff && self.fifo.code_bits & !14 == 0,
            "invalid VDP FIFO drive state"
        );
        ensure!(
            self.fifo
                .cells
                .into_iter()
                .all(|cell| cell.address <= 0x1ffff && cell.code < 16),
            "invalid VDP FIFO cell"
        );
        if let Some(receipt) = self.launched {
            ensure!(
                receipt.sequence != 0 && receipt.sequence == self.sequence,
                "invalid VDP launched write sequence"
            );
            receipt.write.validate()?;
        }
        Ok(())
    }
}
