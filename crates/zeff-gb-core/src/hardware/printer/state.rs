use super::*;
use crate::save_state::{StateReader, StateWriter};

impl GameboyPrinter {
    pub(in crate::hardware) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(LOGICAL_STATE_FORMAT_MARKER | self.state.encode());
        writer.write_bytes(&self.header);
        writer.write_u64(self.header_pos as u64);
        writer.write_u64(self.payload_expected as u64);
        writer.write_u64(self.payload_received as u64);
        write_vec(writer, &self.payload);
        writer.write_u16(self.checksum);
        writer.write_u16(self.received_checksum);
        writer.write_u8(self.status);
        writer.write_u8(self.response_status);
        writer.write_bool(self.finish_print_after_status);
        writer.write_u64(self.busy_cycles_remaining);
        write_vec(writer, &self.data_buffer);
        writer.write_bool(self.data_end_seen);
        writer.write_u64(self.jobs.len() as u64);
        for job in &self.jobs {
            writer.write_u64(job.height as u64);
            write_vec(writer, &job.pixels);
            writer.write_u8(job.copies);
            writer.write_u8(job.feed_before);
            writer.write_u8(job.feed_after);
            writer.write_u8(job.palette);
            writer.write_u8(job.density);
        }
    }

    pub(in crate::hardware) fn read_state(
        reader: &mut StateReader<'_>,
        format_version: u32,
    ) -> Result<Self> {
        let state_tag = reader.read_u8()?;
        let expected_marker = if format_version >= 9 {
            LOGICAL_STATE_FORMAT_MARKER
        } else {
            LEGACY_STATE_FORMAT_MARKER
        };
        if state_tag & 0xC0 != expected_marker {
            bail!("Game Boy Printer state has an invalid format marker");
        }

        let state = ParserState::decode(state_tag & 0x3F)?;
        let mut header = [0; 4];
        reader.read_exact(&mut header)?;
        let header_pos = read_len(reader, header.len(), "header position")?;
        let payload_expected = read_len(reader, MAX_PACKET_PAYLOAD, "payload length")?;
        let payload_received = read_len(reader, payload_expected, "payload position")?;
        let payload = read_vec(reader, MAX_PACKET_PAYLOAD, "packet payload")?;
        if payload.len() != payload_received {
            bail!("Game Boy Printer payload position does not match stored payload");
        }
        validate_parser_state(state, header_pos, payload_expected, payload_received)?;

        let checksum = reader.read_u16()?;
        let received_checksum = reader.read_u16()?;
        let status = reader.read_u8()?;
        let response_status = reader.read_u8()?;
        let finish_print_after_status = reader.read_bool()?;
        let busy_cycles_remaining = reader.read_u64()?;
        if (status & STATUS_BUSY != 0) != (busy_cycles_remaining != 0) {
            bail!("Game Boy Printer busy status disagrees with its remaining duration");
        }
        let data_buffer = read_vec(reader, BUFFER_CAPACITY, "printer image buffer")?;
        let data_end_seen = reader.read_bool()?;
        let jobs = if format_version >= 9 {
            read_jobs(reader)?
        } else {
            read_legacy_images(reader, "RGBA image")?
        };

        Ok(Self {
            state,
            header,
            header_pos,
            payload,
            payload_expected,
            payload_received,
            checksum,
            received_checksum,
            status,
            response_status,
            finish_print_after_status,
            busy_cycles_remaining,
            data_buffer,
            data_end_seen,
            jobs,
        })
    }

    pub(in crate::hardware) fn read_legacy_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let state_tag = reader.read_u8()?;
        if state_tag > 2 {
            bail!("invalid legacy Game Boy Printer state: {state_tag}");
        }
        let mut legacy_command = [0; 5];
        reader.read_exact(&mut legacy_command)?;
        let _command_pos = reader.read_u64()?;
        let _data_expected = reader.read_u64()?;
        let _data_pos = reader.read_u64()?;
        let status = reader.read_u8()?;
        let jobs = read_legacy_images(reader, "legacy RGBA image")?;
        Ok(Self {
            status,
            jobs,
            ..Self::new()
        })
    }

    #[cfg(test)]
    pub(in crate::hardware) fn write_legacy_current_state(&self, writer: &mut StateWriter) {
        writer.write_u8(LEGACY_STATE_FORMAT_MARKER | self.state.encode());
        writer.write_bytes(&self.header);
        writer.write_u64(self.header_pos as u64);
        writer.write_u64(self.payload_expected as u64);
        writer.write_u64(self.payload_received as u64);
        write_vec(writer, &self.payload);
        writer.write_u16(self.checksum);
        writer.write_u16(self.received_checksum);
        writer.write_u8(self.status);
        writer.write_u8(self.response_status);
        writer.write_bool(self.finish_print_after_status);
        writer.write_u64(self.busy_cycles_remaining);
        write_vec(writer, &self.data_buffer);
        writer.write_bool(self.data_end_seen);
        writer.write_u64(0);
    }
}

fn write_vec(writer: &mut StateWriter, bytes: &[u8]) {
    writer.write_u64(bytes.len() as u64);
    writer.write_bytes(bytes);
}

fn read_len(reader: &mut StateReader<'_>, max: usize, name: &str) -> Result<usize> {
    let value = reader.read_u64()?;
    let value = usize::try_from(value).map_err(|_| anyhow::anyhow!("{name} does not fit usize"))?;
    if value > max {
        bail!("Game Boy Printer {name} {value} exceeds maximum {max}");
    }
    Ok(value)
}

fn read_vec(reader: &mut StateReader<'_>, max: usize, name: &str) -> Result<Vec<u8>> {
    let len = read_len(reader, max, name)?;
    let mut bytes = vec![0; len];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn read_jobs(reader: &mut StateReader<'_>) -> Result<Vec<GameBoyPrinterJob>> {
    let count = read_len(reader, MAX_SAVED_JOBS, "job count")?;
    let mut jobs = Vec::with_capacity(count);
    for _ in 0..count {
        let height = read_len(reader, GAME_BOY_PRINTER_MAX_HEIGHT, "job height")?;
        let pixels = read_vec(
            reader,
            GAME_BOY_PRINTER_WIDTH * GAME_BOY_PRINTER_MAX_HEIGHT,
            "logical pixels",
        )?;
        let job = GameBoyPrinterJob {
            pixels,
            height,
            copies: reader.read_u8()?,
            feed_before: reader.read_u8()?,
            feed_after: reader.read_u8()?,
            palette: reader.read_u8()?,
            density: reader.read_u8()?,
        };
        job.validate()?;
        jobs.push(job);
    }
    Ok(jobs)
}

fn read_legacy_images(reader: &mut StateReader<'_>, name: &str) -> Result<Vec<GameBoyPrinterJob>> {
    let count = read_len(reader, MAX_SAVED_JOBS, "legacy image count")?;
    let mut jobs = Vec::with_capacity(count);
    for _ in 0..count {
        let len = read_len(reader, LEGACY_PRINTER_RGBA_SIZE, name)?;
        let mut image = vec![0; len];
        reader.read_exact(&mut image)?;
        if image.len() == LEGACY_PRINTER_RGBA_SIZE {
            jobs.push(convert_legacy_image(&image));
        }
    }
    Ok(jobs)
}

fn convert_legacy_image(rgba: &[u8]) -> GameBoyPrinterJob {
    const MARGIN: usize = 8;
    const HEIGHT: usize = 144;
    let mut pixels = Vec::with_capacity(GAME_BOY_PRINTER_WIDTH * HEIGHT);
    for y in MARGIN..MARGIN + HEIGHT {
        for x in MARGIN..MARGIN + GAME_BOY_PRINTER_WIDTH {
            let gray = rgba[(y * LEGACY_PRINTER_IMAGE_W + x) * 4];
            pixels.push(match gray {
                213..=u8::MAX => 0,
                128..=212 => 1,
                32..=127 => 2,
                _ => 3,
            });
        }
    }
    GameBoyPrinterJob {
        pixels,
        height: HEIGHT,
        copies: 1,
        feed_before: 0,
        feed_after: 0,
        palette: 0xE4,
        density: 0x40,
    }
}

fn validate_parser_state(
    state: ParserState,
    header_pos: usize,
    payload_expected: usize,
    payload_received: usize,
) -> Result<()> {
    let valid = match state {
        ParserState::Magic1 | ParserState::Magic2 => header_pos == 0,
        ParserState::Header => header_pos < 4,
        ParserState::Payload => {
            header_pos == 4 && payload_expected > 0 && payload_received < payload_expected
        }
        ParserState::ChecksumLow
        | ParserState::ChecksumHigh
        | ParserState::DeviceId
        | ParserState::Status => header_pos == 4 && payload_received == payload_expected,
    };
    if !valid {
        bail!("inconsistent Game Boy Printer parser state");
    }
    Ok(())
}
