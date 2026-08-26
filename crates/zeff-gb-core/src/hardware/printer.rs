use crate::hardware::serial::SerialDevice;
use crate::hardware::types::constants::GB_T_CYCLES_PER_SECOND;
use anyhow::{Result, bail};

const DEVICE_ID: u8 = 0x81;
const BUFFER_CAPACITY: usize = 8 * 1024;
const MAX_PACKET_PAYLOAD: usize = u16::MAX as usize;
const MAX_SAVED_JOBS: usize = 1024;
const PRINT_BANDS_PER_SECOND_NUMERATOR: u64 = 11;
const PRINT_BANDS_PER_SECOND_DENOMINATOR: u64 = 10;

const STATUS_CHECKSUM_ERROR: u8 = 0x01;
const STATUS_BUSY: u8 = 0x02;
const STATUS_BUFFER_FULL: u8 = 0x04;
const STATUS_UNPROCESSED_DATA: u8 = 0x08;
const STATUS_PACKET_ERROR: u8 = 0x10;
const TRANSIENT_ERROR_MASK: u8 = STATUS_CHECKSUM_ERROR | STATUS_PACKET_ERROR;

const PRINTER_TILES_PER_ROW: usize = 20;
const BYTES_PER_TILE: usize = 16;
const BYTES_PER_TILE_ROW: usize = PRINTER_TILES_PER_ROW * BYTES_PER_TILE;
const LEGACY_PRINTER_IMAGE_W: usize = 176;
const LEGACY_PRINTER_IMAGE_H: usize = 160;
const LEGACY_PRINTER_RGBA_SIZE: usize = LEGACY_PRINTER_IMAGE_W * LEGACY_PRINTER_IMAGE_H * 4;
const LEGACY_STATE_FORMAT_MARKER: u8 = 0x80;
const LOGICAL_STATE_FORMAT_MARKER: u8 = 0xC0;

pub const GAME_BOY_PRINTER_WIDTH: usize = PRINTER_TILES_PER_ROW * 8;
pub const GAME_BOY_PRINTER_MAX_HEIGHT: usize = BUFFER_CAPACITY / BYTES_PER_TILE_ROW * 8;
pub const GAME_BOY_PRINTER_FEED_HEIGHT: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameBoyPrinterJob {
    pub pixels: Vec<u8>,
    pub height: usize,
    pub copies: u8,
    pub feed_before: u8,
    pub feed_after: u8,
    pub palette: u8,
    pub density: u8,
}

impl GameBoyPrinterJob {
    pub fn validate(&self) -> Result<()> {
        if self.height > GAME_BOY_PRINTER_MAX_HEIGHT || !self.height.is_multiple_of(8) {
            bail!("invalid Game Boy Printer job height: {}", self.height);
        }
        let expected = GAME_BOY_PRINTER_WIDTH
            .checked_mul(self.height)
            .ok_or_else(|| anyhow::anyhow!("Game Boy Printer job dimensions overflow"))?;
        if self.pixels.len() != expected || self.pixels.iter().any(|pixel| *pixel > 3) {
            bail!("invalid Game Boy Printer logical pixel data");
        }
        if self.feed_before > 0x0F || self.feed_after > 0x0F || self.density > 0x7F {
            bail!("invalid Game Boy Printer job parameters");
        }
        Ok(())
    }

    pub fn has_image(&self) -> bool {
        self.copies != 0 && self.height != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParserState {
    Magic1,
    Magic2,
    Header,
    Payload,
    ChecksumLow,
    ChecksumHigh,
    DeviceId,
    Status,
}

impl ParserState {
    fn encode(self) -> u8 {
        match self {
            Self::Magic1 => 0,
            Self::Magic2 => 1,
            Self::Header => 2,
            Self::Payload => 3,
            Self::ChecksumLow => 4,
            Self::ChecksumHigh => 5,
            Self::DeviceId => 6,
            Self::Status => 7,
        }
    }

    fn decode(tag: u8) -> Result<Self> {
        Ok(match tag {
            0 => Self::Magic1,
            1 => Self::Magic2,
            2 => Self::Header,
            3 => Self::Payload,
            4 => Self::ChecksumLow,
            5 => Self::ChecksumHigh,
            6 => Self::DeviceId,
            7 => Self::Status,
            _ => bail!("invalid Game Boy Printer parser state: {tag}"),
        })
    }
}

#[derive(Debug)]
pub struct GameboyPrinter {
    state: ParserState,
    header: [u8; 4],
    header_pos: usize,
    payload: Vec<u8>,
    payload_expected: usize,
    payload_received: usize,
    checksum: u16,
    received_checksum: u16,
    status: u8,
    response_status: u8,
    finish_print_after_status: bool,
    busy_cycles_remaining: u64,
    data_buffer: Vec<u8>,
    data_end_seen: bool,
    jobs: Vec<GameBoyPrinterJob>,
}

impl GameboyPrinter {
    pub(super) fn new() -> Self {
        Self {
            state: ParserState::Magic1,
            header: [0; 4],
            header_pos: 0,
            payload: Vec::new(),
            payload_expected: 0,
            payload_received: 0,
            checksum: 0,
            received_checksum: 0,
            status: 0,
            response_status: 0,
            finish_print_after_status: false,
            busy_cycles_remaining: 0,
            data_buffer: Vec::with_capacity(BUFFER_CAPACITY),
            data_end_seen: false,
            jobs: Vec::new(),
        }
    }

    pub(super) fn feed_serial_byte(&mut self, byte: u8) -> u8 {
        match self.state {
            ParserState::Magic1 => {
                if byte == 0x88 {
                    self.state = ParserState::Magic2;
                }
                0
            }
            ParserState::Magic2 => {
                if byte == 0x33 {
                    self.start_packet();
                } else if byte != 0x88 {
                    self.state = ParserState::Magic1;
                }
                0
            }
            ParserState::Header => {
                self.header[self.header_pos] = byte;
                self.header_pos += 1;
                self.checksum = self.checksum.wrapping_add(u16::from(byte));
                if self.header_pos == self.header.len() {
                    self.payload_expected =
                        usize::from(u16::from_le_bytes([self.header[2], self.header[3]]));
                    self.payload.reserve(self.payload_expected);
                    self.state = if self.payload_expected == 0 {
                        ParserState::ChecksumLow
                    } else {
                        ParserState::Payload
                    };
                }
                0
            }
            ParserState::Payload => {
                self.checksum = self.checksum.wrapping_add(u16::from(byte));
                self.payload.push(byte);
                self.payload_received += 1;
                if self.payload_received == self.payload_expected {
                    self.state = ParserState::ChecksumLow;
                }
                0
            }
            ParserState::ChecksumLow => {
                self.received_checksum = u16::from(byte);
                self.state = ParserState::ChecksumHigh;
                0
            }
            ParserState::ChecksumHigh => {
                self.received_checksum |= u16::from(byte) << 8;
                self.finish_packet();
                self.state = ParserState::DeviceId;
                0
            }
            ParserState::DeviceId => {
                self.state = ParserState::Status;
                DEVICE_ID
            }
            ParserState::Status => {
                let response = self.response_status;
                if self.finish_print_after_status {
                    self.finish_print_after_status = false;
                    self.status &= !STATUS_UNPROCESSED_DATA;
                    self.data_buffer.clear();
                    self.data_end_seen = false;
                }
                self.reset_parser();
                response
            }
        }
    }

    fn start_packet(&mut self) {
        self.reset_parser();
        self.state = ParserState::Header;
    }

    fn reset_parser(&mut self) {
        self.state = ParserState::Magic1;
        self.header = [0; 4];
        self.header_pos = 0;
        self.payload.clear();
        self.payload_expected = 0;
        self.payload_received = 0;
        self.checksum = 0;
        self.received_checksum = 0;
    }

    fn finish_packet(&mut self) {
        self.status &= !TRANSIENT_ERROR_MASK;
        let status_before_command = self.status;
        if self.received_checksum != self.checksum {
            self.status |= STATUS_CHECKSUM_ERROR;
            self.response_status = self.status;
        } else {
            self.execute_command();
            self.response_status =
                if self.header[0] == 0x01 || self.status & TRANSIENT_ERROR_MASK != 0 {
                    self.status
                } else {
                    status_before_command
                };
        }
    }

    fn execute_command(&mut self) {
        let command = self.header[0];
        let compression = self.header[1];
        match command {
            0x01 if compression == 0 && self.payload.is_empty() => self.initialize(),
            0x02 if compression == 0 && self.payload.len() == 4 => self.print(),
            0x04 => self.receive_data(compression),
            0x08 if compression == 0 && self.payload.is_empty() => self.break_print(),
            0x0F if compression == 0 && self.payload.is_empty() => {}
            _ => self.status |= STATUS_PACKET_ERROR,
        }
    }

    fn initialize(&mut self) {
        self.status = 0;
        self.response_status = 0;
        self.finish_print_after_status = false;
        self.busy_cycles_remaining = 0;
        self.data_buffer.clear();
        self.data_end_seen = false;
    }

    fn receive_data(&mut self, compression: u8) {
        if self.payload.is_empty() {
            if compression == 0 {
                self.data_end_seen = true;
            } else {
                self.status |= STATUS_PACKET_ERROR;
            }
            return;
        }

        let decoded = match compression {
            0 => self.payload.clone(),
            1 => match decode_rle(&self.payload) {
                Some(decoded) => decoded,
                None => {
                    self.status |= STATUS_PACKET_ERROR;
                    return;
                }
            },
            _ => {
                self.status |= STATUS_PACKET_ERROR;
                return;
            }
        };

        let Some(new_len) = self.data_buffer.len().checked_add(decoded.len()) else {
            self.status |= STATUS_BUFFER_FULL | STATUS_PACKET_ERROR;
            return;
        };
        if new_len > BUFFER_CAPACITY {
            self.status |= STATUS_BUFFER_FULL;
            return;
        }

        self.data_buffer.extend_from_slice(&decoded);
        self.data_end_seen = false;
        self.status |= STATUS_UNPROCESSED_DATA;
        if self.data_buffer.len() == BUFFER_CAPACITY {
            self.status |= STATUS_BUFFER_FULL;
        }
    }

    fn print(&mut self) {
        if !self.data_end_seen {
            return;
        }

        let mut job = self.build_job();
        if !job.has_image() {
            job.pixels.clear();
            job.height = 0;
            job.feed_before = 0;
        }
        if job.has_image() || job.feed_after != 0 {
            if self.jobs.len() == MAX_SAVED_JOBS {
                self.jobs.remove(0);
            }
            self.jobs.push(job);
        }
        self.status |= STATUS_BUFFER_FULL;
        self.busy_cycles_remaining = self.print_duration_cycles();
        if self.busy_cycles_remaining != 0 {
            self.status |= STATUS_BUSY;
        }
        self.finish_print_after_status = true;
    }

    fn break_print(&mut self) {
        self.status &= !STATUS_BUSY;
        self.busy_cycles_remaining = 0;
        self.finish_print_after_status = false;
    }

    fn print_duration_cycles(&self) -> u64 {
        let copies = u64::from(self.payload[0]);
        let image_bands = if copies == 0 {
            0
        } else {
            self.data_buffer.len().div_ceil(BYTES_PER_TILE_ROW * 2) as u64
        };
        let feed_before = if image_bands == 0 {
            0
        } else {
            u64::from(self.payload[1] >> 4)
        };
        let feed_after = u64::from(self.payload[1] & 0x0F);
        let total_bands = image_bands
            .saturating_add(feed_before)
            .saturating_add(feed_after)
            .saturating_mul(copies.max(1));
        total_bands
            .saturating_mul(GB_T_CYCLES_PER_SECOND * PRINT_BANDS_PER_SECOND_DENOMINATOR)
            .div_ceil(PRINT_BANDS_PER_SECOND_NUMERATOR)
    }

    pub(super) fn step(&mut self, t_cycles: u64) {
        if self.busy_cycles_remaining == 0 {
            return;
        }
        self.busy_cycles_remaining = self.busy_cycles_remaining.saturating_sub(t_cycles);
        if self.busy_cycles_remaining == 0 {
            self.status &= !STATUS_BUSY;
        }
    }

    fn build_job(&self) -> GameBoyPrinterJob {
        let tile_rows = self
            .data_buffer
            .len()
            .div_ceil(BYTES_PER_TILE_ROW)
            .min(GAME_BOY_PRINTER_MAX_HEIGHT / 8);
        let height = tile_rows * 8;
        let mut pixels = vec![0; GAME_BOY_PRINTER_WIDTH * height];
        for tile_row in 0..tile_rows {
            for tile_col in 0..PRINTER_TILES_PER_ROW {
                let tile_offset = (tile_row * PRINTER_TILES_PER_ROW + tile_col) * BYTES_PER_TILE;
                for row in 0..8 {
                    let lo = self
                        .data_buffer
                        .get(tile_offset + row * 2)
                        .copied()
                        .unwrap_or(0);
                    let hi = self
                        .data_buffer
                        .get(tile_offset + row * 2 + 1)
                        .copied()
                        .unwrap_or(0);
                    for col in 0..8 {
                        let bit = 7 - col;
                        let color = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);
                        let x = tile_col * 8 + col;
                        let y = tile_row * 8 + row;
                        pixels[y * GAME_BOY_PRINTER_WIDTH + x] = color;
                    }
                }
            }
        }
        GameBoyPrinterJob {
            pixels,
            height,
            copies: self.payload[0],
            feed_before: self.payload[1] >> 4,
            feed_after: self.payload[1] & 0x0F,
            palette: self.payload[2],
            density: self.payload[3] & 0x7F,
        }
    }

    pub(super) fn latest_job(&self) -> Option<&GameBoyPrinterJob> {
        self.jobs.last()
    }

    pub(super) fn job_count(&self) -> usize {
        self.jobs.len()
    }

    pub(super) fn take_jobs(&mut self) -> Vec<GameBoyPrinterJob> {
        std::mem::take(&mut self.jobs)
    }

    pub(super) fn clear(&mut self) {
        self.jobs.clear();
    }

    pub(super) fn reconnect(&mut self) {
        self.reset_parser();
        self.initialize();
    }
}

fn decode_rle(encoded: &[u8]) -> Option<Vec<u8>> {
    let mut decoded = Vec::new();
    let mut pos = 0;
    while pos < encoded.len() {
        let control = encoded[pos];
        pos += 1;
        if control & 0x80 == 0 {
            let len = usize::from(control) + 1;
            let end = pos.checked_add(len)?;
            let literal = encoded.get(pos..end)?;
            if decoded.len().checked_add(len)? > BUFFER_CAPACITY {
                return None;
            }
            decoded.extend_from_slice(literal);
            pos = end;
        } else {
            let value = *encoded.get(pos)?;
            pos += 1;
            let len = usize::from(control & 0x7F) + 2;
            if decoded.len().checked_add(len)? > BUFFER_CAPACITY {
                return None;
            }
            decoded.resize(decoded.len() + len, value);
        }
    }
    Some(decoded)
}

impl SerialDevice for GameboyPrinter {
    fn exchange_byte(&mut self, byte: u8) -> u8 {
        self.feed_serial_byte(byte)
    }
}

mod state;

#[cfg(test)]
mod tests;
