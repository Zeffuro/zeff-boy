use crate::save_state::{StateReader, StateWriter};
use anyhow::Result;

const PRINTER_TILE_ROWS: usize = 18;
const PRINTER_TILES_PER_ROW: usize = 20;
const PRINTER_BYTES_PER_TILE_ROW: usize = PRINTER_TILES_PER_ROW * 2;
const PRINTER_IMAGE_SIZE: usize = PRINTER_TILE_ROWS * PRINTER_BYTES_PER_TILE_ROW;

const PRINTER_MARGIN_TOP: usize = 8;
const PRINTER_MARGIN_BOTTOM: usize = 8;
const PRINTER_MARGIN_LEFT: usize = 8;
const PRINTER_MARGIN_RIGHT: usize = 8;

const PRINTER_IMAGE_W: usize = PRINTER_TILES_PER_ROW * 8 + PRINTER_MARGIN_LEFT + PRINTER_MARGIN_RIGHT;
const PRINTER_IMAGE_H: usize = PRINTER_TILE_ROWS * 8 + PRINTER_MARGIN_TOP + PRINTER_MARGIN_BOTTOM;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrinterState {
    Idle,
    ReceivingCommand,
    ReceivingData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrinterCommand {
    Initialize,
    Print,
    Data,
    Unknown(u8),
}

#[derive(Debug)]
pub struct GameboyPrinter {
    state: PrinterState,
    command_bytes: [u8; 5],
    command_pos: usize,
    data_buffer: Vec<u8>,
    data_expected: usize,
    data_pos: usize,
    status: u8,
    images: Vec<Vec<u8>>,
}

impl GameboyPrinter {
    pub(super) fn new() -> Self {
        Self {
            state: PrinterState::Idle,
            command_bytes: [0; 5],
            command_pos: 0,
            data_buffer: vec![0; PRINTER_IMAGE_SIZE],
            data_expected: 0,
            data_pos: 0,
            status: 0x08,
            images: Vec::new(),
        }
    }

    pub(super) fn feed_serial_byte(&mut self, byte: u8) -> u8 {
        match self.state {
            PrinterState::Idle => {
                if byte == 0x88 {
                    self.state = PrinterState::ReceivingCommand;
                    self.command_pos = 0;
                    self.command_bytes = [0; 5];
                    0x00
                } else {
                    0x00
                }
            }
            PrinterState::ReceivingCommand => {
                self.command_bytes[self.command_pos] = byte;
                self.command_pos += 1;

                if self.command_pos == 5 {
                    self.process_command()
                } else {
                    0x00
                }
            }
            PrinterState::ReceivingData => {
                if self.data_pos < self.data_buffer.len() {
                    self.data_buffer[self.data_pos] = byte;
                }
                self.data_pos += 1;

                if self.data_pos >= self.data_expected {
                    self.state = PrinterState::Idle;
                    self.data_pos = 0;
                }
                0x00
            }
        }
    }

    fn process_command(&mut self) -> u8 {
        let command = self.command_bytes[0];
        let _compression = self.command_bytes[1];
        let data_len = u16::from_le_bytes([self.command_bytes[2], self.command_bytes[3]]) as usize;
        let _checksum = self.command_bytes[4];

        let cmd = match command {
            0x01 => PrinterCommand::Initialize,
            0x02 => PrinterCommand::Print,
            0x04 => PrinterCommand::Data,
            other => PrinterCommand::Unknown(other),
        };

        match cmd {
            PrinterCommand::Initialize => {
                self.state = PrinterState::Idle;
                self.data_pos = 0;
                self.status = 0x08;
                0x00
            }
            PrinterCommand::Print => {
                self.state = PrinterState::Idle;
                if !self.data_buffer.is_empty() {
                    let mut image = vec![0u8; PRINTER_IMAGE_W * PRINTER_IMAGE_H * 4];
                    self.decode_image_to_rgba(&mut image);
                    self.images.push(image);
                }
                self.status = 0x08;
                0x00
            }
            PrinterCommand::Data => {
                self.data_expected = data_len;
                self.data_pos = 0;
                self.state = PrinterState::ReceivingData;
                self.status = 0x08;
                0x00
            }
            PrinterCommand::Unknown(_) => {
                self.state = PrinterState::Idle;
                0x00
            }
        }
    }

    fn decode_image_to_rgba(&self, output: &mut [u8]) {
        for row in 0..PRINTER_TILE_ROWS {
            for tile_col in 0..PRINTER_TILES_PER_ROW {
                let tile_idx = row * PRINTER_TILES_PER_ROW + tile_col;
                let tile_data_offset = tile_idx * PRINTER_BYTES_PER_TILE_ROW;

                for tile_row in 0..8 {
                    for tile_col_px in 0..8 {
                        let byte_offset = tile_data_offset + tile_row * PRINTER_TILES_PER_ROW * 2 + tile_col_px / 8 * 2;
                        let bit = 7 - (tile_col_px % 8);

                        let lo = if byte_offset < self.data_buffer.len() {
                            (self.data_buffer[byte_offset] >> bit) & 1
                        } else {
                            0
                        };
                        let hi = if byte_offset + 1 < self.data_buffer.len() {
                            (self.data_buffer[byte_offset + 1] >> bit) & 1
                        } else {
                            0
                        };

                        let color_id = (hi << 1) | lo;
                        let gray = match color_id {
                            0 => 255,
                            1 => 192,
                            2 => 64,
                            _ => 0,
                        };

                        let screen_x = PRINTER_MARGIN_LEFT + tile_col * 8 + tile_col_px;
                        let screen_y = PRINTER_MARGIN_TOP + row * 8 + tile_row;

                        if screen_x < PRINTER_IMAGE_W && screen_y < PRINTER_IMAGE_H {
                            let offset = (screen_y * PRINTER_IMAGE_W + screen_x) * 4;
                            output[offset] = gray;
                            output[offset + 1] = gray;
                            output[offset + 2] = gray;
                            output[offset + 3] = 255;
                        }
                    }
                }
            }
        }
    }

    pub(super) fn latest_image(&self) -> Option<&[u8]> {
        self.images.last().map(|img| img.as_slice())
    }

    pub(super) fn image_count(&self) -> usize {
        self.images.len()
    }

    pub fn image_dimensions() -> (usize, usize) {
        (PRINTER_IMAGE_W, PRINTER_IMAGE_H)
    }

    pub(super) fn clear(&mut self) {
        self.images.clear();
        self.state = PrinterState::Idle;
        self.data_pos = 0;
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.state as u8);
        writer.write_bytes(&self.command_bytes);
        writer.write_u64(self.command_pos as u64);
        writer.write_u64(self.data_expected as u64);
        writer.write_u64(self.data_pos as u64);
        writer.write_u8(self.status);
        writer.write_u64(self.images.len() as u64);
        for image in &self.images {
            writer.write_u64(image.len() as u64);
            writer.write_bytes(image);
        }
    }

    pub(super) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let state_tag = reader.read_u8()?;
        let state = match state_tag {
            0 => PrinterState::Idle,
            1 => PrinterState::ReceivingCommand,
            2 => PrinterState::ReceivingData,
            _ => PrinterState::Idle,
        };
        let mut command_bytes = [0; 5];
        reader.read_exact(&mut command_bytes)?;
        let command_pos = reader.read_u64()? as usize;
        let data_expected = reader.read_u64()? as usize;
        let data_pos = reader.read_u64()? as usize;
        let status = reader.read_u8()?;
        let image_count = reader.read_u64()? as usize;
        let mut images = Vec::with_capacity(image_count);
        for _ in 0..image_count {
            let len = reader.read_u64()? as usize;
            let mut image = vec![0u8; len];
            reader.read_exact(&mut image)?;
            images.push(image);
        }
        Ok(Self {
            state,
            command_bytes,
            command_pos,
            data_buffer: vec![0; PRINTER_IMAGE_SIZE],
            data_expected,
            data_pos,
            status,
            images,
        })
    }
}
