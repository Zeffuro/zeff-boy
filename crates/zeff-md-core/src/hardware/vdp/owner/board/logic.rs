use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Delay {
    pub cycle: u64,
    pub position: u8,
    pub previous: bool,
    pub values: [bool; 7],
}

impl Delay {
    pub const fn zero() -> Self {
        Self {
            cycle: 0,
            position: 0,
            previous: false,
            values: [false; 7],
        }
    }

    pub fn update(&mut self, cycle: u64, value: bool, length: u8) -> Result<bool> {
        ensure!(cycle >= self.cycle, "arbiter delay clock reversal");
        while self.cycle != cycle {
            self.cycle += 1;
            self.position = (self.position + 1) % length;
            self.values[usize::from(self.position)] = self.previous;
        }
        self.values[usize::from(self.position)] = value;
        self.previous = value;
        Ok(self.values[usize::from((self.position + 1) % length)])
    }

    pub fn write(&self, writer: &mut StateWriter) {
        writer.write_u64(self.cycle);
        writer.write_u8(self.position);
        writer.write_bool(self.previous);
        write_bits(writer, self.values);
    }

    pub fn read(reader: &mut StateReader<'_>, length: u8) -> Result<Self> {
        let value = Self {
            cycle: reader.read_u64()?,
            position: reader.read_u8()?,
            previous: reader.read_bool()?,
            values: read_bits(reader)?,
        };
        ensure!(value.position < length, "invalid arbiter delay position");
        ensure!(
            value.values[usize::from(length)..].iter().all(|bit| !bit),
            "invalid arbiter delay tail"
        );
        ensure!(
            value.position == (value.cycle % u64::from(length)) as u8,
            "invalid arbiter delay phase"
        );
        ensure!(
            value.values[usize::from(value.position)] == value.previous,
            "invalid arbiter delay input"
        );
        Ok(value)
    }
}

pub(crate) fn plain(cell: &mut [bool; 2], clock: bool, data: bool) {
    if clock {
        cell[1] = cell[0];
    } else {
        cell[0] = data;
    }
}

pub(crate) fn reset(cell: &mut [bool; 4], clock: bool, data: bool, released: bool) {
    if !released {
        cell[0] = false;
        cell[1] = false;
    } else if clock {
        cell[1] = cell[0];
    } else {
        cell[0] = data;
    }
    cell[2] = !cell[1];
    cell[3] = cell[1];
}

pub(crate) fn set(cell: &mut [bool; 4], clock: bool, data: bool, released: bool) {
    if !clock {
        cell[0] = data;
    } else if !released {
        cell[0] = true;
    }
    if !released {
        cell[1] = true;
    } else if clock {
        cell[1] = cell[0];
    }
    cell[2] = !cell[1];
    cell[3] = cell[1];
}

pub(crate) fn counter(cell: &mut [bool; 3], clock: bool, load: bool, value: bool, carry: bool) {
    cell[2] = carry && cell[1];
    if clock {
        cell[1] = cell[0];
    } else {
        cell[0] = if load { cell[1] ^ carry } else { value };
    }
}

pub(crate) fn write_bits<const N: usize>(writer: &mut StateWriter, bits: [bool; N]) {
    for bit in bits {
        writer.write_bool(bit);
    }
}

pub(crate) fn read_bits<const N: usize>(reader: &mut StateReader<'_>) -> Result<[bool; N]> {
    let mut bits = [false; N];
    for bit in &mut bits {
        *bit = reader.read_bool()?;
    }
    Ok(bits)
}
