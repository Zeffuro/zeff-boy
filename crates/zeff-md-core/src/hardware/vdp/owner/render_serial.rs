use super::{domain::Storage, ram};
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Access {
    Random,
    Transfer,
    Refresh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Levels {
    row: bool,
    column: bool,
    transfer: bool,
    clock: bool,
    enabled: bool,
}

impl Levels {
    fn pins(pins: ram::Pins) -> Self {
        Self {
            row: pins.ras,
            column: pins.cas,
            transfer: pins.oe,
            clock: pins.sc,
            enabled: !pins.se0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SerialPort {
    previous: Levels,
    row: u8,
    column: u8,
    access: Access,
    pending: bool,
    cells: [u8; 256],
    cursor: u8,
    output: u8,
    resolved: u8,
}

impl SerialPort {
    pub fn software_origin() -> Self {
        Self {
            previous: Levels {
                row: true,
                column: true,
                transfer: true,
                clock: false,
                enabled: true,
            },
            row: 0,
            column: 0,
            access: Access::Random,
            pending: false,
            cells: [0; 256],
            cursor: 0,
            output: 0,
            resolved: 0,
        }
    }

    pub fn input(&self) -> u8 {
        self.resolved
    }

    pub fn validate_response(&self, pins: ram::Pins, origin: bool) -> Result<()> {
        if !origin {
            ensure!(
                pins.se0 != pins.se1 && self.previous == Levels::pins(pins),
                "serial VRAM response frontier differs from controller pins"
            );
        }
        Ok(())
    }

    pub fn respond(
        &mut self,
        pins: ram::Pins,
        response: ram::Response,
        storage: &impl Storage,
    ) -> Result<()> {
        ensure!(pins.se0 != pins.se1, "invalid serial VRAM select pins");
        let mut candidate = self.clone();
        candidate.advance(pins, response, storage)?;
        *self = candidate;
        Ok(())
    }

    fn advance(
        &mut self,
        pins: ram::Pins,
        response: ram::Response,
        storage: &impl Storage,
    ) -> Result<()> {
        let next = Levels::pins(pins);
        let row = self.previous.row && !next.row;
        let column = self.previous.column && !next.column && !next.row;
        let selected = row && next.column && !next.transfer;
        let released = !self.previous.transfer && next.transfer;
        let shifted = !self.previous.clock && next.clock;
        let loaded = self.pending && released;
        ensure!(
            !selected || pins.we0,
            "serial VRAM write transfer is unsupported"
        );
        ensure!(
            !(released && self.access == Access::Transfer && !self.pending && !next.row),
            "repeated serial VRAM transfer release"
        );
        ensure!(!(loaded && shifted), "coincident serial transfer and shift");
        ensure!(
            !(loaded && (row || column)),
            "coincident serial transfer address capture"
        );
        ensure!(
            !(loaded && response.write.is_some()),
            "coincident serial transfer and random write"
        );
        if loaded {
            for (column, byte) in self.cells.iter_mut().enumerate() {
                let physical = u16::from(self.row) << 8 | column as u16;
                let canonical = ram::canonical_address(physical);
                *byte = storage.vram(canonical)?;
            }
            self.cursor = self.column;
            self.pending = false;
        }
        if row {
            self.row = pins.ad;
            self.pending = selected;
            self.access = if !next.column {
                Access::Refresh
            } else if selected {
                Access::Transfer
            } else {
                Access::Random
            };
        }
        if column && self.access != Access::Refresh {
            self.column = pins.ad;
        }
        if shifted {
            self.output = self.cells[usize::from(self.cursor)];
            self.cursor = self.cursor.wrapping_add(1);
        }
        if next.enabled {
            self.resolved = self.output;
        }
        self.previous = next;
        Ok(())
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(2);
        for level in [
            self.previous.row,
            self.previous.column,
            self.previous.transfer,
            self.previous.clock,
            self.previous.enabled,
        ] {
            writer.write_bool(level);
        }
        writer.write_u8(match self.access {
            Access::Random => 0,
            Access::Transfer => 1,
            Access::Refresh => 2,
        });
        writer.write_bool(self.pending);
        writer.write_u8(self.row);
        writer.write_u8(self.column);
        writer.write_u8(self.cursor);
        writer.write_u8(self.output);
        writer.write_u8(self.resolved);
        writer.write_bytes(&self.cells);
    }

    pub fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        ensure!(reader.read_u8()? == 2, "unsupported serial VRAM state");
        let previous = Levels {
            row: reader.read_bool()?,
            column: reader.read_bool()?,
            transfer: reader.read_bool()?,
            clock: reader.read_bool()?,
            enabled: reader.read_bool()?,
        };
        let access = match reader.read_u8()? {
            0 => Access::Random,
            1 => Access::Transfer,
            2 => Access::Refresh,
            _ => anyhow::bail!("invalid serial VRAM cycle"),
        };
        let pending = reader.read_bool()?;
        let row = reader.read_u8()?;
        let column = reader.read_u8()?;
        let cursor = reader.read_u8()?;
        let output = reader.read_u8()?;
        let resolved = reader.read_u8()?;
        let mut cells = [0; 256];
        reader.read_exact(&mut cells)?;
        ensure!(
            !pending || access == Access::Transfer && !previous.transfer,
            "invalid pending serial VRAM transfer"
        );
        ensure!(
            !previous.enabled || output == resolved,
            "invalid driven serial VRAM output"
        );
        Ok(Self {
            previous,
            row,
            column,
            access,
            pending,
            cells,
            cursor,
            output,
            resolved,
        })
    }
}
