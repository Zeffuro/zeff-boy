use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Responder {
    pub address: u16,
    pub previous: Pins,
    pub transfer: bool,
    pub write_sequence: u64,
}

impl Responder {
    pub fn software_origin() -> Self {
        Self {
            address: 0,
            previous: Pins::software_origin(),
            transfer: false,
            write_sequence: 0,
        }
    }

    pub fn evaluate(
        &mut self,
        pins: Pins,
        mut bus: BusInputs,
        mut read_byte: impl FnMut(u16) -> Result<u8>,
    ) -> Result<Response> {
        let old = self.previous;
        let old_column = !old.ras && !old.cas;
        let old_write = old_column && !old.we0;
        let column = !pins.ras && !pins.cas;
        let read = column && !pins.oe && !self.transfer;
        let mut transfer = None;
        if old.ras && !pins.ras {
            self.address = (self.address & 255) | (u16::from(pins.ad) << 8);
            self.transfer = !pins.oe;
            if self.transfer {
                transfer = Some(RowTransfer { row: pins.ad });
            }
        }
        if !old_column && column {
            self.address = (self.address & 0xff00) | u16::from(pins.ad);
        }
        let write = if !old_write && column && !pins.we0 {
            ensure!(pins.driven, "VRAM write without AD driver");
            self.write_sequence = self
                .write_sequence
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("VRAM write receipt overflow"))?;
            Some(ByteWrite {
                sequence: self.write_sequence,
                physical_address: self.address,
                canonical_address: canonical_address(self.address),
                value: pins.ad,
            })
        } else {
            None
        };
        if pins.driven {
            bus.ad = pins.ad;
            bus.rd = pins.rd;
        }
        let old_read = old_column && !old.oe && !self.transfer;
        let read = if !old_read && read {
            let canonical_address = canonical_address(self.address);
            let value = match write {
                Some(write) if write.canonical_address == canonical_address => write.value,
                _ => read_byte(canonical_address)?,
            };
            bus.ad = value;
            Some(ByteRead {
                physical_address: self.address,
                canonical_address,
                value,
            })
        } else {
            None
        };
        self.previous = pins;
        Ok(Response {
            bus,
            write,
            read,
            transfer,
        })
    }
}

pub(in crate::hardware::vdp) fn canonical_address(physical: u16) -> u16 {
    // Row/column permutation precedes the core's byte-lane swap.
    ((physical & 3) | ((physical >> 6) & 0x3fc) | ((physical & 0xfc) << 8)) ^ 1
}
