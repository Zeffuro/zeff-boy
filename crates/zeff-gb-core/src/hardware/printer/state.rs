use super::*;
use crate::save_state::{StateReader, StateWriter};

impl GameboyPrinter {
    pub(in crate::hardware) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(STATE_FORMAT_MARKER | self.state.encode());
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
        writer.write_u64(self.images.len() as u64);
        for image in &self.images {
            write_vec(writer, image);
        }
    }

    pub(in crate::hardware) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let state_tag = reader.read_u8()?;
        if state_tag & STATE_FORMAT_MARKER == 0 {
            bail!("Game Boy Printer state is missing the format-6 marker");
        }

        let state = ParserState::decode(state_tag & !STATE_FORMAT_MARKER)?;
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
        let images = read_images(reader)?;

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
            images,
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
        let images = read_legacy_images(reader)?;
        Ok(Self {
            status,
            images,
            ..Self::new()
        })
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

fn read_images(reader: &mut StateReader<'_>) -> Result<Vec<Vec<u8>>> {
    let count = read_len(reader, MAX_SAVED_IMAGES, "image count")?;
    let mut images = Vec::with_capacity(count);
    for _ in 0..count {
        let image = read_vec(reader, PRINTER_RGBA_SIZE, "RGBA image")?;
        if image.len() != PRINTER_RGBA_SIZE {
            bail!("Game Boy Printer RGBA image has invalid length");
        }
        images.push(image);
    }
    Ok(images)
}

fn read_legacy_images(reader: &mut StateReader<'_>) -> Result<Vec<Vec<u8>>> {
    let count = read_len(reader, MAX_SAVED_IMAGES, "legacy image count")?;
    let mut images = Vec::with_capacity(count);
    for _ in 0..count {
        let len = read_len(reader, PRINTER_RGBA_SIZE, "legacy RGBA image")?;
        let mut image = vec![0; len];
        reader.read_exact(&mut image)?;
        if image.len() == PRINTER_RGBA_SIZE {
            images.push(image);
        }
    }
    Ok(images)
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
