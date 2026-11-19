use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};

fn write_pair(writer: &mut StateWriter, pair: [bool; 2]) {
    writer.write_bool(pair[0]);
    writer.write_bool(pair[1]);
}

fn read_pair(reader: &mut StateReader<'_>) -> Result<[bool; 2]> {
    Ok([reader.read_bool()?, reader.read_bool()?])
}

impl InternalRam {
    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(5);
        let v = self.vsram;
        for word in v.data {
            writer.write_u16(word);
        }
        writer.write_bytes(&v.high);
        for pair in [
            v.return_gate,
            v.read,
            v.upper,
            v.lower,
            v.read_delay,
            v.upper_delay,
            v.lower_delay,
        ] {
            write_pair(writer, pair);
        }
        writer.write_bytes(&v.address);
        writer.write_bytes(&v.cpu_address);
        for word in [v.return_hold, v.out, v.even, v.odd] {
            writer.write_u16(word);
        }
        let c = self.cram;
        for pair in [c.upper, c.lower] {
            write_pair(writer, pair);
        }
        writer.write_bytes(&c.address);
        writer.write_bytes(&c.blue);
        writer.write_bytes(&c.read_delay);
        c.palette.write_state(writer);
        c.rgb.write_state(writer);
        writer.write_u8(c.address_wire);
        writer.write_u16(c.out);
        writer.write_u16(c.return_hold);
        writer.write_bool(c.return_capture);
        for tag in c.address_tag.into_iter().chain([c.wire_tag, c.out_tag]) {
            raster::write_tag(writer, tag);
        }
        writer.write_u64(self.sequence);
    }

    pub fn read_state(reader: &mut StateReader<'_>, profile: Profile) -> Result<Self> {
        ensure!(
            reader.read_u8()? == 5,
            "unsupported internal VDP RAM version"
        );
        let data = [reader.read_u16()?, reader.read_u16()?];
        let mut high = [0; 2];
        reader.read_exact(&mut high)?;
        let return_gate = read_pair(reader)?;
        let read = read_pair(reader)?;
        let upper = read_pair(reader)?;
        let lower = read_pair(reader)?;
        let read_delay = read_pair(reader)?;
        let upper_delay = read_pair(reader)?;
        let lower_delay = read_pair(reader)?;
        let mut address = [0; 2];
        reader.read_exact(&mut address)?;
        let mut cpu_address = [0; 2];
        reader.read_exact(&mut cpu_address)?;
        let vsram = Vsram {
            data,
            high,
            return_gate,
            read,
            upper,
            lower,
            read_delay,
            upper_delay,
            lower_delay,
            address,
            cpu_address,
            return_hold: reader.read_u16()?,
            out: reader.read_u16()?,
            even: reader.read_u16()?,
            odd: reader.read_u16()?,
        };
        let upper = read_pair(reader)?;
        let lower = read_pair(reader)?;
        let mut address = [0; 2];
        reader.read_exact(&mut address)?;
        let mut blue = [0; 2];
        reader.read_exact(&mut blue)?;
        let mut read_delay = [0; 2];
        reader.read_exact(&mut read_delay)?;
        let cram = Cram {
            upper,
            lower,
            address,
            blue,
            read_delay,
            palette: Palette::read_state(reader)?,
            rgb: Rgb::read_state(reader)?,
            address_wire: reader.read_u8()?,
            out: reader.read_u16()?,
            return_hold: reader.read_u16()?,
            return_capture: reader.read_bool()?,
            address_tag: [raster::read_tag(reader)?, raster::read_tag(reader)?],
            wire_tag: raster::read_tag(reader)?,
            out_tag: raster::read_tag(reader)?,
        };
        let value = Self {
            vsram,
            cram,
            sequence: reader.read_u64()?,
        };
        value.validate_state(profile)?;
        Ok(value)
    }

    pub fn validate_state(&self, profile: Profile) -> Result<()> {
        profile.validate()?;
        let v = self.vsram;
        ensure!(
            v.data
                .into_iter()
                .chain([v.return_hold, v.out, v.even, v.odd])
                .all(|word| word <= 0x7ff),
            "invalid VSRAM data latch"
        );
        ensure!(
            v.high.into_iter().all(|value| value < 8)
                && v.address
                    .into_iter()
                    .chain(v.cpu_address)
                    .all(|value| value < 64),
            "invalid VSRAM address or high latch"
        );
        let c = self.cram;
        ensure!(
            c.address
                .into_iter()
                .chain([c.address_wire])
                .all(|value| value < 64)
                && c.blue
                    .into_iter()
                    .chain(c.read_delay)
                    .all(|value| value < 8),
            "invalid CRAM address or data latch"
        );
        ensure!(
            c.out <= 0x1ff && c.return_hold <= 0x1ff,
            "invalid packed CRAM data"
        );
        c.palette.validate()?;
        c.rgb.validate()?;
        c.rgb.validate_input(
            profile.memory.registers[0],
            profile.memory.registers[1] & 4 != 0,
        )?;
        ensure!(
            c.wire_tag == c.palette.tag() && c.out_tag == c.address_tag[1],
            "stale CRAM pixel provenance"
        );
        ensure!(
            c.address_tag
                .into_iter()
                .chain([c.wire_tag, c.out_tag])
                .all(|tag| tag.is_none_or(|tag| tag.field <= c.palette.field())),
            "CRAM tag exceeds raster frontier"
        );
        c.rgb.validate_frontier(c.palette.field())?;
        Ok(())
    }
}
