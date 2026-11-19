use crate::hardware::vdp::control_core::PhaseLevels;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

pub(super) const WORD_MASK: u64 = (1 << 56) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ReadWord {
    pub value: u64,
    pub known: u64,
}

impl ReadWord {
    pub const UNKNOWN: Self = Self { value: 0, known: 0 };
    pub const ONES: Self = Self {
        value: WORD_MASK,
        known: WORD_MASK,
    };

    fn row(row: [u8; 8]) -> Self {
        let mut value = 0;
        for (lane, byte) in row.into_iter().enumerate() {
            value |= u64::from(byte) << (lane * 7);
        }
        Self {
            value,
            known: WORD_MASK,
        }
    }

    fn and(self, other: Self) -> Self {
        let value = self.value & other.value;
        let zero = (self.known & !self.value) | (other.known & !other.value);
        Self {
            value,
            known: (zero | value) & WORD_MASK,
        }
    }

    fn merge(self, other: Self) -> Self {
        let known = self.known & other.known & !(self.value ^ other.value);
        Self {
            value: self.value & known,
            known,
        }
    }

    pub fn lanes(self) -> ([u8; 8], [u8; 8]) {
        let mut values = [0; 8];
        let mut masks = [0; 8];
        for i in 0..8 {
            values[i] = ((self.value >> (i * 7)) & 127) as u8;
            masks[i] = ((self.known >> (i * 7)) & 127) as u8;
        }
        (values, masks)
    }

    pub fn write(self, w: &mut StateWriter) {
        w.write_u64(self.value);
        w.write_u64(self.known);
    }

    pub fn read(r: &mut StateReader<'_>) -> Result<Self> {
        let word = Self {
            value: r.read_u64()?,
            known: r.read_u64()?,
        };
        ensure!(
            word.known & !WORD_MASK == 0 && word.value & !word.known == 0,
            "noncanonical sprite line read word"
        );
        Ok(word)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Edge {
    common: ReadWord,
    parity: [ReadWord; 2],
    rows: [Option<u8>; 2],
    write: Option<(u8, [u8; 8])>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LinePort {
    pub common: ReadWord,
    pub parity: [ReadWord; 2],
    pub rows: [Option<u8>; 2],
    pub output_known: [u8; 8],
    pending: Option<Edge>,
}

#[derive(Clone, Copy)]
pub(super) struct PortPins {
    pub address: Option<u8>,
    pub enabled: bool,
    pub data_master: bool,
    pub writes: [bool; 8],
    pub incoming: [u8; 8],
}

impl LinePort {
    pub fn legacy_unknown() -> Self {
        Self {
            common: ReadWord::UNKNOWN,
            parity: [ReadWord::UNKNOWN; 2],
            rows: [None; 2],
            output_known: [127; 8],
            pending: None,
        }
    }

    pub fn begin_edge(
        &mut self,
        pins: PortPins,
        phases: PhaseLevels,
        horizontal_input: u16,
        lines: &[[u8; 8]; 40],
    ) -> Result<()> {
        ensure!(self.pending.is_none(), "sprite line edge already pending");
        ensure!(
            pins.address.is_none_or(|v| v < 64),
            "invalid sprite line address"
        );
        let mut edge = Edge {
            common: self.common,
            parity: self.parity,
            rows: self.rows,
            write: None,
        };
        if phases.data_master {
            edge.rows[0] = Some((horizontal_input >> 3) as u8);
        }
        if phases.data_slave {
            edge.rows[1] = self.rows[0];
        }
        let writing = pins.enabled && pins.data_master && pins.writes.into_iter().any(|v| v);
        ensure!(
            !writing || pins.address.is_some(),
            "unknown sprite data-row address at line write; parity=unknown missing=3f"
        );
        let mut merged: Option<(ReadWord, [ReadWord; 2])> = None;
        let addresses = pins.address.map_or(0..64u8, |address| address..address + 1);
        for address in addresses {
            let lane = usize::from(address & 1);
            let mut parity = self.parity;
            let common = if let Some(row) = lines.get(usize::from(address)) {
                let word = ReadWord::row(*row);
                parity[lane] = word;
                word
            } else {
                self.common.and(self.parity[lane])
            };
            merged = Some(if let Some((old, old_parity)) = merged {
                (
                    old.merge(common),
                    [
                        old_parity[0].merge(parity[0]),
                        old_parity[1].merge(parity[1]),
                    ],
                )
            } else {
                (common, parity)
            });
            if writing && let Some(old) = lines.get(usize::from(address)) {
                let mut row = *old;
                for (i, byte) in row.iter_mut().enumerate() {
                    if pins.writes[i] {
                        *byte = pins.incoming[i];
                    }
                }
                edge.write = Some((address, row));
            }
        }
        let (common, parity) = merged.expect("six-bit sprite address has a candidate");
        edge.common = common;
        edge.parity = parity;
        self.pending = Some(edge);
        Ok(())
    }

    pub fn finish_edge(&mut self, lines: &mut [[u8; 8]; 40]) -> Result<bool> {
        let edge = self
            .pending
            .take()
            .ok_or_else(|| anyhow::anyhow!("sprite line edge was not prepared"))?;
        let changed = self.common != edge.common || self.rows != edge.rows;
        if let Some((address, row)) = edge.write {
            lines[usize::from(address)] = row;
        }
        self.common = edge.common;
        self.parity = edge.parity;
        self.rows = edge.rows;
        Ok(changed)
    }

    pub fn bus(&self, enabled: bool) -> ReadWord {
        if enabled { self.common } else { ReadWord::ONES }
    }

    pub fn validate(&self, output: [u8; 8]) -> Result<()> {
        ensure!(self.pending.is_none(), "incomplete sprite line MCLK edge");
        ensure!(
            self.rows.into_iter().flatten().all(|v| v < 64),
            "invalid sprite data row"
        );
        ensure!(
            self.output_known
                .into_iter()
                .zip(output)
                .all(|(known, value)| known < 128 && value & !known == 0),
            "noncanonical sprite line output"
        );
        Ok(())
    }
}
